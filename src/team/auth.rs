//! Signed requests from a member's hub to the workspace hub. The Tailscale
//! login gets a request in the door; this proves which member's hub sent it.

use anyhow::{Result, anyhow, ensure};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use sha2::Digest;
use std::collections::HashMap;

use crate::record::{Key, hex, keys};

/// How far a request's time may be from the workspace hub's.
pub const WINDOW_MS: i64 = 5 * 60 * 1000;

/// What a member's key signs for one request.
pub fn message(workspace: &str, method: &str, path: &str, time_ms: i64, body: &[u8]) -> Vec<u8> {
    format!("wh-team-req/v1\n{workspace}\n{}\n{path}\n{time_ms}\n{}", method.to_ascii_uppercase(), hex::encode(&sha2::Sha256::digest(body))).into_bytes()
}

/// The headers that sign a request: (`x-wh-key`, `x-wh-time`, `x-wh-sig`).
pub fn sign(key: &Key, workspace: &str, method: &str, path: &str, body: &[u8], now_ms: i64) -> [(&'static str, String); 3] {
    let sig = key.sign(&message(workspace, method, path, now_ms, body));
    [("x-wh-key", key.keyid()), ("x-wh-time", now_ms.to_string()), ("x-wh-sig", B64.encode(sig))]
}

/// A request as the workspace hub got it, with its signing headers.
pub struct Req<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub key: Option<&'a str>,
    pub time: Option<&'a str>,
    pub sig: Option<&'a str>,
}

/// Signatures seen lately, so a request is accepted once.
#[derive(Default)]
pub struct Replay {
    seen: HashMap<String, i64>,
}

impl Replay {
    /// Check a request's signature, its time, and that it's new; the key
    /// id it was signed with. Whether that key may make the request is the
    /// caller's to check.
    pub fn check(&mut self, workspace: &str, req: &Req, now_ms: i64) -> Result<String> {
        let Req { method, path, body, key, time, sig } = *req;
        let (key, time, sig) = match (key, time, sig) {
            (Some(k), Some(t), Some(s)) => (k, t, s),
            _ => return Err(anyhow!("an unsigned request")),
        };
        let t: i64 = time.parse().map_err(|_| anyhow!("x-wh-time isn't a number"))?;
        ensure!((t - now_ms).abs() <= WINDOW_MS, "the request's clock is more than 5 minutes off the workspace's");
        let raw = B64.decode(sig).map_err(|_| anyhow!("x-wh-sig isn't base64"))?;
        ensure!(keys::verify_by(key, &message(workspace, method, path, t, body), &raw), "the request's signature doesn't match");
        self.seen.retain(|_, at| now_ms - *at <= 2 * WINDOW_MS);
        ensure!(self.seen.len() < 100_000, "too many requests");
        ensure!(self.seen.insert(sig.to_string(), now_ms).is_none(), "a request sent twice");
        Ok(key.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_request_checks_once() {
        let k = Key::generate("owner");
        let now = 1_800_000_000_000;
        let h = sign(&k, "ws", "post", "/team/v1/log", b"{}", now);
        let mut r = Replay::default();
        let req = Req { method: "POST", path: "/team/v1/log", body: b"{}", key: Some(&h[0].1), time: Some(&h[1].1), sig: Some(&h[2].1) };
        assert_eq!(r.check("ws", &req, now + 1000).unwrap(), k.keyid());
        assert!(r.check("ws", &req, now + 2000).is_err(), "replayed");
    }

    #[test]
    fn anything_changed_fails() {
        let k = Key::generate("owner");
        let now = 1_800_000_000_000;
        let h = sign(&k, "ws", "POST", "/team/v1/log", b"{}", now);
        let c = |ws: &str, method: &str, path: &str, body: &[u8], t: i64| {
            Replay::default().check(ws, &Req { method, path, body, key: Some(&h[0].1), time: Some(&h[1].1), sig: Some(&h[2].1) }, t)
        };
        assert!(c("other", "POST", "/team/v1/log", b"{}", now).is_err(), "another workspace");
        assert!(c("ws", "GET", "/team/v1/log", b"{}", now).is_err(), "another method");
        assert!(c("ws", "POST", "/team/v1/dockets", b"{}", now).is_err(), "another path");
        assert!(c("ws", "POST", "/team/v1/log", b"{\"a\":1}", now).is_err(), "another body");
        assert!(c("ws", "POST", "/team/v1/log", b"{}", now + WINDOW_MS + 1).is_err(), "too late");
        let unsigned = Req { method: "POST", path: "/team/v1/log", body: b"{}", key: Some(&h[0].1), time: None, sig: Some(&h[2].1) };
        assert!(Replay::default().check("ws", &unsigned, now).is_err(), "unsigned");
    }
}
