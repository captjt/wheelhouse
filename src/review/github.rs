//! GitHub, through your `gh` login: the PRs that request you, a round's
//! context, and posting. Agents never call GitHub; only the hub does.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::rules::Obs;

pub fn gh_bin() -> PathBuf {
    std::env::var_os("WH_GH").map(PathBuf::from).or_else(|| crate::paths::which("gh")).unwrap_or_else(|| "gh".into())
}

/// The PATH every helper command runs with: Homebrew and the usual places
/// first, since a LaunchAgent's PATH is short.
pub fn command_path() -> String {
    let home = crate::paths::home();
    let h = home.to_string_lossy();
    let mut dirs: Vec<String> = [
        format!("{h}/.local/bin"),
        format!("{h}/.cargo/bin"),
        "/opt/homebrew/bin".into(),
        "/opt/homebrew/sbin".into(),
        "/usr/local/bin".into(),
        "/usr/bin".into(),
        "/bin".into(),
        "/usr/sbin".into(),
        "/sbin".into(),
    ]
    .into();
    if let Ok(p) = std::env::var("PATH") {
        for d in p.split(':') {
            if !d.is_empty() && !dirs.iter().any(|x| x == d) {
                dirs.push(d.to_string());
            }
        }
    }
    dirs.join(":")
}

/// `gh ARGS…`, with `stdin` if given; its stdout.
pub async fn gh(args: &[&str], stdin: Option<String>) -> Result<String> {
    gh_run(args, stdin, false).await
}

/// `gh api graphql …`: GitHub answers what it can, with `errors` beside the
/// partial `data` (one tracked repo deleted or out of reach), and `gh`
/// exits 1. The partial answer is kept, so one bad repo doesn't blind the
/// scan to every other PR.
pub async fn gh_graphql(args: &[&str]) -> Result<String> {
    gh_run(args, None, true).await
}

async fn gh_run(args: &[&str], stdin: Option<String>, partial: bool) -> Result<String> {
    let mut c = Command::new(gh_bin());
    c.args(args).env("PATH", command_path()).env("GH_PROMPT_DISABLED", "1").env("NO_COLOR", "1").current_dir(crate::paths::home());
    c.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).kill_on_drop(true);
    let mut child = c.spawn().with_context(|| format!("running {}", gh_bin().display()))?;
    if let Some(input) = stdin {
        let mut si = child.stdin.take().context("gh's stdin")?;
        si.write_all(input.as_bytes()).await?;
        drop(si);
    }
    let out = tokio::time::timeout(std::time::Duration::from_secs(120), child.wait_with_output())
        .await
        .map_err(|_| anyhow!("gh {} took over 2 minutes", args.first().unwrap_or(&"")))??;
    if !out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if partial && serde_json::from_str::<Value>(&stdout).is_ok_and(|v| v["data"].is_object()) {
            say!("gh: a partial answer: {}", String::from_utf8_lossy(&out.stderr).trim());
            return Ok(stdout.into_owned());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        bail!("{}", if err.is_empty() { stdout.trim().to_string() } else { err.to_string() });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

static LOGIN: OnceLock<String> = OnceLock::new();

/// The signed-in GitHub account.
pub async fn login() -> Result<String> {
    if let Some(l) = LOGIN.get() {
        return Ok(l.clone());
    }
    let l = gh(&["api", "user", "--jq", ".login"], None).await?.trim().to_string();
    if l.is_empty() {
        bail!("gh returned no signed-in user; run gh auth login");
    }
    let _ = LOGIN.set(l.clone());
    Ok(l)
}

const PR_FIELDS: &str = "number title url state isDraft headRefOid author { login }";

/// One check: the PRs in `repos` that request you by name, and every PR in
/// `tracked`, however it stands now. Returns your login and an observation
/// per PR, by repo and number.
pub async fn scan(repos: &[String], tracked: &[(String, i64)]) -> Result<(String, Vec<Obs>)> {
    let search = std::iter::once("is:pr is:open draft:false user-review-requested:@me".to_string()).chain(repos.iter().map(|r| format!("repo:{r}"))).collect::<Vec<_>>().join(" ");
    let mut q = String::from("query($q: String!) {\n  viewer { login }\n");
    q.push_str(
        "  search(query: $q, type: ISSUE, first: 100) { nodes { ... on PullRequest {\n    number title url isDraft headRefOid author { login } repository { nameWithOwner }\n    timelineItems(last: 20, itemTypes: [REVIEW_REQUESTED_EVENT]) {\n      nodes { ... on ReviewRequestedEvent { createdAt requestedReviewer { ... on User { login } } } }\n    }\n  } } }\n",
    );
    let mut by_repo: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for (r, n) in tracked {
        by_repo.entry(r.as_str()).or_default().push(*n);
    }
    let mut aliases: Vec<(String, String)> = Vec::new();
    for (i, (repo, prs)) in by_repo.iter().enumerate() {
        let Some((owner, name)) = repo.split_once('/') else { continue };
        let alias = format!("r{}", i + 1);
        q.push_str(&format!("  {alias}: repository(owner: {}, name: {}) {{\n", json!(owner), json!(name)));
        for n in prs {
            q.push_str(&format!("    p{n}: pullRequest(number: {n}) {{ {PR_FIELDS} }}\n"));
        }
        q.push_str("  }\n");
        aliases.push((alias, repo.to_string()));
    }
    q.push('}');
    let out = gh_graphql(&["api", "graphql", "-f", &format!("q={search}"), "-f", &format!("query={q}")]).await?;
    let v: Value = serde_json::from_str(&out).context("reading GitHub's answer")?;
    let data = &v["data"];
    let login = data["viewer"]["login"].as_str().ok_or_else(|| anyhow!("gh returned no signed-in user; run gh auth login"))?.to_string();
    let _ = LOGIN.set(login.clone());

    let mut obs: BTreeMap<(String, i64), Obs> = BTreeMap::new();
    let s = |v: &Value| v.as_str().map(str::to_string);
    for (alias, repo) in &aliases {
        let Some(prs) = data[alias].as_object() else { continue };
        for p in prs.values() {
            let Some(n) = p["number"].as_i64() else { continue };
            obs.insert(
                (repo.clone(), n),
                Obs {
                    repo: repo.clone(),
                    pr: n,
                    title: s(&p["title"]),
                    url: s(&p["url"]),
                    author: s(&p["author"]["login"]),
                    state: s(&p["state"]).unwrap_or_else(|| "OPEN".into()),
                    head: s(&p["headRefOid"]),
                    requested: false,
                    requested_at: None,
                },
            );
        }
    }
    for p in data["search"]["nodes"].as_array().into_iter().flatten() {
        let (Some(repo), Some(n)) = (p["repository"]["nameWithOwner"].as_str(), p["number"].as_i64()) else { continue };
        if !repos.iter().any(|r| r.eq_ignore_ascii_case(repo)) {
            continue;
        }
        let repo = repos.iter().find(|r| r.eq_ignore_ascii_case(repo)).cloned().unwrap_or_else(|| repo.to_string());
        let at = p["timelineItems"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["requestedReviewer"]["login"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(&login)))
            .filter_map(|e| e["createdAt"].as_str())
            .max()
            .map(str::to_string);
        let o = obs.entry((repo.clone(), n)).or_insert_with(|| Obs { repo: repo.clone(), pr: n, state: "OPEN".into(), ..Default::default() });
        o.title = s(&p["title"]).or(o.title.take());
        o.url = s(&p["url"]).or(o.url.take());
        o.author = s(&p["author"]["login"]).or(o.author.take());
        o.head = s(&p["headRefOid"]).or(o.head.take());
        o.requested = true;
        o.requested_at = at;
    }
    Ok((login, obs.into_values().collect()))
}

/// The PR's head on GitHub now.
pub async fn live_head(repo: &str, pr: i64) -> Result<String> {
    Ok(gh(&["pr", "view", &pr.to_string(), "-R", repo, "--json", "headRefOid", "--jq", ".headRefOid"], None).await?.trim().to_string())
}

/// `gh pr view` with these fields.
pub async fn pr_view(repo: &str, pr: i64, fields: &str) -> Result<Value> {
    let out = gh(&["pr", "view", &pr.to_string(), "-R", repo, "--json", fields], None).await?;
    Ok(serde_json::from_str(&out)?)
}

/// `gh api --paginate` prints one array per page; one array of them all.
fn merge_pages(out: &str) -> String {
    let mut all = Vec::new();
    let mut other = None;
    for v in serde_json::Deserializer::from_str(out).into_iter::<Value>() {
        match v {
            Ok(Value::Array(a)) => all.extend(a),
            Ok(v) => other = Some(v),
            Err(_) => return out.to_string(),
        }
    }
    match other {
        Some(v) if all.is_empty() => v.to_string(),
        _ => Value::Array(all).to_string(),
    }
}

const THREADS: &str = "query($o: String!, $n: String!, $pr: Int!) {
  repository(owner: $o, name: $n) { pullRequest(number: $pr) {
    reviewThreads(first: 100) { nodes {
      id isResolved isOutdated path line originalLine
      comments(first: 50) { nodes { databaseId author { login } body createdAt url } }
    } }
  } }
}";

/// Write the round's context from GitHub into `dir`: the PR, its diff, its
/// reviews and comments, its threads and CI. A file that can't be fetched
/// is left out; the agents work with what's there.
pub async fn fetch_context(repo: &str, pr: i64, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let n = pr.to_string();
    let (owner, name) = repo.split_once('/').ok_or_else(|| anyhow!("{repo} isn't owner/name"))?;
    let reviews = format!("repos/{repo}/pulls/{n}/reviews");
    let comments = format!("repos/{repo}/pulls/{n}/comments");
    let issue = format!("repos/{repo}/issues/{n}/comments");
    let threads_q = format!("query={THREADS}");
    let (o, nm, p) = (format!("o={owner}"), format!("n={name}"), format!("pr={n}"));
    let jobs: Vec<(&str, Vec<&str>, bool)> = vec![
        (
            "pr.json",
            vec![
                "pr",
                "view",
                &n,
                "-R",
                repo,
                "--json",
                "number,title,body,author,url,baseRefName,headRefName,headRefOid,isDraft,labels,files,commits,additions,deletions,changedFiles",
            ],
            false,
        ),
        ("pr.diff", vec!["pr", "diff", &n, "-R", repo], false),
        ("reviews.json", vec!["api", "--paginate", &reviews], true),
        ("review-comments.json", vec!["api", "--paginate", &comments], true),
        ("issue-comments.json", vec!["api", "--paginate", &issue], true),
        ("threads.json", vec!["api", "graphql", "-F", &p, "-f", &o, "-f", &nm, "-f", &threads_q], false),
        ("ci.json", vec!["pr", "checks", &n, "-R", repo, "--json", "name,state,bucket,link,workflow"], false),
    ];
    for (file, args, pages) in jobs {
        match gh(&args, None).await {
            Ok(out) => {
                let out = if pages { merge_pages(&out) } else { out };
                super::write_atomic(&dir.join(file), &out)?;
            }
            Err(e) => say!("review: {repo}#{pr}: {file}: {e:#}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginated_arrays_become_one() {
        assert_eq!(merge_pages("[1,2]\n[3]"), "[1,2,3]");
        assert_eq!(merge_pages("[]"), "[]");
        assert_eq!(merge_pages("{\"a\":1}"), "{\"a\":1}");
    }
}
