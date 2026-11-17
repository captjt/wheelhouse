//! Reading rex-reviewer's `config.lua` as it is, with LuaJIT: its settings
//! as JSON, and its functions (the effort policy) called per PR.

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
    let bin = luajit().ok_or_else(|| anyhow!("LuaJIT isn't installed (brew install luajit); it reads your rex-reviewer config"))?;
    let mut child = Command::new(bin).arg("-").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    child.stdin.take().context("luajit's stdin")?.write_all(DRIVER.as_bytes())?;
    // A config is a few lines of Lua; one that runs on is a mistake in it.
    let started = std::time::Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            bail!("your config took over 10s to run");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = child.wait_with_output()?;
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
}
