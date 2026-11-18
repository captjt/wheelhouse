//! The app on your tailnet. `tailscale serve` publishes the hub's tailnet
//! listener (127.0.0.1:7501) at https://<mac>.<tailnet>.ts.net:7500 and says
//! who is calling (`Tailscale-User-Login`). That listener answers only the
//! Mac's own Tailscale user; the loopback listener (7500) answers this Mac.

use anyhow::{Result, bail};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

use crate::hub::Shared;

/// The HTTPS port the app gets on the tailnet.
pub const TAILNET_PORT: u16 = 7500;

pub fn listen_addr() -> Option<std::net::SocketAddr> {
    let v = std::env::var("WH_TAILNET").unwrap_or_else(|_| "127.0.0.1:7501".into());
    if v == "off" { None } else { v.parse().ok() }
}

pub fn tailscale_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WH_TAILSCALE") {
        return Some(p.into());
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join("tailscale");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let app = PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
    app.is_file().then_some(app)
}

fn run_json(args: &[&str]) -> Option<Value> {
    let out = std::process::Command::new(tailscale_bin()?).args(args).output().ok()?;
    out.status.success().then(|| serde_json::from_slice(&out.stdout).ok()).flatten()
}

/// This Mac's Tailscale user (`WH_OWNER` overrides) and its name on the tailnet.
pub fn owner_and_host(status: Option<&Value>) -> (Option<String>, Option<String>) {
    let owner = std::env::var("WH_OWNER").ok().or_else(|| {
        let s = status?;
        let uid = s["Self"]["UserID"].as_i64()?;
        s["User"][uid.to_string()]["LoginName"].as_str().map(str::to_string)
    });
    let host = status.and_then(|s| s["Self"]["DNSName"].as_str()).map(|d| d.trim_end_matches('.').to_string());
    (owner, host)
}

/// Keep the owner current: Tailscale may start after the hub.
pub async fn watch(shared: Shared) {
    loop {
        let status = tokio::task::spawn_blocking(|| run_json(&["status", "--json"])).await.ok().flatten();
        let (owner, host) = owner_and_host(status.as_ref());
        {
            let mut core = shared.lock();
            if core.tailnet_owner != owner
                && let Some(o) = &owner
            {
                say!("tailnet owner: {o}");
            }
            core.tailnet_owner = owner;
            core.tailnet_host = host;
        }
        tokio::time::sleep(Duration::from_secs(120)).await;
    }
}

/// Whether `tailscale serve` proxies the app's port to our listener.
fn served(serve: Option<&Value>, backend: &str) -> Option<bool> {
    let s = serve?;
    Some(s["Web"].as_object().is_some_and(|sites| {
        sites.iter().any(|(site, v)| {
            site.ends_with(&format!(":{TAILNET_PORT}"))
                && v["Handlers"].as_object().is_some_and(|hs| hs.values().any(|h| h["Proxy"].as_str().is_some_and(|p| p.trim_end_matches('/').ends_with(backend))))
        })
    }))
}

/// `wh web`: publish the app on the tailnet if it isn't, and print its address.
pub fn cli(publish: bool) -> Result<()> {
    let Some(_) = tailscale_bin() else { bail!("Tailscale isn't installed. brew install --cask tailscale-app, sign in, then run wh web") };
    let status = run_json(&["status", "--json"]);
    let (owner, host) = owner_and_host(status.as_ref());
    let Some(host) = host else { bail!("Tailscale isn't running or signed in") };
    let https = status.as_ref().map(|s| s["CertDomains"].as_array().is_some_and(|d| !d.is_empty()));
    if https == Some(false) {
        bail!("turn on HTTPS certificates in the Tailscale admin console (DNS page), then run wh web");
    }
    let backend = listen_addr().map(|a| format!(":{}", a.port())).unwrap_or_else(|| ":7501".into());
    let is_served = served(run_json(&["serve", "status", "--json"]).as_ref(), &backend);
    if is_served != Some(true) {
        if !publish {
            println!("Not on your tailnet yet. Run `wh web --publish` to serve it at https://{host}:{TAILNET_PORT}");
            return Ok(());
        }
        let target = format!("http://127.0.0.1{backend}");
        let out = std::process::Command::new(tailscale_bin().unwrap()).args(["serve", "--bg", &format!("--https={TAILNET_PORT}"), &target]).output()?;
        if !out.status.success() {
            bail!("tailscale serve: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
    }
    println!("https://{host}:{TAILNET_PORT}");
    println!("Answers only {} (this Mac's Tailscale user). On a phone: Share › Add to Home Screen.", owner.as_deref().unwrap_or("this Mac's Tailscale user"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_owner_host_and_serve() {
        let status = json!({"Self": {"UserID": 7, "DNSName": "mac.tail1.ts.net."}, "User": {"7": {"LoginName": "you@example.com"}}});
        let (o, h) = owner_and_host(Some(&status));
        assert_eq!(o.as_deref(), Some("you@example.com"));
        assert_eq!(h.as_deref(), Some("mac.tail1.ts.net"));
        let serve = json!({"Web": {"mac.tail1.ts.net:7500": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:7501"}}},
                                   "mac.tail1.ts.net:443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:7430"}}}}});
        assert_eq!(served(Some(&serve), ":7501"), Some(true));
        assert_eq!(served(Some(&serve), ":7600"), Some(false));
        assert_eq!(served(None, ":7501"), None);
    }
}
