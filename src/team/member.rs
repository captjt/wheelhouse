//! A member's side of a workspace: the hub on each engineer's Mac.
//!
//! - It keeps a copy of the member log and of every shared docket, under
//!   the workspace's ids and in the workspace's order, and takes a new entry
//!   only if it follows the chain it has and passes the rules.
//! - It writes to a shared docket by signing an entry and putting it in the
//!   outbox; `follow` sends the outbox in order and pulls what's new. An
//!   entry is on the local copy once the workspace hub has ordered it.
//! - It keeps back what never leaves (`shared::redact`), and holds an entry
//!   that looks like a secret until its author says.
//!
//! docs/teams.md is the design.

use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use super::roster::{self, Roster, open_envelope};
use super::shared::{self, People};
use crate::hub::{Core, Shared, Who};
use crate::record::entry::Actor;
use crate::record::{Entry, Envelope, Key, Stored, chain, hex};

/// What a member's hub keeps about its workspace (in the store's meta).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// The workspace hub's address: `https://ws.tail1234.ts.net`.
    pub url: String,
    pub id: String,
    pub name: String,
    pub prefix: String,
    /// This hub's member: its handle in the workspace.
    pub handle: String,
}

/// A member's hub's live view of its workspace.
pub struct Team {
    pub config: Config,
    pub roster: Roster,
    pub people: BTreeMap<String, People>,
    /// Local dockets shared, by the shared docket they went on as.
    pub aliases: BTreeMap<String, String>,
    /// Shared dockets this hub stopped following, and why: a workspace hub
    /// that changed history, or an entry that broke the rules.
    pub halted: BTreeMap<String, String>,
    /// Whether the last call to the workspace hub got through, and the
    /// last error if not.
    pub online: bool,
    pub last_error: Option<String>,
    pub cursor: u64,
    pub wake: Arc<tokio::sync::Notify>,
    /// One send or pull at a time: the follow loop and `wh team sync`
    /// never take in the same entry twice.
    pub busy: Arc<tokio::sync::Mutex<()>>,
}

impl Team {
    pub fn log_id(&self) -> String {
        format!("{}-0", self.config.prefix)
    }

    /// Whether a docket id is one of this workspace's shared dockets.
    pub fn shares(&self, docket: &str) -> bool {
        shared::docket_ok(&self.config.prefix, docket)
    }

    /// Whether a docket id is a shared docket or the member log.
    pub fn owns_id(&self, docket: &str) -> bool {
        self.shares(docket) || docket == self.log_id()
    }
}

const META: &str = "team";
const ALIASES: &str = "team_aliases";
const HALTED: &str = "team_halted";

/// Load the workspace this hub is a member of, if any, checking its copy of
/// the member log as it folds it.
pub fn load(core: &mut Core) -> Result<()> {
    let Some(c) = core.store.meta_get(META)? else { return Ok(()) };
    let config: Config = serde_json::from_str(&c).context("the team config doesn't read")?;
    core.team = Some(team_of(core, config)?);
    Ok(())
}

/// A workspace's view from this hub's store, checking its copy of the
/// member log as it folds it.
fn team_of(core: &Core, config: Config) -> Result<Team> {
    ensure!(roster::prefix_ok(&config.prefix), "the workspace's prefix {} isn't one", config.prefix);
    let log = format!("{}-0", config.prefix);
    let roster = Roster::fold(&core.store.entries(&log)?).with_context(|| format!("this hub's copy of {log} doesn't check"))?;
    let mut people = BTreeMap::new();
    for id in core.store.docket_ids()? {
        if id == log || !id.starts_with(&format!("{}-", config.prefix)) {
            continue;
        }
        let mut p = People::default();
        for s in core.store.entries(&id)? {
            p.apply(&s.entry()?);
        }
        people.insert(id, p);
    }
    let aliases = core.store.meta_get(ALIASES)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    let halted = core.store.meta_get(HALTED)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    Ok(Team {
        config,
        roster,
        people,
        aliases,
        halted,
        online: false,
        last_error: None,
        cursor: 0,
        wake: Arc::new(tokio::sync::Notify::new()),
        busy: Arc::new(tokio::sync::Mutex::new(())),
    })
}

fn save(core: &mut Core) -> Result<()> {
    let Some(t) = &core.team else { return Ok(()) };
    let (c, a, h) = (serde_json::to_string(&t.config)?, serde_json::to_string(&t.aliases)?, serde_json::to_string(&t.halted)?);
    core.store.meta_set(META, &c)?;
    core.store.meta_set(ALIASES, &a)?;
    core.store.meta_set(HALTED, &h)
}

/// The actor and key name an entry on a shared docket is written as:
/// qualified by member, so `hub@sam`, never `hub@local`.
fn actor(handle: &str, who: &Who) -> (Actor, String) {
    match who {
        Who::Hub => (Actor::new("hub", &format!("hub@{handle}")), "hub".into()),
        Who::Owner => (Actor::new("person", handle), "owner".into()),
        Who::Agent { app, block, session } => {
            (Actor::new("agent", &format!("{app}@{handle}")).with("block", block.as_str()).with("session", session.clone()), format!("agent-{app}@local"))
        }
    }
}

fn key_use(name: &str) -> &'static str {
    if name == "hub" { "hub" } else { "agent" }
}

fn sign(core: &mut Core, key_name: &str, e: &Entry) -> Result<(String, String, bool)> {
    let at = e.at.clone();
    let (envelope, keyid, new) = {
        let (k, new) = core.keys.get_or_create(key_name)?;
        (Envelope::sign(&e.to_bytes(), k).to_string_compact(), k.keyid(), new)
    };
    if new {
        core.store.put_key(&keyid, key_name, &at)?;
    }
    Ok((envelope, keyid, new))
}

fn id_of(envelope: &str) -> String {
    chain::id_string(&chain::entry_id(envelope.as_bytes()))
}

/// Make sure the workspace knows a key of this hub's before an entry signed
/// with it goes: a `key` entry, signed by the member key, goes in the
/// outbox ahead of it.
fn register_key(core: &mut Core, key_name: &str, keyid: &str) -> Result<()> {
    let t = core.team.as_ref().ok_or_else(|| anyhow!("not in a workspace"))?;
    let handle = t.config.handle.clone();
    if let Some(k) = t.roster.keys.get(keyid) {
        // Named already: by this member, or else something's wrong.
        ensure!(k.handle == handle, "this hub's key {} is in the workspace as {}'s: tell the workspace's admin", roster::short(keyid), k.handle);
        return Ok(());
    }
    if key_name == "owner" {
        return Ok(());
    }
    let log = t.log_id();
    let queued = core.store.outbox()?.iter().any(|o| o.docket == log && open_envelope(&o.envelope).is_ok_and(|(e, _)| e.kind == "key" && e.data["keyid"] == keyid));
    if queued {
        return Ok(());
    }
    let wid = t.config.id.clone();
    let e = key_entry(core, &log, &wid, &handle, key_name)?;
    let (env, _, _) = sign(core, "owner", &e)?;
    core.store.outbox_push(&log, &id_of(&env), &env, "queued", None)?;
    Ok(())
}

/// A `key` entry naming this hub's key `key_name`, with the key's own
/// signature to show this member holds it.
fn key_entry(core: &mut Core, log: &str, workspace: &str, handle: &str, key_name: &str) -> Result<Entry> {
    let (keyid, proof) = {
        let k = core.keys.get_or_create(key_name)?.0;
        (k.keyid(), B64.encode(k.sign(&roster::key_message(workspace, handle, &k.keyid()))))
    };
    let mut e = Entry::new(log, "key", &crate::time::now_ms(), Actor::new("person", handle));
    e.data = json!({ "keyid": keyid, "use": key_use(key_name), "name": key_name, "proof": proof });
    Ok(e)
}

/// Write an entry on a shared docket: redact what never leaves, check it
/// against the rules as this hub knows them, sign it, and put it in the
/// outbox. Its `seq` is 0 until the workspace hub orders it.
pub fn append(core: &mut Core, docket: &str, kind: &str, who: &Who, body: Option<String>, data: Value, blobs: Vec<String>) -> Result<Stored> {
    let t = core.team.as_ref().ok_or_else(|| anyhow!("not in a workspace"))?;
    if let Some(why) = t.halted.get(docket) {
        bail!("{docket} isn't followed: {why}");
    }
    let handle = t.config.handle.clone();
    let people = t.people.get(docket).cloned().unwrap_or_default();
    let (actor, key_name) = actor(&handle, who);
    let (body, data, private) = shared::redact(kind, body, data, people.private_prompts.contains(&handle));
    if let Some(bytes) = &private {
        core.store.put_blob(&hex::encode(&sha2::Sha256::digest(bytes)), bytes, &crate::time::now_ms())?;
    }
    let mut e = Entry::new(docket, kind, &crate::time::now_ms(), actor);
    e.body = body;
    e.data = data;
    e.blobs = blobs;
    // The key it's signed with, made now if it's new.
    let keyid = core.keys.get_or_create(&key_name)?.0.keyid();
    // The rules, as this hub knows them, before anything is signed: a
    // refusal here is the caller's to see, not an outbox entry to fail later.
    {
        let t = core.team.as_ref().expect("checked above");
        let mut roster = t.roster.clone();
        if !roster.keys.contains_key(&keyid) {
            // Its `key` entry goes first; check as if it were in.
            roster.keys.insert(
                keyid.clone(),
                roster::KeyInfo { handle: handle.clone(), use_: key_use(&key_name).into(), name: key_name.clone(), since: roster.seq + 1, revoked_at: None },
            );
        }
        let next = core.store.entries_len(docket)? + core.store.outbox()?.iter().filter(|o| o.docket == docket).count() as i64 + 1;
        shared::check(&roster, &people, docket, next, &e, &keyid, None)?;
    }
    register_key(core, &key_name, &keyid)?;
    let (envelope, _, _) = sign(core, &key_name, &e)?;
    let id = id_of(&envelope);
    let held = shared::secret_in(e.body.as_deref(), &e.data);
    core.store.outbox_push(docket, &id, &envelope, if held.is_some() { "held" } else { "queued" }, held.as_deref().map(|w| format!("it looks like it holds {w}")).as_deref())?;
    if let Some(w) = held {
        say!("{docket}: an entry held back: it looks like it holds {w} (Send anyway or Drop)");
    }
    if let Some(t) = &core.team {
        t.wake.notify_one();
    }
    let _ = core.events.send(json!({ "type": "team" }));
    Ok(Stored { docket: docket.to_string(), seq: 0, id, chain: String::new(), envelope })
}

/// Whether this hub may write `kind` on a shared docket now (an owner's
/// kinds, for a member who isn't one, can't be).
pub fn may(core: &Core, docket: &str, kind: &str) -> bool {
    let Some(t) = &core.team else { return true };
    if !t.shares(docket) {
        return true;
    }
    !matches!(kind, "intent" | "set") || t.people.get(docket).is_some_and(|p| p.owners.contains(&t.config.handle))
}

// ---- talking to the workspace hub ----

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(45)).connect_timeout(Duration::from_secs(10)).build().expect("an HTTP client")
}

/// A signed request to this hub's workspace.
async fn call(shared: &Shared, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let bytes = body.map(|b| serde_json::to_vec(b).expect("json")).unwrap_or_default();
    let (url, headers) = {
        let mut core = shared.lock();
        let t = core.team.as_ref().ok_or_else(|| anyhow!("this hub isn't in a workspace: wh team join"))?;
        let (url, wid) = (t.config.url.clone(), t.config.id.clone());
        let key = core.keys.get_or_create("owner")?.0;
        (format!("{}{path}", url.trim_end_matches('/')), super::auth::sign(key, &wid, method, path, &bytes, now_ms()))
    };
    let mut req = client().request(method.parse()?, &url);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    if body.is_some() {
        req = req.header("content-type", "application/json").body(bytes);
    }
    answer(req.send().await.with_context(|| format!("reaching the workspace at {url}"))?).await
}

/// A response's body, read only up to `cap` bytes: a workspace hub can't
/// make this hub hold more than that in memory.
async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Result<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n as usize > cap) {
        bail!("the workspace sent more than {} MB", cap >> 20);
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        out.extend_from_slice(&chunk);
        ensure!(out.len() <= cap, "the workspace sent more than {} MB", cap >> 20);
    }
    Ok(out)
}

/// The most a JSON answer from the workspace may be: a page is at most 4 MB.
const MAX_ANSWER: usize = 16 << 20;

async fn answer(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    let bytes = read_capped(resp, MAX_ANSWER).await?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    if !status.is_success() {
        let msg = v["error"].as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
        return Err(Refused { status: status.as_u16(), msg }.into());
    }
    Ok(v)
}

/// The workspace hub said no (as opposed to not answering).
#[derive(Debug)]
pub struct Refused {
    pub status: u16,
    pub msg: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the workspace refused it: {}", self.msg)
    }
}

impl std::error::Error for Refused {}

/// An unsigned request, for before this hub is a member.
async fn call_open(url: &str, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let full = format!("{}{path}", url.trim_end_matches('/'));
    let mut req = client().request(method.parse()?, &full);
    if let Some(b) = body {
        req = req.json(b);
    }
    answer(req.send().await.with_context(|| format!("reaching the workspace at {full}"))?).await
}

async fn put_blob(shared: &Shared, digest_hex: &str) -> Result<()> {
    let (url, headers, bytes) = {
        let mut core = shared.lock();
        let bytes = core.store.blob(digest_hex)?.ok_or_else(|| anyhow!("this hub doesn't have the file {digest_hex}"))?;
        let t = core.team.as_ref().ok_or_else(|| anyhow!("not in a workspace"))?;
        let (url, wid) = (t.config.url.clone(), t.config.id.clone());
        let path = format!("/team/v1/blobs/{digest_hex}");
        let key = core.keys.get_or_create("owner")?.0;
        (format!("{}{path}", url.trim_end_matches('/')), super::auth::sign(key, &wid, "PUT", &path, &bytes, now_ms()), bytes)
    };
    let mut req = client().put(&url).body(bytes);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    answer(req.send().await?).await.map(|_| ())
}

async fn get_blob(shared: &Shared, digest_hex: &str) -> Result<()> {
    let (url, headers) = {
        let mut core = shared.lock();
        let t = core.team.as_ref().ok_or_else(|| anyhow!("not in a workspace"))?;
        let (url, wid) = (t.config.url.clone(), t.config.id.clone());
        let path = format!("/team/v1/blobs/{digest_hex}");
        let key = core.keys.get_or_create("owner")?.0;
        (format!("{}{path}", url.trim_end_matches('/')), super::auth::sign(key, &wid, "GET", &path, b"", now_ms()))
    };
    let mut req = client().get(&url);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req.send().await?;
    ensure!(resp.status().is_success(), "the workspace doesn't have the file {digest_hex}");
    let bytes = read_capped(resp, crate::artifacts::MAX).await?;
    ensure!(hex::encode(&sha2::Sha256::digest(&bytes)) == digest_hex, "the workspace's file {digest_hex} doesn't match its hash");
    shared.lock().store.put_blob(digest_hex, &bytes, &crate::time::now_ms())
}

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

// ---- following ----

/// Send the outbox and follow the workspace, for as long as the hub runs.
pub async fn follow(shared: Shared) {
    let mut backoff = 1u64;
    loop {
        let wake = shared.lock().team.as_ref().map(|t| t.wake.clone());
        let Some(wake) = wake else {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        let pass = async {
            send(&shared).await?;
            pull(&shared, 25).await
        };
        let r = tokio::select! {
            r = pass => r,
            // Something new to send: start again, without waiting out the poll.
            _ = wake.notified() => Ok(()),
        };
        let pause = {
            let mut core = shared.lock();
            let Some(t) = core.team.as_mut() else { continue };
            match r {
                Ok(()) => {
                    if !t.online {
                        say!("workspace {}: connected", t.config.prefix);
                    }
                    t.online = true;
                    t.last_error = None;
                    backoff = 1;
                    None
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    if t.online || t.last_error.as_deref() != Some(&msg) {
                        say!("workspace {}: {msg}", t.config.prefix);
                    }
                    t.online = false;
                    t.last_error = Some(msg);
                    let wait = backoff;
                    backoff = (backoff * 2).min(30);
                    Some(wait)
                }
            }
        };
        if let Some(secs) = pause {
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
        let _ = shared.lock().events.send(json!({ "type": "team" }));
    }
}

/// Send what's queued in the outbox, in order. One the workspace refuses
/// is marked and kept for you to see; the rest go on.
pub async fn send(shared: &Shared) -> Result<()> {
    let busy = shared.lock().team.as_ref().map(|t| t.busy.clone());
    let Some(busy) = busy else { return Ok(()) };
    let _one = busy.lock().await;
    loop {
        let (next, log) = {
            let core = shared.lock();
            let Some(t) = &core.team else { return Ok(()) };
            (core.store.outbox()?.into_iter().find(|o| o.status == "queued"), t.log_id())
        };
        let Some(o) = next else { return Ok(()) };
        let (e, _) = open_envelope(&o.envelope)?;
        let mut missing = None;
        for b in &e.blobs {
            let h = b.strip_prefix("sha256:").unwrap_or(b);
            if !shared.lock().store.has_blob(h)? {
                missing = Some(h.to_string());
                break;
            }
            put_blob(shared, h).await?;
        }
        if let Some(h) = missing {
            // Never sendable: set aside, so the rest of the outbox goes on.
            shared.lock().store.outbox_set(o.n, "refused", Some(&format!("it names a file this hub doesn't have ({})", &h[..h.len().min(12)])))?;
            continue;
        }
        let path = if o.docket == log { "/team/v1/log".to_string() } else { format!("/team/v1/dockets/{}", o.docket) };
        match call(shared, "POST", &path, Some(&json!({ "envelope": o.envelope }))).await {
            Ok(v) => {
                let mut core = shared.lock();
                core.store.outbox_remove(o.n)?;
                // Take it in now when it's the next one; else `pull` will.
                if let Ok(s) = serde_json::from_value::<Stored>(v["stored"].clone()) {
                    let next = core.store.entries_len(&s.docket)? + 1;
                    if s.seq == next
                        && let Err(err) = admit(&mut core, &s)
                    {
                        say!("{}: {err:#}", s.docket);
                    }
                }
            }
            // The workspace refused the entry itself; a refusal of who's
            // asking (401, 403) or a slow-down (429) is tried again later.
            Err(err) if err.downcast_ref::<Refused>().is_some_and(|r| (400..500).contains(&r.status) && !matches!(r.status, 401 | 403 | 408 | 429)) => {
                let msg = err.downcast_ref::<Refused>().map(|r| r.msg.clone()).unwrap_or_default();
                say!("{}: the workspace refused an entry: {msg}", o.docket);
                shared.lock().store.outbox_set(o.n, "refused", Some(&msg))?;
            }
            Err(err) => return Err(err),
        }
    }
}

/// Take in what's new on the workspace: the member log first, then each
/// shared docket. `wait` is how long the workspace may hold the question
/// open for something new.
pub async fn pull(shared: &Shared, wait: u64) -> Result<()> {
    let cursor = shared.lock().team.as_ref().map_or(0, |t| t.cursor);
    // The long wait comes first, outside the turn: a `wh team sync` meanwhile
    // goes ahead, and this pull takes in only what's still new after it.
    let ch = call(shared, "GET", &format!("/team/v1/changes?cursor={cursor}&wait={wait}"), None).await?;
    let busy = shared.lock().team.as_ref().map(|t| t.busy.clone());
    let Some(busy) = busy else { return Ok(()) };
    let _one = busy.lock().await;
    let log = shared.lock().team.as_ref().map(Team::log_id).unwrap_or_default();
    follow_one(shared, &log, ch["log"].as_i64().unwrap_or(0)).await?;
    for (d, n) in ch["dockets"].as_object().into_iter().flatten() {
        let skip = shared.lock().team.as_ref().is_none_or(|t| t.halted.contains_key(d) || !t.shares(d));
        if skip {
            continue;
        }
        if let Err(e) = follow_one(shared, d, n.as_i64().unwrap_or(0)).await {
            if e.downcast_ref::<Refused>().is_some() || e.downcast_ref::<reqwest::Error>().is_some() {
                return Err(e);
            }
            halt(shared, d, &format!("{e:#}"));
        }
    }
    if let Some(t) = shared.lock().team.as_mut() {
        t.cursor = ch["cursor"].as_u64().unwrap_or(0);
    }
    Ok(())
}

fn halt(shared: &Shared, docket: &str, why: &str) {
    let mut core = shared.lock();
    say!("{docket}: stopped following it: {why}");
    if let Some(t) = core.team.as_mut() {
        t.halted.insert(docket.to_string(), why.to_string());
    }
    let _ = save(&mut core);
}

/// Bring one docket (or the member log) up to `remote` entries.
async fn follow_one(shared: &Shared, docket: &str, remote: i64) -> Result<()> {
    loop {
        let local = shared.lock().store.entries_len(docket)?;
        if local >= remote {
            ensure!(local == remote || remote == 0, "{docket}: this hub has {local} entries and the workspace says it has {remote}: entries were dropped");
            return Ok(());
        }
        let log = shared.lock().team.as_ref().map(Team::log_id).unwrap_or_default();
        let path = if docket == log { format!("/team/v1/log?after={local}") } else { format!("/team/v1/dockets/{docket}?after={local}") };
        let page = call(shared, "GET", &path, None).await?;
        let entries: Vec<Stored> = serde_json::from_value(page["entries"].clone()).context("the workspace's entries don't read")?;
        ensure!(!entries.is_empty(), "{docket}: the workspace said there's more and sent nothing");
        // Only what was there when it said how many: a later entry may be
        // signed by a key whose `key` entry this pull hasn't taken in yet.
        let entries: Vec<Stored> = entries.into_iter().filter(|s| s.seq <= remote).collect();
        ensure!(entries.iter().any(|s| s.seq > local), "{docket}: the workspace sent nothing new");
        for s in entries {
            ensure!(s.docket == docket, "the workspace sent an entry of {} for {docket}", s.docket);
            let blobs = s.entry().map(|e| e.blobs).unwrap_or_default();
            for b in blobs {
                let h = b.strip_prefix("sha256:").unwrap_or(&b).to_string();
                let have = shared.lock().store.has_blob(&h)?;
                if !have {
                    get_blob(shared, &h).await?;
                }
            }
            let mut core = shared.lock();
            // One this hub has already: the same entry, or a fork.
            if s.seq <= core.store.entries_len(docket)? {
                let mine = core.store.entries_after(docket, s.seq - 1, 1)?;
                ensure!(mine.first().is_some_and(|m| m.id == s.id), "#{}: the workspace now shows another entry where this hub has one: the history forked", s.seq);
                continue;
            }
            admit(&mut core, &s)?;
        }
    }
}

/// Check one entry the workspace ordered and take it in: it follows the
/// chain this hub has, it's signed by a key the roster names, and it
/// passes the rules.
pub fn admit(core: &mut Core, s: &Stored) -> Result<()> {
    let t = core.team.as_mut().ok_or_else(|| anyhow!("not in a workspace"))?;
    let log = t.log_id();
    ensure!(id_of(&s.envelope) == s.id, "#{}: its bytes don't match its id: it was changed after it was ordered", s.seq);
    let (e, keyid) = open_envelope(&s.envelope).map_err(|e| anyhow!("#{}: {e:#}", s.seq))?;
    if s.docket == log {
        let mut r = t.roster.clone();
        r.admit(s)?;
        core.store.insert_ordered(s, &e.kind, &e.at, &keyid, |_, _| None)?;
        let t = core.team.as_mut().expect("checked");
        t.roster = r;
        return Ok(());
    }
    ensure!(t.shares(&s.docket), "{} isn't one of this workspace's dockets", s.docket);
    // An entry is on a docket once: an old one ordered again (a revoked
    // trust, a past promotion) is refused.
    ensure!(!core.store.has_entry(&s.docket, &s.id)?, "#{}: that entry is on {} already: it was sent again", s.seq, s.docket);
    let t = core.team.as_mut().expect("checked");
    let people = t.people.get(&s.docket).cloned().unwrap_or_default();
    // Signed by a key the log names, valid when it was written: a member
    // removed since still wrote what they wrote before, and nothing after.
    shared::check(&t.roster.as_of(&e.at), &people, &s.docket, s.seq, &e, &keyid, None)?;
    let d = s.docket.clone();
    core.store.insert_ordered(s, &e.kind, &e.at, &keyid, |st, was| {
        let mut x = was.unwrap_or_else(|| crate::record::DocketState::new(&d));
        x.apply(st.seq, &st.chain, &e);
        Some(x)
    })?;
    let t = core.team.as_mut().expect("checked");
    t.people.entry(s.docket.clone()).or_default().apply(&e);
    let speaker = t.roster.any_key(&keyid).map(|(m, _)| m.handle.clone()).unwrap_or_default();
    let _ = core.events.send(json!({ "type": "docket", "docket": s.docket }));
    // What it means for this hub (a message, a question, an answer) is
    // worked out once it's safely in; a problem there doesn't undo it.
    if let Err(err) = on_admitted(core, s, &e, &speaker) {
        say!("{} #{}: {err:#}", s.docket, s.seq);
    }
    Ok(())
}

// ---- joining and sharing ----

/// What `wh team create` sends: a new workspace, made by this hub's owner.
pub async fn create(shared: &Shared, url: &str, setup: &str, name: &str, prefix: &str, handle: &str, login: Option<String>) -> Result<Value> {
    ensure!(shared.lock().team.is_none(), "this hub is in a workspace already");
    ensure!(roster::handle_ok(handle), "{handle} isn't a handle: lowercase letters, digits, - and _, starting with a letter");
    ensure!(roster::prefix_ok(prefix), "{prefix} isn't a prefix: 2 to 10 capital letters and digits, starting with a letter");
    let hello = call_open(url, "GET", "/team/v1/hello", None).await?;
    ensure!(hello["workspace"] == false, "{url} has a workspace already ({})", hello["name"].as_str().unwrap_or("?"));
    let wid = hex::encode(&crate::record::keys::random32()[..16]);
    let log = format!("{prefix}-0");
    let entries = {
        let mut core = shared.lock();
        let login = login.or_else(|| core.tailnet_owner.clone());
        let owner = core.keys.get_or_create("owner")?.0.keyid();
        let mut e = Entry::new(&log, "workspace", &crate::time::now_ms(), Actor::new("person", handle));
        e.data = json!({ "id": wid, "name": name, "prefix": prefix, "handle": handle, "key": owner, "login": login });
        let mut out = vec![sign(&mut core, "owner", &e)?.0];
        out.extend(key_entries(&mut core, &log, &wid, handle)?);
        out
    };
    let v = call_open(url, "POST", "/team/v1/create", Some(&json!({ "setup": setup, "entries": entries }))).await?;
    become_member(shared, Config { url: url.to_string(), id: wid, name: name.to_string(), prefix: prefix.to_string(), handle: handle.to_string() }).await?;
    Ok(v)
}

/// `key` entries for every key this hub has besides its owner's.
fn key_entries(core: &mut Core, log: &str, workspace: &str, handle: &str) -> Result<Vec<String>> {
    core.keys.get_or_create("hub")?;
    let names: Vec<String> = core.keys.iter().filter(|k| k.name != "owner").map(|k| k.name.clone()).collect();
    let mut out = Vec::new();
    for name in names {
        let e = key_entry(core, log, workspace, handle, &name)?;
        out.push(sign(core, "owner", &e)?.0);
    }
    Ok(out)
}

/// Save the config, take in the member log, and start following.
async fn become_member(shared: &Shared, config: Config) -> Result<()> {
    {
        let mut core = shared.lock();
        let t = team_of(&core, config.clone())?;
        core.team = Some(t);
    }
    // Nothing's kept until the member log checks and has this hub's member
    // in it: a workspace that answers wrongly leaves this hub as it was.
    let checked = async {
        pull(shared, 0).await?;
        let core = shared.lock();
        let t = core.team.as_ref().ok_or_else(|| anyhow!("not in a workspace"))?;
        ensure!(t.roster.id == config.id, "the workspace's member log is for another workspace");
        ensure!(t.roster.prefix == config.prefix, "the workspace's member log has another prefix");
        ensure!(t.roster.members.get(&config.handle).is_some_and(|m| m.active()), "the member log doesn't have {} in it", config.handle);
        Ok(())
    }
    .await;
    let mut core = shared.lock();
    match checked {
        Ok(()) => save(&mut core),
        Err(e) => {
            core.team = None;
            Err(e)
        }
    }
}

/// An invite code: everything `wh team join` needs, in one paste.
#[derive(Serialize, Deserialize)]
struct Code {
    /// The workspace hub's address.
    u: String,
    /// The workspace's id.
    w: String,
    /// The invite entry's id.
    i: String,
    /// The handle it's for.
    h: String,
    /// The invite key's seed, hex: never sent to anyone.
    s: String,
}

/// `wh team invite`: an invite for `handle`, as a code to hand them.
pub async fn invite(shared: &Shared, handle: &str, role: &str, hours: i64) -> Result<Value> {
    ensure!(roster::handle_ok(handle), "{handle} isn't a handle");
    ensure!((1..=24 * 14).contains(&hours), "an invite lasts 1 hour to 14 days");
    let seed = crate::record::keys::random32();
    let ik = Key::from_seed("invite", &seed)?;
    let (envelope, url, wid) = {
        let mut core = shared.lock();
        let t = core.team.as_ref().ok_or_else(|| anyhow!("this hub isn't in a workspace"))?;
        let (log, me, url, wid) = (t.log_id(), t.config.handle.clone(), t.config.url.clone(), t.config.id.clone());
        let expires = crate::time::from_unix_ms(now_ms() + hours * 3_600_000).ok_or_else(|| anyhow!("a time out of range"))?;
        let mut e = Entry::new(&log, "invite", &crate::time::now_ms(), Actor::new("person", &me));
        e.data = json!({ "handle": handle, "role": role, "pub": ik.keyid(), "expires": expires });
        // Checked here first, so a refusal says why in plain words.
        let keyid = core.keys.get_or_create("owner")?.0.keyid();
        core.team.as_ref().expect("checked").roster.check(&e, &keyid, "")?;
        (sign(&mut core, "owner", &e)?.0, url, wid)
    };
    let v = call(shared, "POST", "/team/v1/log", Some(&json!({ "envelope": envelope }))).await?;
    let stored: Stored = serde_json::from_value(v["stored"].clone())?;
    {
        let mut core = shared.lock();
        if stored.seq == core.store.entries_len(&stored.docket)? + 1 {
            admit(&mut core, &stored)?;
        }
    }
    let code = Code { u: url, w: wid, i: stored.id.clone(), h: handle.to_string(), s: hex::encode(&seed) };
    Ok(json!({ "code": format!("whi1.{}", B64URL.encode(serde_json::to_vec(&code)?)), "handle": handle, "role": role, "hours": hours }))
}

/// `wh team join CODE`.
pub async fn join(shared: &Shared, code: &str, login: Option<String>, name: Option<String>) -> Result<Value> {
    ensure!(shared.lock().team.is_none(), "this hub is in a workspace already");
    let raw = code.trim().strip_prefix("whi1.").ok_or_else(|| anyhow!("that isn't an invite code (they start whi1.)"))?;
    let c: Code =
        serde_json::from_slice(&B64URL.decode(raw).map_err(|_| anyhow!("that invite code is cut off or changed"))?).map_err(|_| anyhow!("that invite code doesn't read"))?;
    let seed = hex::decode(&c.s).filter(|s| s.len() == 32).ok_or_else(|| anyhow!("that invite code's secret doesn't read"))?;
    let ik = Key::from_seed("invite", &seed)?;
    let hello = call_open(&c.u, "GET", "/team/v1/hello", None).await?;
    ensure!(hello["id"].as_str() == Some(c.w.as_str()), "{} is another workspace than the invite's", c.u);
    let prefix = hello["prefix"].as_str().ok_or_else(|| anyhow!("the workspace didn't say its prefix"))?.to_string();
    ensure!(roster::prefix_ok(&prefix), "the workspace's prefix {prefix:?} isn't one");
    let log = format!("{prefix}-0");
    let entries = {
        let mut core = shared.lock();
        let login = login.or_else(|| core.tailnet_owner.clone());
        let me = core.keys.get_or_create("owner")?.0.keyid();
        let proof = B64.encode(ik.sign(&roster::join_message(&c.w, &c.i, &me)));
        let mut e = Entry::new(&log, "join", &crate::time::now_ms(), Actor::new("person", &c.h));
        e.data = json!({ "invite": c.i, "handle": c.h, "key": me, "login": login, "name": name.unwrap_or_else(|| c.h.clone()), "proof": proof });
        let mut out = vec![sign(&mut core, "owner", &e)?.0];
        out.extend(key_entries(&mut core, &log, &c.w, &c.h)?);
        out
    };
    let v = call_open(&c.u, "POST", "/team/v1/join", Some(&json!({ "entries": entries }))).await?;
    let name = v["name"].as_str().unwrap_or("").to_string();
    become_member(shared, Config { url: c.u.clone(), id: c.w.clone(), name: name.clone(), prefix: prefix.clone(), handle: c.h.clone() }).await?;
    Ok(json!({ "workspace": name, "prefix": prefix, "handle": c.h }))
}

/// `wh team share D-12`: put a local docket on the workspace, its history
/// with it. The local docket ends at the share and stays an alias.
pub async fn share(shared: &Shared, local: &str) -> Result<Value> {
    let st = {
        let core = shared.lock();
        let t = core.team.as_ref().ok_or_else(|| anyhow!("this hub isn't in a workspace: wh team join"))?;
        ensure!(!t.owns_id(local), "{local} is shared already");
        ensure!(!local.contains('-') || local.starts_with("D-"), "{local} isn't one of this hub's dockets");
        let st = core.store.state(local)?.ok_or_else(|| anyhow!("no docket {local}"))?;
        if let Some(to) = &st.shared_as {
            bail!("{local} was shared as {to}");
        }
        ensure!(!matches!(st.protocol.as_str(), "review" | "build"), "{local} is {}'s; sharing protocol dockets comes later", st.protocol);
        st
    };
    // The local history stays here: it has your approvals and prompts in it,
    // unredacted. The share names it by its head, so you can show it later.
    let id = open_shared(shared, json!({ "local": local, "head": st.head, "entries": st.entries, "title": st.title, "status": st.status, "protocol": st.protocol, "intent": st.intent.as_ref().map(|i| i.body.clone()) }), vec![]).await?;
    {
        let mut core = shared.lock();
        core.append(local, "set", &Who::Owner, None, json!({ "shared_as": id }))?;
        if let Some(t) = core.team.as_mut() {
            t.aliases.insert(local.to_string(), id.clone());
        }
        save(&mut core)?;
        // This hub's agents on it go with it.
        let moving: Vec<String> = core.agents.by_block.iter().filter(|(_, t)| t.docket == local).map(|(b, _)| b.clone()).collect();
        for b in moving {
            if let Err(e) = crate::agents::join(&mut core, &b, &id, None) {
                say!("{id}: {b} didn't move over: {e:#}");
            }
        }
    }
    Ok(json!({ "docket": id, "from": local }))
}

/// `wh new --team`: a docket opened straight on the workspace.
pub async fn new_shared(shared: &Shared, title: &str) -> Result<Value> {
    ensure!(!title.trim().is_empty(), "a docket needs a title");
    let id = open_shared(shared, json!({ "title": title.trim(), "status": "todo", "protocol": "none" }), vec![]).await?;
    Ok(json!({ "docket": id }))
}

async fn open_shared(shared: &Shared, mut data: Value, blobs: Vec<String>) -> Result<String> {
    let v = call(shared, "POST", "/team/v1/dockets", Some(&json!({}))).await?;
    let id = v["id"].as_str().ok_or_else(|| anyhow!("the workspace didn't give a number"))?.to_string();
    let envelope = {
        let mut core = shared.lock();
        let t = core.team.as_ref().ok_or_else(|| anyhow!("not in a workspace"))?;
        ensure!(t.shares(&id), "the workspace gave {id}, which isn't one of its numbers");
        let me = t.config.handle.clone();
        data["owners"] = json!([me]);
        let mut e = Entry::new(&id, "share", &crate::time::now_ms(), Actor::new("person", &me));
        e.data = data;
        e.blobs = blobs;
        sign(&mut core, "owner", &e)?.0
    };
    let v = call(shared, "POST", &format!("/team/v1/dockets/{id}"), Some(&json!({ "envelope": envelope }))).await?;
    let s: Stored = serde_json::from_value(v["stored"].clone())?;
    let mut core = shared.lock();
    admit(&mut core, &s)?;
    Ok(id)
}

/// Write a member log entry now (a role, a removal), not through the outbox.
pub async fn log_entry(shared: &Shared, kind: &str, data: Value) -> Result<Value> {
    let envelope = {
        let mut core = shared.lock();
        let t = core.team.as_ref().ok_or_else(|| anyhow!("this hub isn't in a workspace"))?;
        let mut e = Entry::new(&t.log_id(), kind, &crate::time::now_ms(), Actor::new("person", &t.config.handle));
        e.data = data;
        let keyid = core.keys.get_or_create("owner")?.0.keyid();
        core.team.as_ref().expect("checked").roster.check(&e, &keyid, "")?;
        sign(&mut core, "owner", &e)?.0
    };
    let v = call(shared, "POST", "/team/v1/log", Some(&json!({ "envelope": envelope }))).await?;
    pull(shared, 0).await?;
    Ok(v)
}

/// What `wh team` (and the apps) show.
pub fn status(core: &Core) -> Value {
    let Some(t) = &core.team else { return json!({ "workspace": null }) };
    let outbox = core.store.outbox().unwrap_or_default();
    let members: Vec<Value> = t
        .roster
        .members
        .values()
        .map(
            |m| json!({ "handle": m.handle, "name": m.name, "role": m.role, "login": m.login, "active": m.active(), "joined_at": m.joined_at, "you": m.handle == t.config.handle }),
        )
        .collect();
    let dockets: Vec<Value> = t
        .people
        .iter()
        .map(|(d, p)| {
            let st = core.store.state(d).ok().flatten();
            json!({ "id": d, "title": st.as_ref().map(|s| s.title.clone()), "status": st.as_ref().map(|s| s.status.clone()), "owners": p.owners, "members": p.members,
                "agents": st.as_ref().map(|s| s.agents_on().map(|a| json!({ "member": a.member, "app": a.agent.app, "pane": a.agent.pane, "role": a.role })).collect::<Vec<_>>()),
                "halted": t.halted.get(d) })
        })
        .collect();
    json!({
        "workspace": { "name": t.config.name, "prefix": t.config.prefix, "url": t.config.url, "id": t.config.id, "you": t.config.handle },
        "online": t.online,
        "error": t.last_error,
        "members": members,
        "dockets": dockets,
        "aliases": t.aliases,
        "outbox": outbox.iter().map(|o| json!({ "n": o.n, "docket": o.docket, "status": o.status, "why": o.why, "at": o.created_at })).collect::<Vec<_>>(),
        "halted": t.halted,
        "log": { "entries": t.roster.seq, "head": t.roster.head },
    })
}

/// Attention items: refused and held entries, and dockets not followed.
pub fn attention(core: &Core) -> Vec<Value> {
    let Some(t) = &core.team else { return vec![] };
    let mut out = Vec::new();
    for o in core.store.outbox().unwrap_or_default() {
        match o.status.as_str() {
            "held" => out.push(json!({ "docket": o.docket, "title": o.docket, "kind": "team-held", "text": format!("Held back from the workspace: {}. wh team send {} or wh team drop {}", o.why.unwrap_or_default(), o.n, o.n), "at": o.created_at })),
            "refused" => out.push(json!({ "docket": o.docket, "title": o.docket, "kind": "team-refused", "text": format!("The workspace refused an entry: {}", o.why.unwrap_or_default()), "at": o.created_at })),
            _ => {}
        }
    }
    for (d, why) in &t.halted {
        out.push(json!({ "docket": d, "title": d, "kind": "team-halted", "text": format!("Not followed: {why}"), "at": null }));
    }
    out
}

/// `wh team follow ACME-12`: try a docket this hub stopped following again,
/// from the entry it last took in. What made it stop is refused again if
/// the workspace still serves it.
pub fn follow_again(core: &mut Core, docket: &str) -> Result<()> {
    let t = core.team.as_mut().ok_or_else(|| anyhow!("this hub isn't in a workspace"))?;
    ensure!(t.halted.remove(docket).is_some(), "{docket} is followed already");
    t.wake.notify_one();
    save(core)
}

/// `wh team send N` (a held entry goes after all) or `wh team drop N`.
pub fn release_held(core: &mut Core, n: i64, send: bool) -> Result<()> {
    let row = core.store.outbox()?.into_iter().find(|o| o.n == n).ok_or_else(|| anyhow!("nothing in the outbox numbered {n}"))?;
    ensure!(matches!(row.status.as_str(), "held" | "refused"), "#{n} is {}, not held", row.status);
    if send {
        ensure!(row.status == "held", "#{n} was refused by the workspace; it can only be dropped");
        core.store.outbox_set(n, "queued", None)?;
        if let Some(t) = &core.team {
            t.wake.notify_one();
        }
    } else {
        core.store.outbox_remove(n)?;
    }
    Ok(())
}

/// Check this hub's copy of a shared docket (or the member log): ids,
/// order, chain and signatures with the roster's keys, and the rules for
/// every entry, from the start.
pub fn verify(t: &Team, docket: &str, entries: &[Stored]) -> crate::record::verify::Report {
    let mut r = crate::record::verify::verify(docket, entries, |k| t.roster.any_key(k).is_some());
    if docket == t.log_id() {
        if let Err(e) = Roster::fold(entries) {
            r.problems.push(format!("{e:#}"));
        }
    } else {
        let mut people = People::default();
        let mut seen = std::collections::BTreeSet::new();
        for s in entries {
            let Ok((e, keyid)) = open_envelope(&s.envelope) else { continue };
            if !seen.insert(s.id.clone()) {
                r.problems.push(format!("#{}: the same entry as an earlier one: it was sent again", s.seq));
            }
            // Checked with the roster as it stood when it was written: a
            // member removed since wrote this while they were in.
            if let Err(err) = shared::check(&t.roster.as_of(&e.at), &people, docket, s.seq, &e, &keyid, None) {
                r.problems.push(format!("#{}: {err:#}", s.seq));
            }
            people.apply(&e);
        }
    }
    if let Some(why) = t.halted.get(docket) {
        r.problems.push(format!("not followed: {why}"));
    }
    r.ok = r.problems.is_empty();
    r
}

// ---- messages across hubs ----

/// A pane on a teammate's Mac, as a message's block names it here:
/// `team:sam:block:sam-1` is Sam's `block:sam-1`. The block is empty for
/// a person (Sam, in a shell).
pub fn remote_block(b: &str) -> Option<(String, String)> {
    let rest = b.strip_prefix("team:")?;
    let (member, block) = rest.split_once(':')?;
    Some((member.to_string(), block.to_string()))
}

const GATES: &str = "team_gates";

fn gates(core: &Core) -> BTreeMap<String, i64> {
    core.store.meta_get(GATES).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

fn save_gates(core: &mut Core, g: &BTreeMap<String, i64>) -> Result<()> {
    core.store.meta_set(GATES, &serde_json::to_string(g)?)
}

/// What a teammate's entry means for this hub, once it's in: a message for
/// one of its agents, a question for its person, or an answer to one.
/// Nothing here goes back on the record: a gate is this hub's own ask.
fn on_admitted(core: &mut Core, s: &Stored, e: &Entry, speaker: &str) -> Result<()> {
    let Some(t) = &core.team else { return Ok(()) };
    let me = t.config.handle.clone();
    if speaker == me || !matches!(e.kind.as_str(), "note" | "question" | "decision") || e.data.get("message").is_none() {
        return Ok(());
    }
    let people = t.people.get(&s.docket).cloned().unwrap_or_default();
    let d = &e.data;
    let person = e.actor.typ == "person";
    let from_addr = if person { speaker.to_string() } else { format!("{speaker}/{}", d["from"].as_str().unwrap_or("agent")) };
    let from_block = format!("team:{speaker}:{}", crate::record::fold::block_of(e).unwrap_or(""));
    let subject = d["subject"].as_str().map(str::to_string);
    let options: Vec<String> = d["options"].as_array().into_iter().flatten().filter_map(|o| o.as_str().map(str::to_string)).collect();
    let body = e.body.clone().unwrap_or_default();

    // An answer to a question this hub has open: the first one stands. Only
    // from a person who may answer on this docket: an owner, or anyone on
    // it when the owners said members answer. Never an agent's.
    if let Some(x) = d["answers"].as_str() {
        let may = person && (people.owners.contains(speaker) || (people.answer == "members" && people.on(speaker)));
        // Or the asking agent's own person, answering on their Mac: this
        // hub's copy of their agent's question is withdrawn.
        let own_asker = |q: &crate::store::Message| person && q.from_block.as_deref().and_then(remote_block).is_some_and(|(m, _)| m == speaker);
        let open: Vec<crate::store::Message> = core
            .store
            .messages_where("entry = ?1 AND docket = ?2 AND to_addr = 'me' AND status = 'open'", rusqlite::params![x, s.docket])?
            .into_iter()
            .filter(|q| may || own_asker(q))
            .collect();
        if !open.is_empty() {
            for q in open {
                core.store.set_message_status(q.id, "answered", Some(&format!("answered by {speaker}")))?;
                // This hub's own agent asked: the answer goes to it.
                if let Some(b) = q.from_block.as_deref().filter(|b| remote_block(b).is_none()) {
                    let mut m = crate::store::Message {
                        from_addr: speaker.to_string(),
                        from_block: Some(from_block.clone()),
                        to_addr: q.from_addr.clone(),
                        to_block: Some(b.to_string()),
                        subject: q.subject.as_ref().map(|s| format!("Re: {}", s.trim_start_matches("Re: "))),
                        body: body.clone(),
                        reply_to: Some(q.id),
                        choice: d["choice"].as_str().map(str::to_string),
                        status: "queued".into(),
                        created_at: crate::time::now_ms(),
                        docket: Some(s.docket.clone()),
                        seq: Some(s.seq),
                        entry: Some(s.id.clone()),
                        ..Default::default()
                    };
                    m.id = core.store.insert_message(&m)?;
                }
            }
            let _ = core.events.send(json!({ "type": "message" }));
            return Ok(());
        }
    }

    // A message for this hub's agents on the docket.
    let targets: Vec<String> =
        d["team_to"].as_array().into_iter().flatten().filter(|r| r["member"] == me.as_str()).filter_map(|r| r["block"].as_str().map(str::to_string)).collect();
    if !targets.is_empty() {
        let trusted = people.trust.contains(&(me.clone(), speaker.to_string()));
        for b in targets {
            // Only to an agent of this hub's still on the docket.
            let here = core.agents.by_block.get(&b).is_some_and(|x| x.docket == s.docket);
            let Some(p) = core.mirror.panes.get(&b).filter(|_| here).cloned() else { continue };
            let mut m = crate::store::Message {
                from_addr: from_addr.clone(),
                from_block: Some(from_block.clone()),
                to_addr: crate::messages::addr_of(core, &p),
                to_block: Some(b.clone()),
                subject: subject.clone(),
                body: body.clone(),
                options: options.clone(),
                choice: d["choice"].as_str().map(str::to_string),
                status: if trusted { "queued".into() } else { "waiting".into() },
                created_at: crate::time::now_ms(),
                docket: Some(s.docket.clone()),
                seq: Some(s.seq),
                entry: Some(s.id.clone()),
                ..Default::default()
            };
            m.id = core.store.insert_message(&m)?;
            if !trusted {
                gate(core, &m, speaker)?;
            }
        }
        let _ = core.events.send(json!({ "type": "message" }));
        return Ok(());
    }

    // A teammate's agent asking its people: here too when this hub's person
    // is an owner (or anyone on it may answer).
    if !person && d["to"] == "me" && d.get("replies_to").is_none() {
        let answers = people.owners.contains(&me) || (people.answer == "members" && people.on(&me));
        if answers {
            let mut m = crate::store::Message {
                from_addr,
                from_block: Some(from_block),
                to_addr: crate::messages::ME.into(),
                subject,
                body,
                options,
                status: "open".into(),
                created_at: crate::time::now_ms(),
                docket: Some(s.docket.clone()),
                seq: Some(s.seq),
                entry: Some(s.id.clone()),
                ..Default::default()
            };
            m.id = core.store.insert_message(&m)?;
            let _ = core.events.send(json!({ "type": "message", "id": m.id, "to": crate::messages::ME }));
        }
    }
    Ok(())
}

/// Ask this hub's person whether a teammate's message goes in. The ask is
/// theirs alone: it's in their Inbox, never on the shared record.
fn gate(core: &mut Core, held: &crate::store::Message, speaker: &str) -> Result<()> {
    let preview: String = held.body.chars().take(600).collect();
    let mut m = crate::store::Message {
        from_addr: crate::messages::HUB.into(),
        to_addr: crate::messages::ME.into(),
        subject: Some(format!("{speaker}'s agent wants to message {}", held.to_addr)),
        body: format!("From {} on {}:\n\n{preview}", held.from_addr, held.docket.as_deref().unwrap_or("?")),
        options: vec!["Deliver".into(), "Drop".into(), format!("Let {speaker}'s agents through")],
        status: "open".into(),
        created_at: crate::time::now_ms(),
        docket: held.docket.clone(),
        ..Default::default()
    };
    m.id = core.store.insert_message(&m)?;
    let mut g = gates(core);
    g.insert(m.id.to_string(), held.id);
    save_gates(core, &g)?;
    let _ = core.events.send(json!({ "type": "message", "id": m.id, "to": crate::messages::ME }));
    Ok(())
}

/// Your answer to a gate: the held message goes in, is dropped, or goes in
/// with every later one from that teammate's agents on that docket.
pub fn gate_answered(core: &mut Core, ask: i64, choice: Option<&str>) -> Result<()> {
    let mut g = gates(core);
    let Some(held) = g.get(&ask.to_string()).copied() else { return Ok(()) };
    let Some(choice) = choice else { return Ok(()) };
    let m = core.store.message(held)?.ok_or_else(|| anyhow!("the held message #{held} is gone"))?;
    let speaker = m.from_block.as_deref().and_then(remote_block).map(|(s, _)| s).unwrap_or_default();
    match choice {
        "Deliver" => core.store.set_message_status(held, "queued", None)?,
        "Drop" => core.store.set_message_status(held, "dropped", Some("you dropped it"))?,
        c if c.starts_with("Let ") => {
            let d = m.docket.clone().ok_or_else(|| anyhow!("the held message has no docket"))?;
            core.append(&d, "trust", &Who::Owner, None, json!({ "teammate": speaker, "on": true }))?;
            if let Some(t) = core.team.as_mut() {
                let me = t.config.handle.clone();
                t.people.entry(d.clone()).or_default().trust.insert((me, speaker.clone()));
            }
            // Everything else of theirs waiting on this docket goes in too.
            let pending: Vec<i64> = core
                .store
                .messages_where("status = 'waiting' AND docket = ?1", rusqlite::params![d])?
                .into_iter()
                .filter(|w| w.from_block.as_deref().and_then(remote_block).is_some_and(|(s, _)| s == speaker))
                .map(|w| w.id)
                .collect();
            for id in pending {
                core.store.set_message_status(id, "queued", None)?;
            }
            for (a, h) in g.clone() {
                if core.store.message(h)?.is_some_and(|x| x.status == "queued") {
                    g.remove(&a);
                    let _ = core.store.set_message_status(a.parse().unwrap_or(0), "answered", None);
                }
            }
        }
        _ => return Ok(()),
    }
    g.remove(&ask.to_string());
    save_gates(core, &g)?;
    core.live_changed();
    Ok(())
}
