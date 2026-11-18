//! The hub's HTTP API (src/server.rs), as the Mac app's HubClient reads it:
//! `/api/state`, the writes the panel makes, and `/api/events` (server-sent
//! events) to stay live. Blocking calls: the panel makes them off the UI
//! thread, except in a headless run, where they're inline so a script's
//! frames are deterministic.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use serde_json::Value;

/// The made-up world (`PORT=7458 scripts/screenshots.sh --serve`).
pub const DEFAULT_HUB: &str = "http://127.0.0.1:7458";

/// The daily hub's port. The trial never talks to it (docs/rune.md).
const DAILY_PORT: &str = ":7500";

#[derive(Clone)]
pub struct Hub {
    base: String,
    agent: ureq::Agent,
    stream: ureq::Agent,
}

impl Hub {
    /// `WH_HUB`, or the made-up world. Refuses the daily hub.
    pub fn from_env() -> Result<Hub, String> {
        let base = std::env::var("WH_HUB").unwrap_or_else(|_| DEFAULT_HUB.to_string());
        Hub::new(base.trim().trim_end_matches('/'))
    }

    pub fn new(base: &str) -> Result<Hub, String> {
        let authority = base.split("://").nth(1).unwrap_or(base).split('/').next().unwrap_or("");
        if authority.ends_with(DAILY_PORT) {
            return Err(format!("{base} is the daily hub: wheelhouse-rune only talks to a trial hub (WH_HUB, default {DEFAULT_HUB})"));
        }
        let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false).build().into();
        // The event stream waits on the hub's keep-alives.
        let stream =
            ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(5))).timeout_recv_body(Some(Duration::from_secs(45))).http_status_as_error(false).build().into();
        Ok(Hub { base: base.to_string(), agent, stream })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn get(&self, path: &str) -> Result<Value, String> {
        let res = self.agent.get(format!("{}{path}", self.base)).header("Accept", "application/json").call();
        answer(res)
    }

    pub fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        let res = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Content-Type", "application/json")
            // Writes carry it; it's how the hub refuses other sites.
            .header("X-Wheelhouse", "rune")
            .send(body.to_string());
        answer(res)
    }

    /// Follow `/api/events` until it ends: `opened` once connected, then
    /// `event` for each one. Returns why it stopped.
    pub fn follow(&self, mut opened: impl FnMut(), mut event: impl FnMut()) -> String {
        let res = self.stream.get(format!("{}/api/events", self.base)).header("Accept", "text/event-stream").call();
        let res = match res {
            Ok(r) if r.status() == 200 => r,
            Ok(r) => return format!("The hub answered {}", r.status().as_u16()),
            Err(e) => return words(&e),
        };
        opened();
        let reader = BufReader::new(res.into_body().into_reader());
        for line in reader.lines() {
            match line {
                Ok(l) if l.starts_with("data:") => event(),
                Ok(_) => {}
                Err(e) => return e.to_string(),
            }
        }
        "The hub closed the stream".to_string()
    }
}

/// A query string's value, percent-encoded.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn answer(res: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Value, String> {
    let mut res = res.map_err(|e| words(&e))?;
    let status = res.status().as_u16();
    let text = res.body_mut().with_config().limit(64 << 20).read_to_string().map_err(|e| words(&e))?;
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(json);
    }
    let msg = json["error"].as_str().map(str::to_string).unwrap_or(text);
    Err(if msg.trim().is_empty() { format!("The hub answered {status}") } else { msg })
}

/// An error in the panel's words.
fn words(e: &ureq::Error) -> String {
    match e {
        ureq::Error::ConnectionFailed | ureq::Error::HostNotFound | ureq::Error::Timeout(_) => "The hub isn't running".to_string(),
        ureq::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::ConnectionRefused) => "The hub isn't running".to_string(),
        e => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_the_daily_hub() {
        assert!(Hub::new("http://127.0.0.1:7500").is_err());
        assert!(Hub::new("http://localhost:7500/").is_err());
        assert!(Hub::new("http://127.0.0.1:7458").is_ok());
        assert!(Hub::new("http://127.0.0.1:17500").is_ok());
    }
}
