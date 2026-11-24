//! Previews: an app an agent serves on this Mac, for you to look at and
//! answer, from the Mac or the phone (after rex-inbox's `share`).
//!
//! Each preview gets its own HTTPS port on the tailnet (`tailscale serve
//! --https=75xx`), so the app is at `/` and its assets and cookies work as
//! on localhost. Unlike rex-inbox's, it goes through the hub: a proxy that
//! answers only the Mac's Tailscale user, on every request, and passes the
//! request on with Host set to localhost (so dev servers needn't allow the
//! tailnet name). Websockets (live reload) aren't passed on yet.
//!
//! The preview rides on a message to you. It comes down when that message
//! is answered or filed, the agent's pane closes, the app stops answering
//! for about 15 seconds, or the agent runs `wh share --stop`.

use anyhow::{Result, anyhow, bail};
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::time::Duration;

use crate::hub::Shared;
use crate::messages::Caller;

/// The tailnet HTTPS ports previews take; the proxy behind each listens on
/// that port plus `BACKEND_OFFSET` on loopback.
const PORTS: std::ops::RangeInclusive<u16> = 7510..=7599;
const BACKEND_OFFSET: u16 = 1000;
const SWEEP: Duration = Duration::from_secs(5);
const MISSES: u32 = 3;

pub struct Live {
    pub message: i64,
    pub app_port: u16,
    pub block: Option<String>,
    pub misses: u32,
    task: tokio::task::JoinHandle<()>,
}

async fn answers(port: u16) -> bool {
    tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(("127.0.0.1", port))).await.is_ok_and(|r| r.is_ok())
}

async fn tailscale(args: &[&str]) -> Result<()> {
    let bin = crate::tailnet::tailscale_bin().ok_or_else(|| anyhow!("Tailscale isn't installed on this Mac"))?;
    let out = tokio::time::timeout(std::time::Duration::from_secs(30), tokio::process::Command::new(bin).args(args).kill_on_drop(true).output())
        .await
        .map_err(|_| anyhow::anyhow!("tailscale took over 30s"))??;
    if !out.status.success() {
        bail!("tailscale {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Share an app the caller's pane serves on `app_port`.
pub async fn share(shared: &Shared, from: &Caller, app_port: u16, title: Option<String>, ask: Option<String>, options: Vec<String>) -> Result<Value> {
    if !answers(app_port).await {
        bail!("nothing answers on 127.0.0.1:{app_port}: start the app first");
    }
    let (https_port, host) = {
        let core = shared.lock();
        let host = core.tailnet_host.clone().ok_or_else(|| anyhow!("Tailscale isn't running, so there's nowhere to share it"))?;
        if core.previews.values().any(|p| p.app_port == app_port) {
            bail!("{app_port} is already shared; stop it first with `wh share --stop {app_port}`");
        }
        let taken: Vec<u16> = core.previews.keys().copied().collect();
        (PORTS.clone().find(|p| !taken.contains(p)).ok_or_else(|| anyhow!("every preview port is taken"))?, host)
    };
    let backend = https_port + BACKEND_OFFSET;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", backend)).await.map_err(|e| anyhow!("binding 127.0.0.1:{backend}: {e}"))?;
    let app = axum::Router::new().fallback(proxy).with_state((shared.clone(), app_port));
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    if let Err(e) = tailscale(&["serve", "--bg", &format!("--https={https_port}"), &format!("http://127.0.0.1:{backend}")]).await {
        task.abort();
        return Err(e);
    }
    let url = format!("https://{host}:{https_port}/");
    let local = format!("http://127.0.0.1:{app_port}/");
    let options = if options.is_empty() { vec!["Looks right".to_string(), "Needs work".to_string()] } else { options };
    let title = title.unwrap_or_else(|| format!("Preview of :{app_port}"));
    let body = format!("{}\n\n{url}", ask.as_deref().unwrap_or("Have a look."));
    let mut core = shared.lock();
    let m = crate::messages::send(&mut core, from, crate::messages::ME, Some(title), &body, options)?;
    let preview = json!({ "url": url, "local": local, "https_port": https_port, "app_port": app_port });
    core.store.set_message_preview(m.id, &preview)?;
    core.previews.insert(https_port, Live { message: m.id, app_port, block: from.block.clone(), misses: 0, task });
    core.live_changed();
    Ok(json!({ "message": m.id, "url": url, "local": local }))
}

async fn take_down(shared: &Shared, https_port: u16, why: &str) {
    let live = shared.lock().previews.remove(&https_port);
    let Some(live) = live else { return };
    live.task.abort();
    if let Err(e) = tailscale(&["serve", &format!("--https={https_port}"), "off"]).await {
        say!("preview {https_port}: {e:#}");
    }
    let mut core = shared.lock();
    let _ = core.store.set_message_preview_down(live.message, why);
    core.live_changed();
}

/// `wh share --stop [PORT]`: the caller's previews, or the one on PORT.
pub async fn stop(shared: &Shared, from: &Caller, app_port: Option<u16>) -> Result<usize> {
    let ports: Vec<u16> = {
        let core = shared.lock();
        core.previews.iter().filter(|(_, p)| app_port.is_none_or(|a| p.app_port == a) && (from.block.is_none() || p.block == from.block)).map(|(k, _)| *k).collect()
    };
    for p in &ports {
        take_down(shared, *p, "the agent stopped it").await;
    }
    Ok(ports.len())
}

/// Every five seconds: take down what's answered, orphaned, or gone quiet.
pub async fn sweep(shared: Shared) {
    loop {
        tokio::time::sleep(SWEEP).await;
        let checks: Vec<(u16, i64, u16, Option<String>)> = shared.lock().previews.iter().map(|(k, p)| (*k, p.message, p.app_port, p.block.clone())).collect();
        for (https_port, msg, app_port, block) in checks {
            let why = {
                let core = shared.lock();
                let status = core.store.message(msg).ok().flatten().map(|m| m.status);
                if !matches!(status.as_deref(), Some("open")) {
                    Some("you answered it")
                } else if block.as_ref().is_some_and(|b| core.mirror.connected_for(b) && !core.mirror.panes.contains_key(b)) {
                    Some("the agent's pane closed")
                } else {
                    None
                }
            };
            let why = match why {
                Some(w) => Some(w),
                None if !answers(app_port).await => {
                    let mut core = shared.lock();
                    let quiet = core.previews.get_mut(&https_port).map(|p| {
                        p.misses += 1;
                        p.misses >= MISSES
                    });
                    quiet.unwrap_or(false).then_some("the app stopped answering")
                }
                None => {
                    if let Some(p) = shared.lock().previews.get_mut(&https_port) {
                        p.misses = 0;
                    }
                    None
                }
            };
            if let Some(why) = why {
                take_down(&shared, https_port, why).await;
            }
        }
    }
}

const HOP: [&str; 8] = ["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade"];

fn copy_headers(from: &HeaderMap, skip: &[&str]) -> Vec<(HeaderName, axum::http::HeaderValue)> {
    from.iter().filter(|(k, _)| !HOP.contains(&k.as_str()) && !skip.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// The preview's proxy: only the owner, then on to the app on loopback.
async fn proxy(State((shared, app_port)): State<(Shared, u16)>, req: Request) -> Response {
    let login = req.headers().get("tailscale-user-login").and_then(|v| v.to_str().ok()).map(str::to_string);
    let owner = shared.lock().tailnet_owner.clone();
    match (login, owner) {
        (Some(l), Some(o)) if l.eq_ignore_ascii_case(&o) => {}
        _ => return (StatusCode::FORBIDDEN, "This preview answers only the Mac's Tailscale user.").into_response(),
    }
    if req.headers().get("upgrade").is_some() {
        return (StatusCode::NOT_IMPLEMENTED, "Live reload over websockets isn't passed through previews yet.").into_response();
    }
    let (parts, body) = req.into_parts();
    let path = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let Ok(bytes) = axum::body::to_bytes(body, 64 << 20).await else { return (StatusCode::PAYLOAD_TOO_LARGE, "too large").into_response() };
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(60)).build();
    let Ok(client) = client else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    let mut out = client.request(parts.method.clone(), format!("http://127.0.0.1:{app_port}{path}"));
    for (k, v) in copy_headers(&parts.headers, &["host", "content-length", "tailscale-user-login", "tailscale-user-name", "tailscale-user-profile-pic"]) {
        out = out.header(k, v);
    }
    out = out.header("host", format!("localhost:{app_port}"));
    match out.body(bytes).send().await {
        Ok(r) => {
            let status = r.status();
            let headers = copy_headers(r.headers(), &["content-length"]);
            let body: Bytes = r.bytes().await.unwrap_or_default();
            let mut resp = Response::new(Body::from(body));
            *resp.status_mut() = status;
            for (k, v) in headers {
                resp.headers_mut().append(k, v);
            }
            resp
        }
        Err(e) => (StatusCode::BAD_GATEWAY, format!("the app on :{app_port} didn't answer: {e}")).into_response(),
    }
}
