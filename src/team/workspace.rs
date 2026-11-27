//! The workspace hub: `wh serve --workspace`. It holds a workspace's member
//! log and its shared dockets, and orders their entries. It needs no
//! runtime, Keychain or launchctl, so it runs on Linux as well as a Mac.
//!
//! It signs nothing anyone relies on. Every entry is signed by the member,
//! hub or agent that wrote it, and checked here against the roster and the
//! shared-docket rules before it's ordered; every member's hub checks it
//! again as it follows. docs/teams.md has the API.

use anyhow::{Context, Result, anyhow, bail, ensure};
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Digest;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::auth::{Replay, Req};
use super::roster::{self, Roster, open_envelope};
use super::shared::{self, People};
use crate::record::{Stored, chain, hex};
use crate::store::Store;

/// Where the workspace hub keeps its state: `$WH_STATE`, or
/// `~/.local/state/wheelhouse-workspace` (never the daily hub's).
pub fn state_dir() -> std::path::PathBuf {
    match std::env::var_os("WH_STATE") {
        Some(d) => d.into(),
        None => crate::paths::home().join(".local/state/wheelhouse-workspace"),
    }
}

/// Where it listens: `$WH_WORKSPACE_HTTP`, or 127.0.0.1:7700, for
/// `tailscale serve` to publish.
pub fn http_addr() -> Result<std::net::SocketAddr> {
    let v = std::env::var("WH_WORKSPACE_HTTP").unwrap_or_else(|_| "127.0.0.1:7700".into());
    v.parse().with_context(|| format!("WH_WORKSPACE_HTTP={v} isn't an address"))
}

pub struct Ws {
    store: Store,
    /// The roster, once the workspace exists.
    roster: Option<Roster>,
    /// Who's on each shared docket.
    people: BTreeMap<String, People>,
    /// Numbers handed out for dockets not shared yet, and to whom.
    reserved: BTreeMap<String, String>,
    replay: Replay,
    /// The setup code, until the workspace is made.
    setup: Option<String>,
    setup_path: std::path::PathBuf,
    /// Failed joins and creates lately, by Tailscale login, to slow guessing
    /// without one login locking out the rest.
    failures: BTreeMap<String, Vec<std::time::Instant>>,
}

#[derive(Clone)]
pub struct WsShared {
    ws: Arc<Mutex<Ws>>,
    /// Counts every entry ordered: what `/changes` waits on.
    changes: Arc<tokio::sync::watch::Sender<u64>>,
}

impl WsShared {
    fn lock(&self) -> MutexGuard<'_, Ws> {
        self.ws.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn changed(&self) {
        self.changes.send_modify(|n| *n += 1);
    }
}

type ApiResult = std::result::Result<Json<Value>, (StatusCode, Json<Value>)>;

fn refuse(status: StatusCode, msg: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": msg.to_string() })))
}

fn bad(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    refuse(StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}"))
}

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

impl Ws {
    /// Open the store and fold what it holds, checking all of it: a
    /// workspace hub whose own record doesn't check doesn't start.
    pub fn open(store: Store, setup_path: std::path::PathBuf) -> Result<Ws> {
        let mut ws = Ws { store, roster: None, people: BTreeMap::new(), reserved: BTreeMap::new(), replay: Replay::default(), setup: None, setup_path, failures: BTreeMap::new() };
        let ids = ws.store.docket_ids()?;
        let log = ids.iter().find(|d| d.ends_with("-0")).cloned();
        if let Some(log) = log {
            let r = Roster::fold(&ws.store.entries(&log)?).with_context(|| format!("the member log {log} doesn't check"))?;
            for d in ids.iter().filter(|d| **d != log) {
                let entries = ws.store.entries(d)?;
                // Checked with every key the log has named: one removed since
                // still names its member, so what they wrote checks.
                let rep = crate::record::verify::verify(d, &entries, |k| r.any_key(k).is_some());
                ensure!(rep.ok, "{d} doesn't check: {}", rep.problems.join("; "));
                let mut p = People::default();
                for s in &entries {
                    p.apply(&s.entry()?);
                }
                ws.people.insert(d.clone(), p);
            }
            let saved: BTreeMap<String, String> = ws.store.meta_get("reserved")?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
            for (id, who) in saved {
                if !ws.people.contains_key(&id) {
                    ws.reserved.insert(id, who);
                }
            }
            ws.roster = Some(r);
        } else {
            ws.setup = Some(match std::fs::read_to_string(&ws.setup_path) {
                Ok(c) if c.trim().len() >= 20 => c.trim().to_string(),
                _ => {
                    let code = format!("whs1.{}", hex::encode(&crate::record::keys::random32()[..20]));
                    std::fs::write(&ws.setup_path, format!("{code}\n")).with_context(|| format!("writing {}", ws.setup_path.display()))?;
                    set_mode(&ws.setup_path, 0o600);
                    code
                }
            });
        }
        Ok(ws)
    }

    fn roster(&self) -> Result<&Roster> {
        self.roster.as_ref().ok_or_else(|| anyhow!("this workspace hub has no workspace yet: wh team create"))
    }

    fn log_id(&self) -> Result<String> {
        Ok(format!("{}-0", self.roster()?.prefix))
    }

    fn slow_down(&mut self, login: &str) -> Result<()> {
        let now = std::time::Instant::now();
        self.failures.retain(|_, ts| {
            ts.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
            !ts.is_empty()
        });
        ensure!(self.failures.get(login).is_none_or(|ts| ts.len() < 10), "too many failed attempts; wait a minute");
        Ok(())
    }

    fn failed(&mut self, login: &str) {
        if self.failures.len() < 10_000 {
            self.failures.entry(login.to_string()).or_default().push(std::time::Instant::now());
        }
    }

    /// Order one member log entry, after the rules pass.
    fn order_log(&mut self, envelope: &str) -> Result<Stored> {
        let (e, keyid) = open_envelope(envelope)?;
        let id = chain::id_string(&chain::entry_id(envelope.as_bytes()));
        let mut r = self.roster.clone().unwrap_or_default();
        let log = if r.seq == 0 { e.data["prefix"].as_str().map(|p| format!("{p}-0")).unwrap_or_default() } else { format!("{}-0", r.prefix) };
        ensure!(e.docket == log, "an entry for {}, not the member log {log}", e.docket);
        // Its time, by this hub's clock: a join can't be backdated into an
        // invite's life. A key may have waited in an offline outbox.
        let at = crate::time::parse(&e.at).map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64).ok_or_else(|| anyhow!("its time doesn't read"))?;
        let now = now_ms();
        ensure!(at <= now + shared::SKEW_MS, "its time {} is ahead of the workspace's clock", e.at);
        ensure!(e.kind == "key" || at >= now - shared::SKEW_MS, "its time {} is more than 5 minutes behind the workspace's clock", e.at);
        r.check(&e, &keyid, &id)?;
        let s = self.store.append_folded(&log, envelope, &e.kind, &e.at, &keyid, |_, _| None)?;
        ensure!(s.id == id, "the store gave the entry another id");
        r.apply(s.seq, &s.chain, &s.id, &e, &keyid);
        self.roster = Some(r);
        Ok(s)
    }

    /// Order one entry on a shared docket, after the rules pass.
    fn order_shared(&mut self, docket: &str, envelope: &str) -> Result<Stored> {
        let roster = self.roster()?.clone();
        ensure!(shared::docket_ok(&roster.prefix, docket), "{docket} isn't one of this workspace's dockets");
        let (e, keyid) = open_envelope(envelope)?;
        let id = chain::id_string(&chain::entry_id(envelope.as_bytes()));
        // Once: an old entry sent again (a revoked trust, a past promotion)
        // would bring back what a later one took away.
        ensure!(!self.store.has_entry(docket, &id)?, "that entry is on {docket} already");
        let next = self.store.entries_len(docket)? + 1;
        if next == 1 {
            ensure!(self.reserved.contains_key(docket), "{docket} wasn't handed out: ask for a number first");
        }
        let people = self.people.get(docket).cloned().unwrap_or_default();
        shared::check(&roster, &people, docket, next, &e, &keyid, Some(now_ms()))?;
        for b in &e.blobs {
            let h = b.strip_prefix("sha256:").unwrap_or(b);
            ensure!(self.store.has_blob(h)?, "it names a file the workspace doesn't have yet: send {} first", &b[..b.len().min(19)]);
        }
        let s = self.store.append_folded(docket, envelope, &e.kind, &e.at, &keyid, |_, _| None)?;
        ensure!(s.seq == next, "{docket} moved while ordering");
        let mut p = people;
        p.apply(&e);
        self.people.insert(docket.to_string(), p);
        if next == 1 {
            self.reserved.remove(docket);
            self.save_reserved()?;
        }
        Ok(s)
    }

    fn save_reserved(&mut self) -> Result<()> {
        let v = serde_json::to_string(&self.reserved)?;
        self.store.meta_set("reserved", &v)
    }

    /// The member a signed request is from, by their own key; with logins
    /// required, its Tailscale login must be theirs.
    fn member(&mut self, method: &Method, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Result<String> {
        let roster = self.roster()?.clone();
        let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
        let req = Req { method: method.as_str(), path, body, key: h("x-wh-key"), time: h("x-wh-time"), sig: h("x-wh-sig") };
        let now = now_ms();
        let (keyid, sig) = self.replay.verify(&roster.id, &req, now)?;
        let m = roster.member_by_key(&keyid).ok_or_else(|| anyhow!("{} isn't an active member's key", roster::short(&keyid)))?;
        let login = h("tailscale-user-login").ok_or_else(|| anyhow!("no Tailscale login: reach the workspace through tailscale serve"))?;
        ensure!(m.login.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(login)), "the Tailscale login {login} isn't {}'s", m.handle);
        // A member's, and theirs: only now is it remembered.
        self.replay.remember(&sig, now)?;
        Ok(m.handle.clone())
    }
}

fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

pub fn router(s: WsShared) -> Router {
    Router::new()
        .route("/team/v1/hello", get(hello))
        .route("/team/v1/create", post(create))
        .route("/team/v1/join", post(join))
        .route("/team/v1/changes", get(changes))
        .route("/team/v1/log", get(log_get).post(log_post))
        .route("/team/v1/dockets", get(dockets_list).post(dockets_new))
        .route("/team/v1/dockets/{id}", get(docket_get).post(docket_post))
        .route("/team/v1/blobs/{hex}", get(blob_get).put(blob_put).layer(axum::extract::DefaultBodyLimit::max(crate::artifacts::MAX + 1024)))
        .layer(axum::extract::DefaultBodyLimit::max(2 * roster::MAX_ENVELOPE + 64 * 1024))
        .with_state(s)
}

async fn hello(State(s): State<WsShared>, headers: HeaderMap) -> ApiResult {
    if headers.get("tailscale-user-login").is_none() {
        return Err(refuse(StatusCode::FORBIDDEN, "reach the workspace through tailscale serve"));
    }
    let ws = s.lock();
    Ok(Json(match &ws.roster {
        Some(r) => json!({ "workspace": true, "id": r.id, "name": r.name, "prefix": r.prefix }),
        None => json!({ "workspace": false }),
    }))
}

#[derive(Deserialize)]
struct CreateBody {
    setup: String,
    /// The `workspace` entry, then the creator's `key` entries.
    entries: Vec<String>,
}

async fn create(State(s): State<WsShared>, headers: HeaderMap, Json(b): Json<CreateBody>) -> ApiResult {
    let login = headers.get("tailscale-user-login").and_then(|v| v.to_str().ok()).map(str::to_string);
    let out = {
        let mut ws = s.lock();
        (|| -> Result<Value> {
            let who = login.clone().unwrap_or_default();
            ws.slow_down(&who)?;
            ensure!(ws.roster.is_none(), "this workspace hub has a workspace already");
            let want = ws.setup.clone().unwrap_or_default();
            if !constant_eq(want.as_bytes(), b.setup.trim().as_bytes()) {
                ws.failed(&who);
                bail!("that isn't this workspace hub's setup code");
            }
            ensure!(!b.entries.is_empty() && b.entries.len() <= 16, "a workspace, then at most 15 keys");
            let (first, _) = open_envelope(&b.entries[0])?;
            let l = login.as_deref().ok_or_else(|| anyhow!("no Tailscale login: reach the workspace through tailscale serve"))?;
            ensure!(first.data["login"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(l)), "the creator's login must be the one Tailscale sees: {l}");
            let mut n = 0;
            for env in &b.entries {
                ws.order_log(env)?;
                n += 1;
            }
            ws.setup = None;
            let _ = std::fs::remove_file(&ws.setup_path);
            let r = ws.roster()?;
            say!("workspace {} ({}) made by {}", r.name, r.prefix, first.actor.id);
            Ok(json!({ "id": r.id, "prefix": r.prefix, "entries": n }))
        })()
    };
    let v = out.map_err(bad)?;
    s.changed();
    Ok(Json(v))
}

/// Compare secrets without stopping at the first difference.
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Deserialize)]
struct JoinBody {
    /// The `join` entry, then the joiner's `key` entries.
    entries: Vec<String>,
}

async fn join(State(s): State<WsShared>, headers: HeaderMap, Json(b): Json<JoinBody>) -> ApiResult {
    let login = headers.get("tailscale-user-login").and_then(|v| v.to_str().ok()).map(str::to_string);
    let out = {
        let mut ws = s.lock();
        (|| -> Result<Value> {
            let who = login.clone().unwrap_or_default();
            ws.slow_down(&who)?;
            ws.roster()?;
            ensure!(!b.entries.is_empty() && b.entries.len() <= 16, "a join, then at most 15 keys");
            let (first, _) = open_envelope(&b.entries[0])?;
            ensure!(first.kind == "join", "a join starts with `join`");
            let l = login.as_deref().ok_or_else(|| anyhow!("no Tailscale login: reach the workspace through tailscale serve"))?;
            ensure!(first.data["login"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(l)), "the login you join with must be the one Tailscale sees: {l}");
            if let Err(e) = ws.order_log(&b.entries[0]) {
                ws.failed(&who);
                return Err(e);
            }
            for env in &b.entries[1..] {
                let (e, _) = open_envelope(env)?;
                ensure!(e.kind == "key", "after a join come keys");
                ws.order_log(env)?;
            }
            let r = ws.roster()?;
            say!("{} joined {}", first.actor.id, r.prefix);
            Ok(json!({ "id": r.id, "name": r.name, "prefix": r.prefix, "log": r.seq }))
        })()
    };
    let v = out.map_err(bad)?;
    s.changed();
    Ok(Json(v))
}

/// The member a request is from, or why not.
fn member(s: &WsShared, method: &Method, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> std::result::Result<String, (StatusCode, Json<Value>)> {
    s.lock().member(method, uri, headers, body).map_err(|e| refuse(StatusCode::FORBIDDEN, format!("{e:#}")))
}

#[derive(Deserialize)]
struct ChangesQ {
    #[serde(default)]
    cursor: u64,
    #[serde(default)]
    wait: u64,
}

async fn changes(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap, Query(q): Query<ChangesQ>) -> ApiResult {
    member(&s, &method, &uri, &headers, b"")?;
    let mut rx = s.changes.subscribe();
    if *rx.borrow() <= q.cursor && q.wait > 0 {
        let _ = tokio::time::timeout(Duration::from_secs(q.wait.min(30)), rx.wait_for(|n| *n > q.cursor)).await;
    }
    let cursor = *rx.borrow();
    let ws = s.lock();
    let r = ws.roster().map_err(bad)?;
    let mut dockets = serde_json::Map::new();
    for d in ws.people.keys() {
        dockets.insert(d.clone(), json!(ws.store.entries_len(d).unwrap_or(0)));
    }
    let log = ws.store.entries_len(&format!("{}-0", r.prefix)).map_err(bad)?;
    Ok(Json(json!({ "cursor": cursor, "log": log, "dockets": dockets })))
}

#[derive(Deserialize)]
struct AfterQ {
    #[serde(default)]
    after: i64,
}

/// At most this many entries in one answer: a hub far behind asks again.
const PAGE: usize = 500;

/// And at most this many bytes of envelopes (at least one entry).
const PAGE_BYTES: usize = 4 << 20;

fn page(store: &Store, docket: &str, after: i64) -> Result<Value> {
    let mut entries: Vec<Stored> = store.entries_after(docket, after, PAGE)?;
    let mut total = 0;
    let keep = entries
        .iter()
        .position(|s| {
            total += s.envelope.len();
            total > PAGE_BYTES
        })
        .unwrap_or(entries.len())
        .max(1);
    entries.truncate(keep);
    Ok(json!({ "docket": docket, "entries": entries }))
}

async fn log_get(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap, Query(q): Query<AfterQ>) -> ApiResult {
    member(&s, &method, &uri, &headers, b"")?;
    let ws = s.lock();
    let log = ws.log_id().map_err(bad)?;
    page(&ws.store, &log, q.after).map(Json).map_err(bad)
}

async fn log_post(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> ApiResult {
    member(&s, &method, &uri, &headers, &body)?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| bad(e.into()))?;
    let env = v["envelope"].as_str().ok_or_else(|| refuse(StatusCode::BAD_REQUEST, "an envelope, please"))?.to_string();
    let st = s.lock().order_log(&env).map_err(bad)?;
    s.changed();
    Ok(Json(json!({ "stored": st })))
}

async fn dockets_list(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap) -> ApiResult {
    member(&s, &method, &uri, &headers, b"")?;
    let ws = s.lock();
    let mut out = Vec::new();
    for (d, p) in &ws.people {
        out.push(json!({ "id": d, "entries": ws.store.entries_len(d).unwrap_or(0), "owners": p.owners, "members": p.members }));
    }
    Ok(Json(json!({ "dockets": out })))
}

async fn dockets_new(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> ApiResult {
    let who = member(&s, &method, &uri, &headers, &body)?;
    let mut ws = s.lock();
    let prefix = ws.roster().map_err(bad)?.prefix.clone();
    ensure_ok(ws.reserved.values().filter(|w| **w == who).count() < 20, "you have 20 numbers handed out and not used")?;
    let n: i64 = ws.store.meta_get("shared_n").map_err(bad)?.and_then(|v| v.parse().ok()).unwrap_or(0) + 1;
    ws.store.meta_set("shared_n", &n.to_string()).map_err(bad)?;
    let id = format!("{prefix}-{n}");
    ws.reserved.insert(id.clone(), who.clone());
    ws.save_reserved().map_err(bad)?;
    say!("{id} handed to {who}");
    Ok(Json(json!({ "id": id })))
}

fn ensure_ok(ok: bool, msg: &str) -> std::result::Result<(), (StatusCode, Json<Value>)> {
    if ok { Ok(()) } else { Err(refuse(StatusCode::TOO_MANY_REQUESTS, msg)) }
}

async fn docket_get(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap, Path(id): Path<String>, Query(q): Query<AfterQ>) -> ApiResult {
    member(&s, &method, &uri, &headers, b"")?;
    let ws = s.lock();
    if !ws.people.contains_key(&id) {
        return Err(refuse(StatusCode::NOT_FOUND, format!("no shared docket {id}")));
    }
    page(&ws.store, &id, q.after).map(Json).map_err(bad)
}

async fn docket_post(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> ApiResult {
    member(&s, &method, &uri, &headers, &body)?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| bad(e.into()))?;
    let env = v["envelope"].as_str().ok_or_else(|| refuse(StatusCode::BAD_REQUEST, "an envelope, please"))?.to_string();
    let st = s.lock().order_shared(&id, &env).map_err(bad)?;
    s.changed();
    Ok(Json(json!({ "stored": st })))
}

fn hex_ok(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

async fn blob_get(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap, Path(h): Path<String>) -> Response {
    if let Err(e) = member(&s, &method, &uri, &headers, b"") {
        return e.into_response();
    }
    if !hex_ok(&h) {
        return (StatusCode::BAD_REQUEST, "not a sha256 digest").into_response();
    }
    let found = s.lock().store.blob(&h);
    match found {
        Ok(Some(b)) => ([(axum::http::header::CONTENT_TYPE, "application/octet-stream")], b).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no such file").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
    }
}

async fn blob_put(State(s): State<WsShared>, method: Method, uri: Uri, headers: HeaderMap, Path(h): Path<String>, body: Bytes) -> ApiResult {
    member(&s, &method, &uri, &headers, &body)?;
    if !hex_ok(&h) {
        return Err(refuse(StatusCode::BAD_REQUEST, "not a sha256 digest"));
    }
    if body.len() > crate::artifacts::MAX {
        return Err(refuse(StatusCode::PAYLOAD_TOO_LARGE, "files are at most 25 MB"));
    }
    if hex::encode(&sha2::Sha256::digest(&body)) != h {
        return Err(refuse(StatusCode::UNPROCESSABLE_ENTITY, "the file doesn't hash to its name"));
    }
    s.lock().store.put_blob(&h, &body, &crate::time::now_ms()).map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}

/// `wh serve --workspace`.
pub fn serve() -> Result<()> {
    let dir = state_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    set_mode(&dir, 0o700);
    let db = dir.join("workspace.db");
    let store = Store::open(&db)?;
    for f in ["workspace.db", "workspace.db-wal", "workspace.db-shm"] {
        set_mode(&dir.join(f), 0o600);
    }
    let ws = Ws::open(store, dir.join("setup-code"))?;
    let addr = http_addr()?;
    match (&ws.roster, &ws.setup) {
        (Some(r), _) => say!("workspace {} ({}): {} members, {} shared dockets", r.name, r.prefix, r.members.values().filter(|m| m.active()).count(), ws.people.len()),
        (None, Some(code)) => {
            say!("no workspace yet. On the first admin's Mac, with this hub published (tailscale serve --bg {}):", addr.port());
            say!("  wh team create NAME --prefix ACME --url https://THIS-MACHINE.ts.net --setup {code}");
        }
        _ => {}
    }
    let (tx, _) = tokio::sync::watch::channel(0u64);
    let shared = WsShared { ws: Arc::new(Mutex::new(ws)), changes: Arc::new(tx) };
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let tcp = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
        say!("workspace hub on http://{addr}");
        // On SIGTERM or Ctrl-C, stop taking requests and let those under
        // way finish, but don't wait out a /changes long poll: members'
        // hubs ask again. Every entry is ordered in one transaction, so a
        // request cut short leaves nothing half written.
        let server = axum::serve(tcp, router(shared)).with_graceful_shutdown(stop_signal());
        tokio::select! {
            r = server => r.context("http")?,
            _ = async {
                stop_signal().await;
                tokio::time::sleep(Duration::from_secs(2)).await;
            } => {}
        }
        say!("workspace hub stopped");
        Ok(())
    })
}

/// SIGTERM (docker stop, systemd, launchd) or Ctrl-C. A handler of its own,
/// so `wh serve --workspace` stops at once as a container's PID 1, which
/// the kernel doesn't send signals it has no handler for.
async fn stop_signal() {
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = term => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::entry::Actor;
    use crate::record::{Entry, Envelope, Key};
    use crate::team::roster::tests::{create, invite_and_join};

    fn now_at() -> String {
        crate::time::now_ms()
    }

    #[test]
    fn an_entry_sent_twice_is_ordered_once() {
        let jordan = Key::generate("owner");
        let sam = Key::generate("owner");
        let mut l = create(&jordan);
        invite_and_join(&mut l, ("jordan", &jordan), "sam", "member", &sam).unwrap();
        let setup = std::env::temp_dir().join(format!("wh-test-unused-setup-{}", std::process::id()));
        let _gone = crate::hub::TestDir(setup.clone());
        let mut ws = Ws::open(Store::in_memory().unwrap(), setup).unwrap();
        ws.roster = Some(l.roster.clone());
        ws.reserved.insert("ACME-1".into(), "jordan".into());
        let sign = |kind: &str, who: &str, data: serde_json::Value, k: &Key| {
            let mut e = Entry::new("ACME-1", kind, &now_at(), Actor::new("person", who));
            e.data = data;
            Envelope::sign(&e.to_bytes(), k).to_string_compact()
        };
        ws.order_shared("ACME-1", &sign("share", "jordan", serde_json::json!({ "owners": ["jordan"], "title": "T" }), &jordan)).unwrap();
        ws.order_shared("ACME-1", &sign("people", "sam", serde_json::json!({ "handle": "sam", "role": "member" }), &sam)).unwrap();
        let trust = sign("trust", "jordan", serde_json::json!({ "teammate": "sam", "on": true }), &jordan);
        ws.order_shared("ACME-1", &trust).unwrap();
        ws.order_shared("ACME-1", &sign("trust", "jordan", serde_json::json!({ "teammate": "sam", "on": false }), &jordan)).unwrap();
        // Sam sends Jordan's old trust again, to get let in again.
        let err = ws.order_shared("ACME-1", &trust).unwrap_err();
        assert!(err.to_string().contains("already"), "{err}");
        assert!(!ws.people["ACME-1"].trust.contains(&("jordan".to_string(), "sam".to_string())));
        // A number not handed out, and one spelled another way, are refused.
        assert!(ws.order_shared("ACME-2", &sign("share", "jordan", serde_json::json!({ "owners": ["jordan"] }), &jordan)).is_err());
        assert!(ws.order_shared("ACME-01", &trust).is_err());
    }
}
