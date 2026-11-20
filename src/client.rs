//! The CLI's side of the hub's socket: plain HTTP/1.1 over a unix socket.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// One request. `timeout` bounds connecting, writing and reading.
pub fn call(method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<(u16, Vec<u8>)> {
    let sock = crate::paths::socket();
    let mut s = UnixStream::connect(&sock).with_context(|| format!("the hub isn't running ({}): start it with `wh serve`", sock.display()))?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    let body = body.map(Value::to_string).unwrap_or_default();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nX-Wheelhouse: cli\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut resp = Vec::new();
    s.read_to_end(&mut resp)?;
    parse(&resp)
}

fn parse(resp: &[u8]) -> Result<(u16, Vec<u8>)> {
    let split = resp.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| anyhow!("a response without headers"))?;
    let head = String::from_utf8_lossy(&resp[..split]);
    let mut body = resp[split + 4..].to_vec();
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| anyhow!("a response without a status"))?;
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if chunked {
        body = dechunk(&body)?;
    }
    Ok((status, body))
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n").ok_or_else(|| anyhow!("a bad chunk"))?;
        let size = usize::from_str_radix(String::from_utf8_lossy(&b[..eol]).split(';').next().unwrap_or("").trim(), 16)?;
        b = &b[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size {
            bail!("a short chunk");
        }
        out.extend_from_slice(&b[..size]);
        b = &b[(size + 2).min(b.len())..];
    }
}

/// A JSON request; an error response becomes an error.
pub fn json(method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    json_within(method, path, body, Duration::from_secs(15))
}

/// `json`, for a write that takes as long as GitHub does (posting a review
/// and its replies).
pub fn json_within(method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Value> {
    let (status, bytes) = call(method, path, body, timeout)?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    if !(200..300).contains(&status) {
        let msg = v["error"].as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
        bail!("{msg}");
    }
    Ok(v)
}

pub fn text(path: &str) -> Result<String> {
    let (status, bytes) = call("GET", path, None, Duration::from_secs(15))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !(200..300).contains(&status) {
        bail!("{}", text.trim());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn chunked_bodies() {
        let resp = b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let (status, body) = super::parse(resp).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"hello world");
        let resp = b"HTTP/1.1 404 Not Found\r\ncontent-length: 2\r\n\r\nno";
        assert_eq!(super::parse(resp).unwrap(), (404, b"no".to_vec()));
    }
}
