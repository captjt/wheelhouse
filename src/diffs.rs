//! A docket's code changes, as the apps show them: the diff of its
//! worktree (or the folder its agent works in) from its base, or between
//! any two points on its record (a check's commit, a round's head).
//!
//! Read-only, always: git runs with the hub's safe config, takes no
//! optional locks (an agent's `git commit` never finds our index.lock),
//! never runs a repo's diff drivers or content filters, never fetches a
//! missing object, and reads pathspecs literally. Refs from a request are only
//! `base`, `head`, `work` or a hex commit id, so none can become a flag.
//! Output is capped; a diff over the cap comes back as its file list, and
//! each file's patch is fetched on its own.
//!
//! When the worktree is gone, the repo's main checkout still has the
//! commits; when that's gone too, the patches the record kept with its
//! checks are what's left.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncReadExt;

use crate::hub::Shared;

/// The most patch text one answer carries.
pub const PATCH_MAX: usize = 6 << 20;
/// Untracked files shown as new, at most, and the largest shown in full.
const UNTRACKED_MAX: usize = 200;
const UNTRACKED_FILE_MAX: u64 = 1 << 20;
/// How long one git command may take here: a diff, not a fetch.
const GIT_TIMEOUT: Duration = Duration::from_secs(30);
/// The points a docket offers to compare, at most (the newest are kept).
const POINTS_MAX: usize = 60;

/// Config that keeps the output in the shape the apps parse, whatever the
/// user's own git config says: a/ and b/ prefixes, paths unquoted, from the
/// top of the repo, no color.
/// And nothing reaches the network: a repo's config can make a missing
/// object fetch itself (a "partial clone" with a promisor remote).
const DIFF_CONFIG: [&str; 18] = [
    "-c",
    "diff.noprefix=false",
    "-c",
    "diff.mnemonicPrefix=false",
    "-c",
    "diff.relative=false",
    "-c",
    "core.quotePath=false",
    "-c",
    "color.ui=never",
    "-c",
    "diff.external=",
    "-c",
    "protocol.allow=never",
    "-c",
    "credential.helper=",
    "-c",
    "core.sshCommand=false",
];
/// Diffs answered at once, and how long one may take in all.
static AT_ONCE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
const DIFF_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the untracked files may take to read, before the rest are
/// only counted.
const UNTRACKED_BUDGET: Duration = Duration::from_secs(10);
const DIFF_FLAGS: [&str; 6] = ["--no-ext-diff", "--no-textconv", "--no-color", "-M", "--src-prefix=a/", "--dst-prefix=b/"];

/// Where a docket's code is, and what its change is measured from.
#[derive(Clone, Debug)]
pub struct Place {
    /// The top of the checkout git runs in.
    pub dir: PathBuf,
    /// `worktree` (a Build's or Review's), `folder` (an agent's), or
    /// `checkout` (the repo's main one, once the worktree went).
    pub source: &'static str,
    /// Its uncommitted changes are the docket's: a Build's worktree or an
    /// agent's folder, not a Review's or the repo's main checkout.
    pub live: bool,
    pub repo: Option<String>,
    /// How to find the base: a commit, or a ref to take the merge base with.
    pub base: Base,
    /// What the docket's HEAD is here, first that resolves: `HEAD` in its
    /// own worktree; in the repo's main checkout, its branch or its last
    /// known commit (that checkout's own HEAD is someone else's work).
    pub head: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Base {
    Commit(String),
    MergeBase(Vec<String>),
}

/// A point on the record that names a commit.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub sha: String,
    pub label: String,
    pub at: String,
    pub seq: i64,
    /// A check's patch from `base` to `sha`, kept in the blob store.
    pub patch: Option<(String, String)>,
}

/// What a request asks for: `base`, `head`, `work`, or a commit id.
#[derive(Clone, Debug, PartialEq)]
pub enum Rev {
    Base,
    Head,
    Work,
    Sha(String),
}

impl Rev {
    pub fn parse(s: &str) -> Result<Rev> {
        match s {
            "base" => Ok(Rev::Base),
            "head" | "HEAD" => Ok(Rev::Head),
            "work" => Ok(Rev::Work),
            s if is_hex(s, 7) => Ok(Rev::Sha(s.to_ascii_lowercase())),
            _ => bail!("{s:?} isn't base, head, work or a commit id"),
        }
    }
}

fn is_hex(s: &str, min: usize) -> bool {
    (min..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A path a request names: relative, inside the repo, and never a flag.
pub fn safe_path(p: &str) -> Result<&str> {
    let ok = !p.is_empty()
        && p.len() <= 4096
        && !p.starts_with('/')
        && !p.starts_with('-')
        && !p.contains('\0')
        && !p.contains('\\')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
    if ok { Ok(p) } else { bail!("{p:?} isn't a path in the repo") }
}

// ---- where the code is ----

/// The places to look for a docket's code, best first, and its points.
pub fn locate(shared: &Shared, docket: &str) -> Result<(Vec<Place>, Vec<Point>)> {
    let build_cfg = crate::build::config(shared).ok();
    let review_cfg = crate::review::config(shared).ok();
    let core = shared.lock();
    let st = core.store.state(docket)?.ok_or_else(|| anyhow!("no docket {docket}"))?;
    let mut places = Vec::new();
    if let Some(t) = crate::build::get(&core, docket)? {
        let base = match &t.base_head {
            Some(b) if is_hex(b, 7) => Base::Commit(b.clone()),
            _ => Base::MergeBase(t.base.iter().map(|b| format!("origin/{b}")).collect()),
        };
        if let Some(w) = t.worktree.as_deref() {
            places.push(Place { dir: PathBuf::from(w), source: "worktree", live: true, repo: Some(t.repo.clone()), base: base.clone(), head: vec!["HEAD".into()] });
        }
        let mut head: Vec<String> = t.branch.iter().map(|b| format!("refs/heads/{b}")).collect();
        head.extend([&t.pushed_head, &t.checked_head, &t.built_head].into_iter().flatten().filter(|h| is_hex(h, 40)).cloned());
        if let Some(r) = build_cfg.as_ref().and_then(|c| c.repos.get(&t.repo))
            && !head.is_empty()
        {
            places.push(Place { dir: r.checkout.clone(), source: "checkout", live: false, repo: Some(t.repo.clone()), base, head });
        }
    } else if let Some(rv) = &st.review {
        // The ledger's key is the full `owner/name#n`; the docket shows `name#n`.
        if let Some(p) = crate::review::get(&core, &format!("{}#{}", rv.repo, rv.pr))? {
            let base = Base::MergeBase(p.base.iter().map(|b| format!("origin/{b}")).collect());
            if let Some(w) = p.worktree.as_deref() {
                places.push(Place { dir: PathBuf::from(w), source: "worktree", live: false, repo: Some(p.repo.clone()), base: base.clone(), head: vec!["HEAD".into()] });
            }
            let head: Vec<String> = [&p.round_head, &p.head].into_iter().flatten().filter(|h| is_hex(h, 40)).cloned().collect();
            if let Some(r) = review_cfg.as_ref().and_then(|c| c.repos.get(&p.repo))
                && !head.is_empty()
            {
                places.push(Place { dir: r.checkout.clone(), source: "checkout", live: false, repo: Some(p.repo.clone()), base, head });
            }
        }
    } else if let Some(cwd) = st.home_cwd() {
        let dir = crate::paths::expand(cwd);
        let found = build_cfg.as_ref().and_then(|c| c.repos.iter().find(|(_, r)| dir.starts_with(&r.checkout)));
        let mut refs: Vec<String> = found.and_then(|(_, r)| r.base.clone()).map(|b| format!("origin/{b}")).into_iter().collect();
        refs.extend(["origin/HEAD", "origin/main", "origin/master"].map(String::from));
        // The folder the agent reports it works in: your own agent's, which
        // runs as you; Build's and Review's agents have worktrees instead.
        places.push(Place { dir, source: "folder", live: true, repo: found.map(|(k, _)| k.clone()), base: Base::MergeBase(refs), head: vec!["HEAD".into()] });
    }
    let entries = core.store.entries(docket)?;
    drop(core);
    let mut points: Vec<Point> = Vec::new();
    for s in entries {
        let Ok(e) = s.entry() else { continue };
        let Some(sha) = e.data.get("head").and_then(Value::as_str).filter(|h| h.len() == 40 && is_hex(h, 40)).map(str::to_ascii_lowercase) else { continue };
        let patch = (e.kind == "check").then(|| Some((e.data.get("base")?.as_str().filter(|b| is_hex(b, 7))?.to_string(), e.data.get("patch")?.as_str()?.to_string()))).flatten();
        if let Some(p) = points.iter_mut().find(|p| p.sha == sha) {
            if p.patch.is_none() {
                p.patch = patch;
            }
            continue;
        }
        points.push(Point { sha, label: label_of(&e.kind, e.body.as_deref(), &e.data), at: e.at.clone(), seq: s.seq, patch });
    }
    if points.len() > POINTS_MAX {
        // The newest; and the newest kept patch, which the record falls back on.
        let cut = points.len() - POINTS_MAX;
        let saved = points.iter().rposition(|p| p.patch.is_some()).filter(|&i| i < cut).map(|i| points[i].clone());
        points.drain(..cut);
        if let Some(p) = saved {
            points.insert(0, p);
        }
    }
    Ok((places, points))
}

/// A point's name: what happened at that commit, in a few words.
fn label_of(kind: &str, body: Option<&str>, data: &Value) -> String {
    let s = |k: &str| data.get(k).and_then(Value::as_str);
    let n = |k: &str| data.get(k).and_then(Value::as_i64);
    let named = match (kind, s("what"), s("event")) {
        ("check", ..) => Some("Checks ran".to_string()),
        (_, Some("review-event"), Some("started" | "requested" | "re-requested" | "pickup")) => n("round").map(|r| format!("Review round {r}")),
        (_, Some("review-event"), Some("delta")) => n("round").map(|r| format!("Round {r}: new commits")),
        (_, Some("review-event"), Some("posted")) => n("round").map(|r| format!("Round {r} posted")),
        (_, Some("open-pr"), _) => Some("PR opened".into()),
        (_, Some("push-round"), _) => n("round").map(|r| format!("Round {r} pushed")),
        ("verdict", ..) => n("n").map(|r| format!("Check {r} verdict")),
        _ => None,
    };
    named.unwrap_or_else(|| {
        let b = body.unwrap_or(kind).lines().next().unwrap_or(kind);
        let first = b.split(". ").next().unwrap_or(b).trim_end_matches('.');
        if first.chars().count() > 60 { format!("{}…", first.chars().take(59).collect::<String>()) } else { first.to_string() }
    })
}

// ---- git ----

/// Git's output, up to `max` bytes (`None` past it), for exits in `ok`.
async fn run(dir: &Path, args: &[&str], ok: &[i32], max: usize) -> Result<Option<Vec<u8>>> {
    run_with(dir, args, ok, max, &[]).await
}

/// `run`, with the content filters named in `filters` switched off.
async fn run_with(dir: &Path, args: &[&str], ok: &[i32], max: usize, filters: &[String]) -> Result<Option<Vec<u8>>> {
    let mut cmd = tokio::process::Command::new("git");
    // As GIT_CONFIG_*, not `-c`: a filter's name may hold an `=`.
    let off: Vec<(String, &str)> = filters.iter().flat_map(|n| ["clean", "smudge", "process"].map(|k| (format!("filter.{n}.{k}"), ""))).collect();
    cmd.env("GIT_CONFIG_COUNT", off.len().to_string());
    for (i, (k, v)) in off.iter().enumerate() {
        cmd.env(format!("GIT_CONFIG_KEY_{i}"), k).env(format!("GIT_CONFIG_VALUE_{i}"), v);
    }
    let mut child = cmd
        .arg("-C")
        .arg(dir)
        .arg("--no-lazy-fetch")
        .arg("--literal-pathspecs")
        .args(crate::review::rounds::SAFE_GIT)
        .args(DIFF_CONFIG)
        .args(args)
        .env("PATH", crate::review::github::command_path())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_PAGER", "cat")
        .env_remove("GIT_EXTERNAL_DIFF")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CDPATH")
        .current_dir(crate::paths::home())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("running git")?;
    let mut out = child.stdout.take().ok_or_else(|| anyhow!("git's output"))?;
    let mut err = child.stderr.take().ok_or_else(|| anyhow!("git's errors"))?;
    let work = async {
        // Both at once, so git never waits on a full pipe we aren't reading.
        let read_out = async {
            let mut b = Vec::new();
            (&mut out).take(max as u64 + 1).read_to_end(&mut b).await?;
            if b.len() > max {
                // Over the cap: stop git, which closes its errors too.
                let _ = child.start_kill();
            }
            Ok::<_, std::io::Error>(b)
        };
        let (buf, e) = tokio::join!(read_out, drain(&mut err, 64 << 10));
        let (buf, e) = (buf?, e?);
        let status = child.wait().await?;
        if buf.len() > max {
            return Ok::<_, anyhow::Error>((None, 0, e));
        }
        Ok((Some(buf), status.code().unwrap_or(-1), e))
    };
    let (buf, code, e) = tokio::time::timeout(GIT_TIMEOUT, work).await.map_err(|_| anyhow!("git took over {}s", GIT_TIMEOUT.as_secs()))??;
    let Some(buf) = buf else { return Ok(None) };
    if !ok.contains(&code) {
        let verb = args.iter().find(|a| !a.starts_with('-')).unwrap_or(&"");
        bail!("git {verb}: {}", String::from_utf8_lossy(&e).trim());
    }
    Ok(Some(buf))
}

/// Every content filter the repo's config names. Reading the working tree
/// runs a file's `clean` filter (its `filter=` attribute), and an agent can
/// write the repo's config and attributes: so the diff switches each one off
/// (`run_with`). A filter added between this read and the diff isn't seen;
/// the wall stops mistakes, not a hostile agent (docs/security.md).
async fn filters(dir: &Path) -> Result<Vec<String>> {
    let out = run(dir, &["config", "--null", "--get-regexp", r"^filter\."], &[0, 1], 1 << 20).await?.ok_or_else(|| anyhow!("the repo's config is too big to read"))?;
    let mut names = std::collections::BTreeSet::new();
    for rec in String::from_utf8_lossy(&out).split('\0') {
        let key = rec.split('\n').next().unwrap_or("");
        if let Some((name, _)) = key.strip_prefix("filter.").and_then(|r| r.rsplit_once('.')) {
            names.insert(name.to_string());
        }
    }
    Ok(names.into_iter().collect())
}

/// Read to the end, keeping the first `keep` bytes.
async fn drain(r: &mut (impl tokio::io::AsyncRead + Unpin), keep: usize) -> std::io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = r.read(&mut chunk).await?;
        if n == 0 {
            return Ok(kept);
        }
        let room = keep.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..n.min(room)]);
    }
}

async fn text(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run(dir, args, &[0], 1 << 20).await?.ok_or_else(|| anyhow!("git said too much"))?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// A commit id this checkout has, in full, or none.
async fn commit(dir: &Path, rev: &str) -> Option<String> {
    let full = text(dir, &["rev-parse", "--verify", "--quiet", "--end-of-options", &format!("{rev}^{{commit}}")]).await.ok()?;
    is_hex(&full, 40).then_some(full)
}

/// A repo whose config moves its work tree (`core.worktree`) elsewhere: its
/// "working tree" could be any folder, so it's not read.
async fn moved_worktree(dir: &Path) -> bool {
    !matches!(run(dir, &["config", "--get", "core.worktree"], &[1], 1 << 16).await, Ok(Some(_)))
}

/// The checkout's top, when it's a git checkout at all.
async fn top(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    let t = text(dir, &["rev-parse", "--show-toplevel"]).await.ok()?;
    (!t.is_empty()).then(|| PathBuf::from(t))
}

async fn base_of(dir: &Path, base: &Base, head: &str) -> Option<String> {
    match base {
        Base::Commit(c) => commit(dir, c).await,
        Base::MergeBase(refs) => {
            for r in refs {
                if commit(dir, r).await.is_some()
                    && let Ok(mb) = text(dir, &["merge-base", "--end-of-options", r, head]).await
                    && is_hex(&mb, 40)
                {
                    return Some(mb);
                }
            }
            None
        }
    }
}

/// One changed file, as `git diff --numstat -z` and the patch say.
#[derive(Clone, Debug, PartialEq)]
pub struct FileStat {
    pub path: String,
    pub old_path: Option<String>,
    pub added: u64,
    pub removed: u64,
    pub binary: bool,
    pub untracked: bool,
}

impl FileStat {
    fn json(&self) -> Value {
        json!({ "path": self.path, "old_path": self.old_path, "added": self.added, "removed": self.removed, "binary": self.binary, "untracked": self.untracked })
    }
}

/// `git diff --numstat -z`: `added\tremoved\tpath\0`, or for a rename
/// `added\tremoved\t\0old\0new\0`; a binary file's counts are `-`.
pub fn parse_numstat(out: &[u8]) -> Vec<FileStat> {
    let s = String::from_utf8_lossy(out);
    let mut parts = s.split('\0');
    let mut files = Vec::new();
    while let Some(head) = parts.next() {
        let mut f = head.splitn(3, '\t');
        let (Some(a), Some(r), Some(path)) = (f.next(), f.next(), f.next()) else { continue };
        let (old_path, path) = if path.is_empty() {
            let (Some(o), Some(n)) = (parts.next(), parts.next()) else { break };
            (Some(o.to_string()), n.to_string())
        } else {
            (None, path.to_string())
        };
        let binary = a == "-" || r == "-";
        files.push(FileStat { path, old_path, added: a.parse().unwrap_or(0), removed: r.parse().unwrap_or(0), binary, untracked: false });
    }
    files
}

/// A new file's lines, read here rather than by git: and whether it's to
/// be shown as binary (a NUL in it, over the size to show, or unreadable).
/// A link counts as its one line (its target), never followed.
fn count_new(path: &Path) -> (u64, bool) {
    let Ok(m) = std::fs::symlink_metadata(path) else { return (0, true) };
    if m.file_type().is_symlink() {
        return (1, false);
    }
    if !m.is_file() || m.len() > UNTRACKED_FILE_MAX {
        return (0, true);
    }
    let Ok(bytes) = std::fs::read(path) else { return (0, true) };
    if bytes[..bytes.len().min(8000)].contains(&0) {
        return (0, true);
    }
    let lines = bytes.iter().filter(|&&b| b == b'\n').count() as u64;
    (lines + u64::from(!bytes.is_empty() && !bytes.ends_with(b"\n")), false)
}

// ---- the diff ----

#[derive(Clone, Debug, Default)]
pub struct Ask {
    pub from: Option<String>,
    pub to: Option<String>,
    /// Only this file's patch (and its old name, for a rename).
    pub path: Option<String>,
    pub old_path: Option<String>,
    /// The file list and counts, no patch.
    pub stat: bool,
}

impl Ask {
    /// The request's own mistakes, found before any git runs.
    pub fn check(&self) -> Result<()> {
        if Rev::parse(self.from.as_deref().unwrap_or("base"))? == Rev::Work {
            bail!("the uncommitted changes can only be what you compare to");
        }
        if let Some(t) = &self.to {
            Rev::parse(t)?;
        }
        for p in self.path.iter().chain(self.old_path.iter()) {
            safe_path(p)?;
        }
        Ok(())
    }
}

/// A docket's diff, as `GET /api/dockets/{id}/diff` answers: a few at a
/// time, each in a minute at most.
pub async fn diff(shared: &Shared, docket: &str, ask: &Ask) -> Result<Value> {
    let _turn = AT_ONCE.acquire().await.context("the hub is stopping")?;
    tokio::time::timeout(DIFF_TIMEOUT, diff_now(shared, docket, ask)).await.map_err(|_| anyhow!("reading the changes took over {}s", DIFF_TIMEOUT.as_secs()))?
}

async fn diff_now(shared: &Shared, docket: &str, ask: &Ask) -> Result<Value> {
    let from = Rev::parse(ask.from.as_deref().unwrap_or("base"))?;
    let to_given = ask.to.as_deref().map(Rev::parse).transpose()?;
    if from == Rev::Work {
        bail!("the uncommitted changes can only be what you compare to");
    }
    let path = ask.path.as_deref().map(safe_path).transpose()?;
    let old_path = ask.old_path.as_deref().map(safe_path).transpose()?;
    let (places, points) = locate(shared, docket)?;
    let mut why: Option<String> = None;
    for p in &places {
        let Some(dir) = top(&p.dir).await else { continue };
        if moved_worktree(&dir).await {
            why = Some(format!("{} sets core.worktree, so its files could be anywhere; Wheelhouse won't read them", crate::paths::tilde(&dir.to_string_lossy())));
            continue;
        }
        let mut head = None;
        for h in &p.head {
            head = commit(&dir, h).await;
            if head.is_some() {
                break;
            }
        }
        let Some(head) = head else { continue };
        let base = base_of(&dir, &p.base, &head).await;
        let to = to_given.clone().unwrap_or(if p.live { Rev::Work } else { Rev::Head });
        if to == Rev::Work && !p.live {
            bail!("a Review's worktree, or the repo's main checkout, has no uncommitted changes of the docket's: compare to HEAD");
        }
        let resolve = |r: &Rev| -> Option<String> {
            match r {
                Rev::Base => base.clone(),
                Rev::Head => Some(head.clone()),
                Rev::Work => None,
                Rev::Sha(s) => Some(s.clone()),
            }
        };
        let Some(from_sha) = resolve(&from) else {
            why = Some(format!("{docket} has no base to compare from in {}", crate::paths::tilde(&dir.to_string_lossy())));
            continue;
        };
        let Some(from_sha) = commit(&dir, &from_sha).await else { continue };
        let to_sha = match resolve(&to) {
            Some(s) => match commit(&dir, &s).await {
                Some(c) => Some(c),
                None => continue,
            },
            None => None,
        };
        let body = from_git(&dir, &from_sha, to_sha.as_deref(), path, old_path, ask.stat).await?;
        let mut v = describe(docket, p, &dir, &head, base.as_deref(), &points, (&from, &from_sha), (&to, to_sha.as_deref()));
        merge(&mut v, body);
        return Ok(v);
    }
    // No checkout has the commits: what the record kept.
    from_record(shared, docket, &points, &from, to_given.as_ref()).map_err(|e| match why {
        Some(w) if from_record_empty(&points) => anyhow!("{w}"),
        _ => e,
    })
}

/// The answer's description of where it came from, and what it compares.
#[allow(clippy::too_many_arguments)]
fn describe(docket: &str, p: &Place, dir: &Path, head: &str, base: Option<&str>, points: &[Point], from: (&Rev, &str), to: (&Rev, Option<&str>)) -> Value {
    let mut pts: Vec<Value> = Vec::new();
    if let Some(b) = base {
        pts.push(json!({ "ref": "base", "sha": b, "label": "Base" }));
    }
    for pt in points.iter().filter(|pt| Some(pt.sha.as_str()) != base) {
        pts.push(json!({ "ref": pt.sha, "sha": pt.sha, "label": pt.label, "at": pt.at, "seq": pt.seq }));
    }
    pts.push(json!({ "ref": "head", "sha": head, "label": "HEAD" }));
    if p.live {
        pts.push(json!({ "ref": "work", "sha": null, "label": "Uncommitted" }));
    }
    let side = |r: &Rev, sha: Option<&str>| {
        let label = match r {
            Rev::Base => "Base".to_string(),
            Rev::Head => "HEAD".into(),
            Rev::Work => "Uncommitted".into(),
            Rev::Sha(_) => points.iter().find(|pt| Some(pt.sha.as_str()) == sha).map(|pt| pt.label.clone()).unwrap_or_else(|| sha.unwrap_or("").chars().take(7).collect()),
        };
        let r = match r {
            Rev::Base => "base".to_string(),
            Rev::Head => "head".into(),
            Rev::Work => "work".into(),
            Rev::Sha(s) => s.clone(),
        };
        json!({ "ref": r, "sha": sha, "label": label })
    };
    json!({
        "docket": docket, "source": p.source, "live": p.live, "repo": p.repo,
        "dir": crate::paths::tilde(&dir.to_string_lossy()), "head": head, "base": base,
        "from": side(from.0, Some(from.1)), "to": side(to.0, to.1), "points": pts,
    })
}

fn merge(v: &mut Value, more: Value) {
    if let (Some(a), Value::Object(b)) = (v.as_object_mut(), more) {
        a.extend(b);
    }
}

/// The diff from `from` to `to` (a commit, or the working tree when none).
async fn from_git(dir: &Path, from: &str, to: Option<&str>, path: Option<&str>, old_path: Option<&str>, stat: bool) -> Result<Value> {
    let mut range: Vec<&str> = vec![from];
    if let Some(t) = to {
        range.push(t);
    }
    let mut spec: Vec<&str> = vec!["--"];
    spec.extend(old_path.iter().chain(path.iter()).copied());
    let numstat_args: Vec<&str> = ["diff", "--numstat", "-z"].iter().copied().chain(DIFF_FLAGS).chain(range.iter().copied()).chain(spec.iter().copied()).collect();
    let off = filters(dir).await?;
    let numstat = run_with(dir, &numstat_args, &[0], 8 << 20, &off).await?.ok_or_else(|| anyhow!("too many files changed to list"))?;
    let mut files = parse_numstat(&numstat);
    // The working tree: new files git doesn't track yet are changes too.
    let mut untracked: Vec<String> = Vec::new();
    let mut untracked_more = 0;
    if to.is_none() {
        let mut args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
        args.extend(path.iter().copied());
        let out = run(dir, &args, &[0], 4 << 20).await?.unwrap_or_default();
        let all: Vec<String> = String::from_utf8_lossy(&out).split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect();
        untracked_more = all.len().saturating_sub(UNTRACKED_MAX);
        untracked = all.into_iter().take(UNTRACKED_MAX).collect();
    }
    let mut patch = String::new();
    let mut too_big = false;
    if !stat {
        let args: Vec<&str> = ["diff", "--patch"].iter().copied().chain(DIFF_FLAGS).chain(range.iter().copied()).chain(spec.iter().copied()).collect();
        match run_with(dir, &args, &[0], PATCH_MAX, &off).await? {
            Some(out) => patch = String::from_utf8_lossy(&out).into_owned(),
            None => too_big = true,
        }
    }
    let started = std::time::Instant::now();
    for (i, u) in untracked.iter().enumerate() {
        if started.elapsed() > UNTRACKED_BUDGET {
            untracked_more += untracked.len() - i;
            break;
        }
        let (added, binary) = count_new(&dir.join(u));
        files.push(FileStat { path: u.clone(), old_path: None, added, removed: 0, binary, untracked: true });
        if stat || too_big || binary {
            continue;
        }
        let args: Vec<&str> = ["diff", "--no-index", "--patch"].iter().copied().chain(DIFF_FLAGS).chain(["--", "/dev/null", u.as_str()]).collect();
        if let Ok(Some(o)) = run_with(dir, &args, &[0, 1], UNTRACKED_FILE_MAX as usize * 2, &off).await {
            if patch.len() + o.len() > PATCH_MAX {
                too_big = true;
            } else {
                patch.push_str(&String::from_utf8_lossy(&o));
            }
        }
    }
    let added: u64 = files.iter().map(|f| f.added).sum();
    let removed: u64 = files.iter().map(|f| f.removed).sum();
    Ok(json!({
        "files": files.iter().map(FileStat::json).collect::<Vec<_>>(),
        "added": added, "removed": removed,
        "patch": if stat || too_big { Value::Null } else { json!(patch) },
        "too_big": too_big, "untracked_more": untracked_more,
    }))
}

fn from_record_empty(points: &[Point]) -> bool {
    !points.iter().any(|p| p.patch.is_some())
}

/// No checkout has the commits any more: a check's kept patch, when the
/// request is for one (base to the commit it ran at).
fn from_record(shared: &Shared, docket: &str, points: &[Point], from: &Rev, to: Option<&Rev>) -> Result<Value> {
    let kept: Vec<&Point> = points.iter().filter(|p| p.patch.is_some()).collect();
    let Some(last) = kept.last() else { bail!("{docket}'s code isn't on this Mac any more, and the record kept no patch for it") };
    let pick = match to {
        None | Some(Rev::Head) | Some(Rev::Work) => *last,
        Some(Rev::Sha(s)) => *kept.iter().find(|p| p.sha.starts_with(s.as_str())).ok_or_else(|| anyhow!("the record kept no patch at {s}"))?,
        Some(Rev::Base) => bail!("compare to a commit, not the base"),
    };
    if *from != Rev::Base {
        bail!("{docket}'s checkout is gone: only its patches from the base, which the record kept, can be shown");
    }
    let (base, digest) = pick.patch.clone().unwrap_or_default();
    let hex = digest.strip_prefix("sha256:").unwrap_or(&digest);
    let bytes = shared.lock().store.blob(hex)?.ok_or_else(|| anyhow!("the record's patch at {} is missing", &pick.sha[..7]))?;
    let patch = String::from_utf8_lossy(&bytes).into_owned();
    let files = stats_of_patch(&patch);
    let added: u64 = files.iter().map(|f| f.added).sum();
    let removed: u64 = files.iter().map(|f| f.removed).sum();
    let mut pts = vec![json!({ "ref": "base", "sha": base, "label": "Base" })];
    pts.extend(kept.iter().map(|p| json!({ "ref": p.sha, "sha": p.sha, "label": p.label, "at": p.at, "seq": p.seq })));
    let too_big = patch.len() > PATCH_MAX;
    Ok(json!({
        "docket": docket, "source": "record", "live": false, "repo": null, "dir": null, "head": pick.sha, "base": base,
        "from": { "ref": "base", "sha": base, "label": "Base" },
        "to": { "ref": pick.sha, "sha": pick.sha, "label": pick.label },
        "points": pts,
        "files": files.iter().map(FileStat::json).collect::<Vec<_>>(),
        "added": added, "removed": removed,
        "patch": if too_big { Value::Null } else { json!(patch) }, "too_big": too_big, "untracked_more": 0,
    }))
}

/// Each file of a git patch and its counts, from the patch alone.
pub fn stats_of_patch(patch: &str) -> Vec<FileStat> {
    let mut files: Vec<FileStat> = Vec::new();
    let mut in_hunk = false;
    for l in patch.lines() {
        if let Some(rest) = l.strip_prefix("diff --git ") {
            in_hunk = false;
            // `a/x b/x`: when both names are the same, the halves are equal.
            let name = rest.strip_prefix("a/").and_then(|r| {
                let half = (r.len().saturating_sub(2)) / 2;
                (r.get(half..half + 3) == Some(" b/") && r[..half] == r[half + 3..]).then(|| r[..half].to_string())
            });
            files.push(FileStat { path: name.unwrap_or_else(|| rest.to_string()), old_path: None, added: 0, removed: 0, binary: false, untracked: false });
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if l.starts_with("@@") {
            in_hunk = true;
        } else if !in_hunk {
            if let Some(p) = l.strip_prefix("+++ b/") {
                f.path = p.to_string();
            } else if let Some(p) = l.strip_prefix("rename from ") {
                f.old_path = Some(p.to_string());
            } else if let Some(p) = l.strip_prefix("rename to ") {
                f.path = p.to_string();
            } else if l.starts_with("Binary files ") {
                f.binary = true;
            }
        } else if l.starts_with('+') {
            f.added += 1;
        } else if l.starts_with('-') {
            f.removed += 1;
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revs_are_words_or_hex_never_flags() {
        assert_eq!(Rev::parse("base").unwrap(), Rev::Base);
        assert_eq!(Rev::parse("work").unwrap(), Rev::Work);
        assert_eq!(Rev::parse("ABCDEF1").unwrap(), Rev::Sha("abcdef1".into()));
        for bad in ["--output=/tmp/x", "-p", "main", "abc", "HEAD~1", "abcdef1..abcdef2", "", "a".repeat(65).as_str()] {
            assert!(Rev::parse(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn paths_stay_inside_the_repo() {
        assert!(safe_path("src/main.rs").is_ok());
        assert!(safe_path("web/src/a b.tsx").is_ok());
        for bad in ["/etc/passwd", "../x", "a/../../x", "-p", "--output=x", "a//b", "./a", "", "a\\b", "a\0b"] {
            assert!(safe_path(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn numstat_with_renames_and_binaries() {
        let out = b"3\t1\tsrc/a.rs\x00-\t-\timg.png\x002\t0\t\x00old/name.rs\x00new/name.rs\x00";
        let f = parse_numstat(out);
        assert_eq!(f.len(), 3);
        assert_eq!((f[0].path.as_str(), f[0].added, f[0].removed, f[0].binary), ("src/a.rs", 3, 1, false));
        assert!(f[1].binary);
        assert_eq!((f[2].old_path.as_deref(), f[2].path.as_str()), (Some("old/name.rs"), "new/name.rs"));
    }

    #[test]
    fn stats_from_a_patch_alone() {
        let p = "diff --git a/x.rs b/x.rs\nindex 1..2 100644\n--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n\
                 diff --git a/o.rs b/n.rs\nsimilarity index 90%\nrename from o.rs\nrename to n.rs\n@@ -1 +1,2 @@\n x\n+y\n\
                 diff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n";
        let f = stats_of_patch(p);
        assert_eq!(f.len(), 3);
        assert_eq!((f[0].path.as_str(), f[0].added, f[0].removed), ("x.rs", 1, 1));
        assert_eq!((f[1].old_path.as_deref(), f[1].path.as_str(), f[1].added), (Some("o.rs"), "n.rs", 1));
        assert!(f[2].binary);
    }

    #[test]
    fn points_are_named_by_what_happened() {
        assert_eq!(label_of("check", Some("test: `x` · passed"), &json!({})), "Checks ran");
        assert_eq!(label_of("observed", None, &json!({ "what": "review-event", "event": "started", "round": 2 })), "Review round 2");
        assert_eq!(label_of("approval", None, &json!({ "what": "open-pr" })), "PR opened");
        assert_eq!(label_of("observed", Some("Check 1 of wh/x at abc1234. More."), &json!({})), "Check 1 of wh/x at abc1234");
    }

    #[test]
    fn a_bad_request_is_refused_before_any_git() {
        let ask = |from: Option<&str>, to: Option<&str>, path: Option<&str>| Ask {
            from: from.map(String::from),
            to: to.map(String::from),
            path: path.map(String::from),
            ..Default::default()
        };
        assert!(ask(None, None, None).check().is_ok());
        assert!(ask(Some("abc1234"), Some("work"), Some("src/a.rs")).check().is_ok());
        assert!(ask(Some("work"), None, None).check().is_err());
        assert!(ask(None, Some("--cached"), None).check().is_err());
        assert!(ask(None, None, Some("../x")).check().is_err());
    }

    #[tokio::test]
    async fn a_missing_object_is_never_fetched() {
        // A repo that says it's a partial clone, with a promisor remote whose
        // upload-pack is a command: asking for a missing blob would run it.
        let tmp = std::env::temp_dir().join(format!("wh-lazy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let mark = tmp.join("ran");
        let g = "git -c user.name=t -c user.email=t@t -c commit.gpgsign=false";
        sh(&tmp, &format!("git init -q -b main . && echo one > f && {g} add f && {g} commit -qm a")).await;
        let base = text(&tmp, &["rev-parse", "HEAD"]).await.unwrap();
        sh(&tmp, &format!("echo two > f && {g} commit -qam b")).await;
        let blob = text(&tmp, &["rev-parse", "HEAD:f"]).await.unwrap();
        std::fs::remove_file(tmp.join(".git/objects").join(&blob[..2]).join(&blob[2..])).unwrap();
        sh(
            &tmp,
            &format!(
                "git config core.repositoryformatversion 1 && git config extensions.partialClone origin && git config remote.origin.promisor true \
             && git config remote.origin.url {0} && git config remote.origin.uploadpack \"touch {1}; false\"",
                tmp.display(),
                mark.display()
            ),
        )
        .await;
        let top = top(&tmp).await.unwrap();
        let _ = from_git(&top, &base, Some("HEAD"), None, None, false).await;
        assert!(!mark.exists(), "a missing object was fetched, and the repo's command ran");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_new_file_is_counted_without_git() {
        let d = std::env::temp_dir().join(format!("wh-count-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a"), "x\ny\nz").unwrap();
        std::fs::write(d.join("b"), b"\x00\x01").unwrap();
        std::fs::write(d.join("e"), "").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", d.join("l")).unwrap();
        assert_eq!(count_new(&d.join("a")), (3, false));
        assert_eq!(count_new(&d.join("b")), (0, true));
        assert_eq!(count_new(&d.join("e")), (0, false));
        assert_eq!(count_new(&d.join("l")), (1, false), "a link is its target's name, never followed");
        assert_eq!(count_new(&d.join("missing")), (0, true));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn output_past_the_cap_is_none_not_a_partial_patch() {
        let dir = std::env::temp_dir();
        // `git version` says a few dozen bytes; a cap of 4 is under it.
        assert!(run(&dir, &["version"], &[0], 4).await.unwrap().is_none());
        assert!(run(&dir, &["version"], &[0], 4096).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn with_no_checkout_the_record_s_kept_patch_is_shown() {
        let shared = Shared::new(crate::hub::test_core());
        let id = shared.lock().open(&crate::hub::Who::Owner, "Gone", json!({})).unwrap();
        let patch = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -1 +1,2 @@\n a\n+b\n";
        let digest = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(patch.as_bytes()));
        let (head, base) = ("a".repeat(40), "b".repeat(40));
        {
            let mut core = shared.lock();
            core.store.put_blob(&digest, patch.as_bytes(), "2026-11-25T00:00:00Z").unwrap();
            let data = json!({ "name": "test", "head": head, "base": base, "patch": format!("sha256:{digest}"), "ok": true });
            core.append_with(&id, "check", &crate::hub::Who::Hub, Some("test: passed".into()), data, vec![format!("sha256:{digest}")]).unwrap();
        }
        let v = diff(&shared, &id, &Ask::default()).await.unwrap();
        assert_eq!(v["source"], "record");
        assert_eq!(v["patch"], patch);
        assert_eq!((v["added"].as_u64(), v["files"][0]["path"].as_str()), (Some(1), Some("x.rs")));
        assert_eq!(v["to"]["label"], "Checks ran");
        // Only the base to a kept commit can be shown from the record.
        let ask = Ask { from: Some(head.clone()), ..Default::default() };
        assert!(diff(&shared, &id, &ask).await.is_err());
    }

    async fn sh(dir: &Path, cmd: &str) {
        let ok = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .await
            .unwrap()
            .success();
        assert!(ok, "{cmd}");
    }

    #[tokio::test]
    async fn a_worktree_diff_has_commits_edits_and_new_files() {
        let tmp = std::env::temp_dir().join(format!("wh-diffs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let g = "git -c user.name=t -c user.email=t@t -c commit.gpgsign=false";
        sh(&tmp, &format!("git init -q -b main . && printf 'a\\nb\\n' > a.txt && {g} add . && {g} commit -qm one")).await;
        let base = text(&tmp, &["rev-parse", "HEAD"]).await.unwrap();
        sh(&tmp, &format!("printf 'a\\nB\\n' > a.txt && {g} commit -qam two && printf 'a\\nB\\nc\\n' > a.txt && printf 'new\\n' > n.txt")).await;
        let head = text(&tmp, &["rev-parse", "HEAD"]).await.unwrap();
        let top = top(&tmp).await.unwrap();

        let v = from_git(&top, &base, None, None, None, false).await.unwrap();
        let files: Vec<&str> = v["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert_eq!(files, ["a.txt", "n.txt"]);
        assert_eq!((v["added"].as_u64(), v["removed"].as_u64()), (Some(3), Some(1)));
        let patch = v["patch"].as_str().unwrap();
        assert!(patch.contains("+++ b/a.txt") && patch.contains("+++ b/n.txt") && patch.contains("+c"));

        let v = from_git(&top, &base, Some(&head), None, None, false).await.unwrap();
        assert_eq!(v["files"].as_array().unwrap().len(), 1, "commits only: no working tree, no untracked");

        let v = from_git(&top, &base, None, Some("n.txt"), None, false).await.unwrap();
        assert_eq!(v["files"].as_array().unwrap().len(), 1);
        assert!(v["patch"].as_str().unwrap().contains("+new"));

        let v = from_git(&top, &base, None, None, None, true).await.unwrap();
        assert!(v["patch"].is_null() && v["files"].as_array().unwrap().len() == 2, "stat: counts, no patch");

        // A content filter the repo's config plants never runs: not for a
        // changed file, not for a new one.
        let mark = tmp.join("ran");
        sh(
            &tmp,
            &format!(
                "echo '* filter=x.y' > .git/info/attributes && git config filter.x.y.clean \"touch {} ; cat\" && git config filter.x.y.process \"touch {}\"",
                mark.display(),
                mark.display()
            ),
        )
        .await;
        sh(&tmp, "printf 'z\\n' >> a.txt").await;
        let v = from_git(&top, &base, None, None, None, false).await.unwrap();
        assert!(v["patch"].as_str().unwrap().contains("+z"));
        assert!(!mark.exists(), "the repo's filter ran");

        // A repo that moves its work tree elsewhere isn't read.
        assert!(!moved_worktree(&top).await);
        sh(&tmp, "git config core.worktree /").await;
        assert!(moved_worktree(&top).await);
        sh(&tmp, "git config --unset core.worktree").await;

        // Never a lock in the agent's checkout, even when its index is stale.
        assert!(!tmp.join(".git/index.lock").exists());
        assert!(commit(&top, "--output=/tmp/x").await.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
