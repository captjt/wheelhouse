//! Web Push to your phone (from rex-inbox). The app, added to the Home
//! Screen and opened over the tailnet, subscribes with the hub's VAPID key;
//! the hub sends what needs you to the browser's push service, end-to-end
//! encrypted (RFC 8291), so the push service carries it without reading it.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde_json::{Value, json};
use std::time::Duration;
use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair};
use web_push_native::p256::PublicKey;
use web_push_native::{Auth, WebPushBuilder};

use crate::hub::Shared;

const GRACE: Duration = Duration::from_secs(2);

/// The hub's VAPID key pair, made on first use and kept in the state dir.
pub fn vapid() -> Result<ES256KeyPair> {
    let path = crate::paths::state_dir().join("vapid.key");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let raw = B64.decode(text.trim()).context("reading vapid.key")?;
        return ES256KeyPair::from_bytes(&raw).map_err(|e| anyhow!("vapid.key: {e}"));
    }
    let kp = ES256KeyPair::generate();
    std::fs::create_dir_all(crate::paths::state_dir())?;
    std::fs::write(&path, B64.encode(kp.to_bytes()))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(kp)
}

pub fn public_key() -> Result<String> {
    Ok(B64.encode(vapid()?.public_key().public_key().to_bytes_uncompressed()))
}

pub fn subscribe(shared: &Shared, sub: &Value, agent: Option<&str>) -> Result<()> {
    let endpoint = sub["endpoint"].as_str().ok_or_else(|| anyhow!("subscription.endpoint is required"))?;
    let p256dh = sub["keys"]["p256dh"].as_str().ok_or_else(|| anyhow!("subscription.keys.p256dh is required"))?;
    let auth = sub["keys"]["auth"].as_str().ok_or_else(|| anyhow!("subscription.keys.auth is required"))?;
    if !endpoint.starts_with("https://") {
        bail!("push endpoints are https URLs");
    }
    shared.lock().store.push_subscribe(endpoint, p256dh, auth, agent)
}

/// Push what needs you to every subscribed device.
pub async fn run(shared: Shared) {
    let mut rx = shared.lock().events.subscribe();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        };
        let kind = ev["type"].as_str().unwrap_or_default();
        if kind != "needs" && !(kind == "message" && ev["to"] == "me") {
            continue;
        }
        // rex-inbox pushes prompts and failures while it runs.
        if kind == "needs" && crate::notify::rex_inbox_running() {
            continue;
        }
        let shared = shared.clone();
        tokio::spawn(async move {
            tokio::time::sleep(GRACE).await;
            let Some(n) = crate::notify::still_due(&shared, &ev) else { return };
            let body = if n.body.is_empty() { n.subtitle.clone() } else { format!("{}\n{}", n.subtitle, n.body) };
            let note = json!({ "title": n.title, "body": body, "tag": n.group, "url": format!("/{}", n.path.trim_start_matches('/')) });
            if let Err(e) = send_all(&shared, &note).await {
                say!("push: {e:#}");
            }
        });
    }
}

pub async fn send_all(shared: &Shared, note: &Value) -> Result<usize> {
    let (subs, contact) = {
        let core = shared.lock();
        // RFC 8292 wants a contact; the app's own address serves.
        let contact = core.tailnet_host.as_ref().map(|h| format!("https://{h}:{}", crate::tailnet::TAILNET_PORT)).unwrap_or_else(|| "https://localhost".into());
        (core.store.push_subs()?, contact)
    };
    if subs.is_empty() {
        return Ok(0);
    }
    let kp = vapid()?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
    let body = serde_json::to_vec(note)?;
    let mut sent = 0;
    for (endpoint, p256dh, auth) in subs {
        let outcome = async {
            let ua_public = PublicKey::from_sec1_bytes(&B64.decode(&p256dh)?)?;
            let ua_auth = Auth::clone_from_slice(&B64.decode(&auth)?);
            let request = WebPushBuilder::new(endpoint.parse()?, ua_public, ua_auth)
                .with_valid_duration(Duration::from_secs(3600))
                .with_vapid(&kp, &contact)
                .build(body.clone())?;
            let request: reqwest::Request = request.try_into()?;
            anyhow::Ok(client.execute(request).await?.status())
        }
        .await;
        let mut core = shared.lock();
        match outcome {
            Ok(s) if s.is_success() => {
                sent += 1;
                core.store.push_result(&endpoint, true)?;
            }
            // The device unsubscribed or the subscription expired.
            Ok(s) if s.as_u16() == 404 || s.as_u16() == 410 => {
                say!("push: dropping a subscription the push service no longer knows ({s})");
                core.store.push_unsubscribe(&endpoint)?;
            }
            Ok(s) => {
                say!("push: {s} from the push service");
                core.store.push_result(&endpoint, false)?;
            }
            Err(e) => {
                say!("push: {e:#}");
                core.store.push_result(&endpoint, false)?;
            }
        }
    }
    Ok(sent)
}
