//! Running the hub as a LaunchAgent: `wh up`, `down`, `logs` (from rex-inbox).

use anyhow::{Context, Result, bail};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::paths::home;

pub const LABEL: &str = "io.github.captjt.wheelhouse";

fn plist() -> PathBuf {
    home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

pub fn log() -> PathBuf {
    home().join("Library/Logs/wheelhouse.log")
}

fn domain() -> Result<String> {
    Ok(format!("gui/{}", std::fs::metadata(home())?.uid()))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn hub_answers() -> bool {
    crate::client::call("GET", "/api/status", None, Duration::from_secs(2)).is_ok_and(|(s, _)| s == 200)
}

fn launchd_has_it() -> Result<bool> {
    Ok(Command::new("launchctl").args(["print", &format!("{}/{LABEL}", domain()?)]).output()?.status.success())
}

pub fn up() -> Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        eprintln!("wh: note: running from a build folder ({}); `cargo install --path .` gives a stable path", exe.display());
    }
    if hub_answers() && !launchd_has_it()? {
        // A hub started by hand (wh serve in a pane): hand over to launchd.
        let _ = crate::client::call("POST", "/api/hub/stop", Some(&serde_json::json!({})), Duration::from_secs(3));
        for _ in 0..30 {
            if !hub_answers() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if hub_answers() {
            bail!("another hub is running and didn't stop; stop it, then run wh up");
        }
        println!("stopped the hub that was running by hand");
    }
    // A LaunchAgent's PATH is short: add rex, Homebrew, cargo, Tailscale.
    let rex_dir = crate::rex::rex_bin().parent().map(|p| p.display().to_string()).unwrap_or_default();
    let cargo = home().join(".cargo/bin").display().to_string();
    let path = [rex_dir.as_str(), "/opt/homebrew/bin", "/usr/local/bin", &cargo, "/usr/bin", "/bin", "/usr/sbin", "/sbin"].join(":");
    let mut env = format!("    <key>PATH</key><string>{}</string>\n", xml(&path));
    for var in ["WH_STATE", "WH_SOCKET", "WH_HTTP", "WH_TAILNET", "WH_REX", "WH_OWNER", "USER", "HOME"] {
        if let Ok(v) = std::env::var(var) {
            env.push_str(&format!("    <key>{var}</key><string>{}</string>\n", xml(&v)));
        }
    }
    let log = log();
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>serve</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
{env}  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        exe = xml(&exe.display().to_string()),
        log = xml(&log.display().to_string()),
    );
    for dir in [plist().parent().map(PathBuf::from), log.parent().map(PathBuf::from)].into_iter().flatten() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(plist(), body).with_context(|| format!("writing {}", plist().display()))?;
    let domain = domain()?;
    // Replace a running one, so `up` also picks up a new binary.
    let _ = Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]).output();
    std::thread::sleep(Duration::from_millis(500));
    let out = Command::new("launchctl").args(["bootstrap", &domain, &plist().display().to_string()]).output()?;
    if !out.status.success() {
        bail!("launchctl bootstrap: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    for _ in 0..80 {
        if hub_answers() {
            println!("Wheelhouse is up ({LABEL}); logs: {}", crate::paths::tilde(&log.display().to_string()));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("the hub didn't answer within 8s; see {}", log.display())
}

pub fn down() -> Result<()> {
    let out = Command::new("launchctl").args(["bootout", &format!("{}/{LABEL}", domain()?)]).output()?;
    if !out.status.success() && !hub_answers() {
        println!("Wheelhouse wasn't running");
        return Ok(());
    }
    println!("Wheelhouse stopped");
    Ok(())
}

pub fn logs(lines: usize) -> Result<()> {
    let text = std::fs::read_to_string(log()).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    for l in &all[all.len().saturating_sub(lines)..] {
        println!("{l}");
    }
    Ok(())
}
