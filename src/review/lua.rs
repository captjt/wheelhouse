//! Reading your config (`review.lua`, `build.lua`) with LuaJIT: its
//! settings as JSON, and its functions (the effort policy) called per PR.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const DRIVER: &str = include_str!("config.lua");

fn luajit() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("WH_LUAJIT") {
        return Some(p.into());
    }
    ["/opt/homebrew/bin/luajit", "/usr/local/bin/luajit"].iter().map(std::path::PathBuf::from).find(|p| p.exists()).or_else(|| crate::paths::which("luajit"))
}

fn run(args: &[&str]) -> Result<Value> {
    let bin = luajit().ok_or_else(|| anyhow!("LuaJIT isn't installed (brew install luajit); it reads your review.lua and build.lua"))?;
    let mut child = Command::new(bin).arg("-").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    child.stdin.take().context("luajit's stdin")?.write_all(DRIVER.as_bytes())?;
    // Its output is read while it runs, on a thread: a big config (long
    // notes or guides inline) would otherwise fill the pipe and stall it.
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    // A config is a few lines of Lua; one that runs on is a mistake in it.
    let out = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(out) => out?,
        Err(_) => {
            let _ = Command::new("kill").arg(pid.to_string()).status();
            bail!("your config took over 10s to run");
        }
    };
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    serde_json::from_slice(&out.stdout).context("reading what your config gave")
}

/// The whole config, with each function as `{"$fn": "its.path"}`.
pub fn dump(path: &Path) -> Result<Value> {
    run(&["dump", &path.to_string_lossy()])
}

/// The function at `key` (`effort.review`) called with `pr`; a plain value
/// there comes back as it is.
pub fn call(path: &Path, key: &str, pr: &Value) -> Result<Value> {
    run(&["call", &path.to_string_lossy(), key, &literal(pr)])
}

/// A JSON value as a Lua table constructor.
pub fn literal(v: &Value) -> String {
    match v {
        Value::Null => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => string(s),
        Value::Array(a) => format!("{{{}}}", a.iter().map(literal).collect::<Vec<_>>().join(",")),
        Value::Object(o) => format!("{{{}}}", o.iter().map(|(k, v)| format!("[{}]={}", string(k), literal(v))).collect::<Vec<_>>().join(",")),
    }
}

fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03}")),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_config_is_read_and_its_policy_called() {
        if luajit().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("wh-lua-{}", std::process::id()));
        let _gone = crate::hub::TestDir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.lua");
        std::fs::write(
            &path,
            r#"return { post = "ask", reviewers = { { name = "claude", agent = "claude" } },
                effort = { review = function(pr)
                  if pr.touches({ "/migrations/" }) then return "xhigh" end
                  return pr.lines < 150 and "medium" or "high" end } }"#,
        )
        .unwrap();
        let c = dump(&path).unwrap();
        assert_eq!(c["post"], "ask");
        assert_eq!(c["effort"]["review"], json!({ "$fn": "effort.review" }));
        let pr = json!({ "lines": 40, "files": ["a/b.rs", "q\"uote\n\u{e9}"] });
        assert_eq!(call(&path, "effort.review", &pr).unwrap(), "medium");
        let pr = json!({ "lines": 40, "files": ["db/migrations/1.sql"] });
        assert_eq!(call(&path, "effort.review", &pr).unwrap(), "xhigh");
        assert_eq!(call(&path, "post", &json!({})).unwrap(), "ask");
        std::fs::write(&path, "return 1 +").unwrap();
        assert!(dump(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn posting_reads_as_a_rule_or_a_function() {
        use super::super::posting::Post;
        if luajit().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("wh-lua-post-{}", std::process::id()));
        let _gone = crate::hub::TestDir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("review.lua");
        std::fs::write(
            &path,
            r#"return {
                repos = {
                  ["acme/api"] = { checkout = "~/x", post = { auto = { verdict = "approve", merge_risk = "low", not_touching = { "db/" } } } },
                  ["acme/web"] = { checkout = "~/y" },
                },
                post = function(pr)
                  if pr.verdict == "approve" and pr.blocking == 0 and not pr.touches({ "auth/" }) then return "auto" end
                  return "ask"
                end,
              }"#,
        )
        .unwrap();
        let c = super::super::config::from_value(path.clone(), dump(&path).unwrap()).unwrap();
        assert_eq!(c.post, Post::Lua("post".into()));
        assert_eq!(c.post_for("acme/api").kind(), "rule");
        let ok = json!({ "verdict": "approve", "blocking": 0, "merge_risk": "low", "files": ["src/a.rs"] });
        assert!(c.post_for("acme/api").decide(&ok, &path).auto);
        let d = c.post_for("acme/web").decide(&ok, &path);
        assert_eq!((d.auto, d.why.as_str()), (true, "`post` in review.lua returned auto"));
        let auth = json!({ "verdict": "approve", "blocking": 0, "files": ["auth/login.rs"] });
        assert!(!c.post_for("acme/web").decide(&auth, &path).auto);
        std::fs::write(&path, r#"return { repos = { ["o/a"] = { checkout = "~/x" } }, post = function(pr) error("boom") end }"#).unwrap();
        let c = super::super::config::from_value(path.clone(), dump(&path).unwrap()).unwrap();
        let d = c.post.decide(&ok, &path);
        assert!(!d.auto && d.why.starts_with("`post` in review.lua failed:") && d.why.contains("boom"), "{}", d.why);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
