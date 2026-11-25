//! The hub's HTTP API and the web app, on localhost (TCP) and on a unix
//! socket for the CLI and the agents' hooks.
//!
//! - `GET /api/state`: dockets, what needs you, agents.
//! - `GET /api/dockets/{id}`: one docket, its entries, and its check.
//! - `POST /api/dockets`, `/api/dockets/{id}/note`, `/set`, `/open`.
//! - `GET /api/dockets/{id}/export`: JSON lines, checkable without the hub.
//! - `POST /api/hook`: from `wh hook`.
//! - `GET /api/events`: server-sent events when anything changes.

use anyhow::{Context, Result};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::agents;
use crate::hub::{Shared, Who};
use crate::record::{self, DocketState, mark};

static WEB: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web/dist");

pub fn http_addr() -> Option<std::net::SocketAddr> {
    let v = std::env::var("WH_HTTP").unwrap_or_else(|_| "127.0.0.1:7500".into());
    if v == "off" { None } else { v.parse().ok() }
}

pub fn router(shared: Shared) -> Router {
    Router::new()
        .route("/api/state", get(state))
        .route("/api/status", get(status))
        .route("/api/dockets", post(new_docket))
        .route("/api/dockets/{id}", get(docket))
        .route("/api/dockets/{id}/subagents", get(docket_subagents))
        .route("/api/dockets/{id}/note", post(note))
        .route("/api/dockets/{id}/set", post(set))
        .route("/api/dockets/{id}/open", post(open_in_rex))
        .route("/api/dockets/{id}/hold", post(hold))
        .route("/api/dockets/{id}/standing", post(standing))
        .route("/api/dockets/{id}/check", post(run_checks))
        .route("/api/dockets/{id}/screen", get(screen))
        .route("/api/dockets/{id}/diff", get(docket_diff))
        .route("/api/dockets/{id}/send", post(send))
        .route("/api/dockets/{id}/queue/{q}/cancel", post(unqueue))
        .route("/api/dockets/{id}/approve", post(approve))
        .route("/api/dockets/{id}/deny", post(deny))
        .route("/api/dockets/{id}/export", get(export))
        .route("/api/dockets/{id}/record", get(record_view))
        .route("/api/dockets/{id}/record/{seq}", get(record_entry))
        .route("/api/dockets/{id}/verify", get(verify_one))
        .route("/api/dockets/{id}/artifacts", get(artifacts_list).post(artifacts_attach).layer(axum::extract::DefaultBodyLimit::max(crate::artifacts::MAX * 4 / 3 + (1 << 20))))
        .route("/api/blobs/{digest}", get(blob))
        .route("/api/verify", get(verify_all))
        .route("/api/dockets/{id}/intent", post(set_intent))
        .route("/api/dockets/{id}/linear", post(link_linear))
        .route("/api/dockets/{id}/brief", get(brief_of))
        .route("/api/dockets/from-linear", post(from_linear))
        .route("/api/linear/search", get(linear_search))
        .route("/api/brief", get(brief_of_pane))
        .route("/api/messages", get(messages_list).post(messages_send))
        .route("/api/messages/wait", get(messages_wait))
        .route("/api/messages/{n}", get(messages_read))
        .route("/api/messages/{n}/reply", post(messages_reply))
        .route("/api/messages/{n}/done", post(messages_done))
        .route("/api/panes/name", post(pane_name))
        .route("/api/whoami", get(whoami))
        .route("/api/join", post(join_docket))
        .route("/api/leave", post(leave_docket))
        .route("/api/agents/start", post(start_agent))
        .route("/api/folders", get(folders_list))
        .route("/api/hub/stop", post(hub_stop))
        .route("/api/push/key", get(push_key))
        .route("/api/push/subscribe", post(push_subscribe))
        .route("/api/push/unsubscribe", post(push_unsubscribe))
        .route("/api/push/test", post(push_test))
        .route("/api/recap", get(recap))
        .route("/api/build", get(build_status))
        .route("/api/build/start", post(build_start))
        .route("/api/build/settings", post(build_settings))
        .route("/api/build/discard", post(build_discard))
        .route("/api/review", get(review_status))
        .route("/api/review/check", post(review_check))
        .route("/api/import/rex-reviewer", post(import_rex_reviewer))
        .route("/api/review/settings", post(review_settings))
        .route("/api/settings", get(settings_view).post(settings_change))
        .route("/api/settings/doc", get(settings_doc).post(settings_doc_save))
        .route("/api/settings/doc/reset", post(settings_doc_reset))
        .route("/api/review/pickup", post(review_pickup))
        .route("/api/review/preview", post(review_native_preview))
        .route("/api/review/post", post(review_native_post))
        .route("/api/review/dismiss", post(review_native_dismiss))
        .route("/api/share", post(share_start))
        .route("/api/share/stop", post(share_stop))
        .route("/api/hook", post(hook))
        .route("/api/events", get(events))
        .merge(crate::team::api::routes())
        .fallback(asset)
        .layer(middleware::from_fn(guard))
        .with_state(shared)
}

pub async fn serve(shared: Shared) -> Result<()> {
    let sock = crate::paths::socket();
    let _ = std::fs::remove_file(&sock);
    let unix = tokio::net::UnixListener::bind(&sock).with_context(|| format!("binding {}", sock.display()))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600));
    }
    let app = router(shared.clone());
    let tailnet_app = router(shared.clone()).layer(middleware::from_fn_with_state(shared.clone(), owner_only));
    let on_unix = async { axum::serve(unix, app.clone()).await.context("socket") };
    let on_http = async {
        let Some(addr) = http_addr() else { return std::future::pending::<Result<()>>().await };
        let tcp = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
        say!("Wheelhouse on http://{addr}");
        axum::serve(tcp, app.clone()).await.context("http")
    };
    let on_tailnet = async {
        let Some(addr) = crate::tailnet::listen_addr() else { return std::future::pending::<Result<()>>().await };
        let tcp = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
        say!("tailnet listener on {addr} (wh web publishes it)");
        axum::serve(tcp, tailnet_app).await.context("tailnet")
    };
    tokio::try_join!(on_unix, on_http, on_tailnet)?;
    Ok(())
}

/// The tailnet listener answers only the Mac's own Tailscale user, as
/// `tailscale serve` reports them. Nothing else is trusted there.
async fn owner_only(State(shared): State<Shared>, mut req: Request, next: Next) -> Response {
    let login = req.headers().get("tailscale-user-login").and_then(|v| v.to_str().ok()).map(str::to_string);
    let owner = shared.lock().tailnet_owner.clone();
    match (login, owner) {
        (Some(l), Some(o)) if l.eq_ignore_ascii_case(&o) => {
            req.extensions_mut().insert(ViaTailnet);
            next.run(req).await
        }
        (_, None) => (StatusCode::SERVICE_UNAVAILABLE, "Wheelhouse doesn't know its owner yet: is Tailscale running on the Mac?").into_response(),
        _ => (StatusCode::FORBIDDEN, "Wheelhouse answers only this Mac's Tailscale user").into_response(),
    }
}

// ---- the guard ----

/// Our own host names only (DNS rebinding), and no cross-site writes.
async fn guard(req: Request, next: Next) -> Response {
    let via_tailnet = req.extensions().get::<ViaTailnet>().is_some();
    let h = req.headers();
    if !host_ok(h, via_tailnet) {
        return (StatusCode::MISDIRECTED_REQUEST, "unknown host").into_response();
    }
    if req.method() != Method::GET && req.method() != Method::HEAD && !same_site(h) {
        return (StatusCode::FORBIDDEN, "cross-site request refused").into_response();
    }
    next.run(req).await
}

/// Set on requests that came through the tailnet listener, after the owner check.
#[derive(Clone, Copy)]
struct ViaTailnet;

/// Loopback names always; a tailnet name only through the tailnet listener,
/// which has already checked the caller is the owner (`owner_only`).
fn host_ok(h: &HeaderMap, via_tailnet: bool) -> bool {
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let name = match host.rsplit_once(':') {
        Some((n, port)) if port.chars().all(|c| c.is_ascii_digit()) => n,
        _ => host,
    };
    matches!(name, "127.0.0.1" | "localhost" | "[::1]") || (via_tailnet && name.ends_with(".ts.net"))
}

/// A write must carry our header (which forces a CORS preflight that's never
/// answered), and its Origin, when sent, must be this host.
fn same_site(h: &HeaderMap) -> bool {
    if h.get("x-wheelhouse").is_none() {
        return false;
    }
    let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true };
    let host = h.get("x-forwarded-host").or_else(|| h.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("");
    let origin_host = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    origin_host.eq_ignore_ascii_case(host)
}

// ---- handlers ----

type ApiResult = std::result::Result<Json<Value>, (StatusCode, Json<Value>)>;

fn fail(status: StatusCode, msg: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": msg.to_string() })))
}

fn internal(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
}

/// A docket as the apps show it: its state, plus what its agents are doing
/// now. `live` is its lead's (`agents::lead`); each of `agents` has its own.
fn view(core: &crate::hub::Core, st: &DocketState) -> Value {
    let lead = agents::lead(core, st);
    let live = lead.as_deref().and_then(|b| agents::live(core, b));
    let quick = !st.is_open()
        && st.protocol == "agent"
        && st.prompts <= 1
        && st.ended_at.as_deref().and_then(|e| {
            let start = crate::time::parse(&st.created_at)?;
            let end = crate::time::parse(e)?;
            Some((end - start).whole_seconds() < 300)
        }) == Some(true);
    let mut v = serde_json::to_value(st).unwrap_or_default();
    // The list needs the start of a prompt or turn; the docket's own page
    // reads the whole of it from the record.
    for k in ["last_prompt", "last_turn"] {
        if let Some(t) = v[k].as_str()
            && t.chars().count() > 600
        {
            v[k] = json!(format!("{}…", t.chars().take(600).collect::<String>()));
        }
    }
    v["live"] = json!(live);
    v["lead"] = json!(lead);
    v["agents"] = st
        .agents
        .iter()
        .map(|a| {
            let mut x = serde_json::to_value(a).unwrap_or_default();
            let here = a.on() && agents::docket_of(core, &a.agent.block).as_deref() == Some(st.id.as_str());
            x["on"] = json!(a.on());
            x["runtime"] = json!(crate::runtime::of(&a.agent.block));
            x["live"] = json!(if here { agents::live(core, &a.agent.block) } else { None });
            x["addr"] = json!(core.mirror.panes.get(&a.agent.block).filter(|_| here).map(|p| crate::messages::addr_of(core, p)));
            x
        })
        .collect();
    v["quick"] = json!(quick);
    v["short_head"] = json!(st.short_head());
    v
}

async fn state(State(shared): State<Shared>) -> ApiResult {
    let core = shared.lock();
    let states = core.store.states().map_err(internal)?;
    // Every client fetches this on every change: lists need an intent's
    // start, not its body, and no Linear description (a docket's own page
    // has them whole).
    let dockets: Vec<Value> = states
        .iter()
        .map(|st| {
            let mut v = view(&core, st);
            if let Some(b) = v["intent"]["body"].as_str()
                && b.chars().count() > 280
            {
                v["intent"]["body"] = json!(format!("{}…", b.chars().take(280).collect::<String>()));
            }
            if v["linear"].is_object() {
                v["linear"]["description"] = Value::Null;
            }
            // Lists show the agents on a docket now; its page, every one.
            if let Some(a) = v["agents"].as_array_mut() {
                a.retain(|x| x["on"] == true);
            }
            v
        })
        .collect();

    // What needs you: a blocked or failed agent. One that finished its turn
    // shows as finished on its docket's row, not here.
    let mut asks = Vec::new();
    for (block, t) in &core.agents.by_block {
        let Some(l) = agents::live(&core, block) else { continue };
        let what = match l.state.as_str() {
            "blocked" => "blocked",
            "error" => "failed",
            _ => continue,
        };
        let st = states.iter().find(|s| s.id == t.docket);
        asks.push(json!({
            "docket": t.docket,
            "block": block,
            "title": st.map(|s| s.title.clone()),
            "what": what,
            "kind": l.kind,
            "msg": l.msg,
            "since": l.since,
            "app": l.app,
        }));
    }
    asks.sort_by_key(|a| a["what"] != "blocked");

    let agents_now: Vec<Value> = core
        .mirror
        .panes
        .values()
        .filter(|p| p.is_agent())
        .filter_map(|p| {
            let mut v = serde_json::to_value(agents::live(&core, &p.block_id)?).ok()?;
            let docket = agents::docket_of(&core, &p.block_id);
            let role = docket.as_deref().and_then(|d| states.iter().find(|s| s.id == d)).and_then(|s| s.agents_on().find(|a| a.agent.block == p.block_id)?.role.clone());
            v["docket"] = json!(docket);
            v["role"] = json!(role);
            Some(v)
        })
        .collect();

    drop(core);
    let review = crate::review::watch::status(&shared);
    let build = crate::build::watch::status(&shared);
    let core = shared.lock();

    let messages: Vec<Value> = core.store.messages_where("to_addr = 'me' AND status = 'open'", []).map_err(internal)?.iter().map(|m| crate::messages::view(&core, m)).collect();
    let mut attention = crate::team::member::attention(&core);
    attention.extend(crate::evidence::attention(&core, &core.store.states().map_err(internal)?));
    Ok(Json(json!({
        "review": review,
        "build": build,
        "messages": messages,
        "owner": core.owner,
        "flavor": flavor(),
        "hooks": crate::hooks::any_installed(),
        "linear": core.linear_ready,
        "rex": crate::runtime::link_json(&core.mirror, crate::runtime::Kind::Rex),
        "runtimes": crate::runtime::status(&core.mirror),
        "dockets": dockets,
        "asks": asks,
        "agents": agents_now,
        "attention": attention,
        "team": crate::team::member::status(&core),
    })))
}

/// Which hub this is: null for the daily one; for a second one (`WH_FLAVOR`,
/// Wheelhouse Dev's `dev`), its name and the build it runs, which its apps
/// show on a DEV badge.
pub fn flavor() -> Value {
    match std::env::var("WH_FLAVOR").ok().map(|f| f.trim().to_string()).filter(|f| !f.is_empty()) {
        Some(name) => json!({ "name": name, "commit": env!("WH_COMMIT"), "branch": env!("WH_BRANCH") }),
        None => Value::Null,
    }
}

async fn status(State(shared): State<Shared>) -> ApiResult {
    let core = shared.lock();
    let dockets = core.store.states().map_err(internal)?;
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "commit": env!("WH_COMMIT"),
        "flavor": flavor(),
        "pid": std::process::id(),
        "uptime_secs": core.started.elapsed().as_secs(),
        "rex": crate::runtime::link_json(&core.mirror, crate::runtime::Kind::Rex),
        "runtimes": crate::runtime::status(&core.mirror),
        "agents": core.mirror.panes.values().filter(|p| p.is_agent()).count(),
        "dockets": { "open": dockets.iter().filter(|d| d.is_open()).count(), "all": dockets.len() },
        "hooks": crate::hooks::any_installed(),
        "tailnet": { "owner": core.tailnet_owner, "url": core.tailnet_host.as_ref().map(|h| format!("https://{h}:{}", crate::tailnet::TAILNET_PORT)) },
        "state_dir": crate::paths::tilde(&crate::paths::state_dir().to_string_lossy()),
    })))
}

async fn docket(State(shared): State<Shared>, Path(id): Path<String>) -> ApiResult {
    let mut core = shared.lock();
    let st = core.store.state(&id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
    let keys: std::collections::BTreeMap<String, String> = core.store.keys().map_err(internal)?.into_iter().collect();
    let mut entries = Vec::new();
    let mut read = Vec::new();
    for s in core.store.entries(&id).map_err(internal)? {
        let e = s.entry().map_err(internal)?;
        read.push(e.clone());
        let keyid = s.keyid();
        entries.push(json!({
            "seq": s.seq,
            "id": s.id,
            "chain": s.chain,
            "kind": e.kind,
            "at": e.at,
            "actor": e.actor,
            "body": e.body,
            "data": e.data,
            "mark": mark(&e),
            "key": keyid.as_ref().and_then(|k| keys.get(k)),
        }));
    }
    let report = core.verify(&id).map_err(internal)?;
    let queued: Vec<Value> = core.store.queued_prompts(Some(&id)).map_err(internal)?.into_iter().map(|(q, _, _, body, at)| json!({ "id": q, "body": body, "at": at })).collect();
    let decisions: Vec<Value> = crate::decisions::of_stored(&core.store.entries(&id).map_err(internal)?).iter().map(crate::decisions::Decision::json).collect();
    let numbered: Vec<(i64, record::Entry)> = entries.iter().filter_map(|v| v["seq"].as_i64()).zip(read.iter().cloned()).collect();
    let verify_view = crate::evidence::view(&core, &st, &numbered);
    Ok(Json(json!({ "docket": view(&core, &st), "entries": entries, "verify": report, "queued": queued, "decisions": decisions, "evidence": verify_view })))
}

/// What the docket's agents spawned, read live from their own files.
async fn docket_subagents(State(shared): State<Shared>, Path(id): Path<String>) -> ApiResult {
    let entries: Vec<record::Entry> = {
        let core = shared.lock();
        core.store.state(&id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
        core.store.entries(&id).map_err(internal)?.iter().filter_map(|s| s.entry().ok()).filter(|e| matches!(e.kind.as_str(), "open" | "claim" | "release")).collect()
    };
    let claims = crate::subagents::claims(&entries);
    let v = tokio::task::spawn_blocking(move || crate::subagents::of(&claims)).await.map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok(Json(v))
}

#[derive(Deserialize)]
struct NewDocket {
    title: String,
}

async fn new_docket(State(shared): State<Shared>, Json(req): Json<NewDocket>) -> ApiResult {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "a docket needs a title"));
    }
    let mut core = shared.lock();
    let id = core.open(&Who::Owner, title, json!({ "protocol": "none", "title_source": "person" })).map_err(internal)?;
    Ok(Json(json!({ "docket": id })))
}

#[derive(Deserialize)]
struct Note {
    body: String,
    /// The pane `wh note` ran in, if any: an agent's pane makes it the agent's.
    #[serde(default)]
    block: Option<String>,
}

async fn note(State(shared): State<Shared>, Path(id): Path<String>, Json(req): Json<Note>) -> ApiResult {
    let body = req.body.trim();
    if body.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "an empty note"));
    }
    let mut core = shared.lock();
    if core.store.state(&id).map_err(internal)?.is_none() {
        return Err(fail(StatusCode::NOT_FOUND, format!("no docket {id}")));
    }
    let who = match req.block.as_deref().and_then(|b| core.mirror.panes.get(b)).filter(|p| p.is_agent()) {
        Some(p) => {
            Who::Agent { app: p.app().unwrap_or("agent").to_string(), block: p.block_id.clone(), session: core.agents.by_block.get(&p.block_id).and_then(|t| t.session.clone()) }
        }
        None => Who::Owner,
    };
    let s = core.append(&id, "note", &who, Some(body.to_string()), Value::Null).map_err(internal)?;
    Ok(Json(json!({ "seq": s.seq, "id": s.id })))
}

#[derive(Deserialize)]
struct Set {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
    /// Close it with nothing witnessed or checkable behind it: on your
    /// word, with why.
    #[serde(default)]
    anyway: bool,
    #[serde(default)]
    reason: Option<String>,
}

async fn set(State(shared): State<Shared>, Path(id): Path<String>, Json(req): Json<Set>) -> ApiResult {
    let mut data = json!({});
    if let Some(t) = req.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        data["title"] = json!(t);
        data["title_source"] = json!("person");
    }
    if let Some(s) = req.status.as_deref() {
        if !matches!(s, "todo" | "in_progress" | "done" | "dropped") {
            return Err(fail(StatusCode::BAD_REQUEST, format!("status is todo, in_progress, done or dropped, not {s}")));
        }
        data["status"] = json!(s);
    }
    if data.as_object().is_some_and(|o| o.is_empty()) {
        return Err(fail(StatusCode::BAD_REQUEST, "nothing to set"));
    }
    let mut core = shared.lock();
    let st = core.store.state(&id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
    // The rule for done: a docket with acceptance items isn't closed on what
    // was said. It needs something witnessed or checkable, or your word.
    if req.status.as_deref() == Some("done") && st.status != "done" && crate::evidence::needs(&st) {
        let entries: Vec<(i64, record::Entry)> = core.store.entries(&id).map_err(internal)?.iter().filter_map(|s| Some((s.seq, s.entry().ok()?))).collect();
        if crate::evidence::of(&st, &entries).is_empty() {
            if !req.anyway {
                return Err(fail(
                    StatusCode::CONFLICT,
                    format!(
                        "{id} has acceptance items, and nothing witnessed or checkable says they're met yet: no passing checks, no merged PR. Run its checks, or close it on your word, with a reason."
                    ),
                ));
            }
            data["on"] = json!("word");
            data["reason"] = json!(req.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).unwrap_or("closed on your word"));
        }
    }
    let s = core.append(&id, "set", &Who::Owner, req.reason.clone().filter(|_| data["on"] == "word"), data).map_err(internal)?;
    Ok(Json(json!({ "seq": s.seq })))
}

#[derive(Deserialize)]
struct Hold {
    #[serde(default)]
    reason: Option<String>,
    /// Resume it.
    #[serde(default)]
    off: bool,
}

/// Hold a docket (its protocol takes no steps), or resume it.
async fn hold(State(shared): State<Shared>, Path(id): Path<String>, Json(req): Json<Hold>) -> ApiResult {
    let mut core = shared.lock();
    let st = core.store.state(&id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
    let data = if req.off {
        if st.hold.is_none() {
            return Err(fail(StatusCode::CONFLICT, format!("{id} isn't on hold")));
        }
        json!({ "hold": null })
    } else {
        json!({ "hold": req.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).unwrap_or("held") })
    };
    let s = core.append(&id, "set", &Who::Owner, None, data).map_err(internal)?;
    core.build.check_now.notify_one();
    Ok(Json(json!({ "seq": s.seq })))
}

#[derive(Deserialize)]
struct Standing {
    /// The decision's key, as the docket's decisions name it.
    key: String,
}

/// Make a docket's decision standing for its repo: it joins the repo's
/// notes, which every later brief there includes, reviewers' too.
async fn standing(State(shared): State<Shared>, Path(id): Path<String>, Json(req): Json<Standing>) -> ApiResult {
    let (d, repo) = {
        let core = shared.lock();
        let st = core.store.state(&id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
        let d = crate::decisions::of_stored(&core.store.entries(&id).map_err(internal)?)
            .into_iter()
            .find(|d| d.key == req.key)
            .ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("{id} has no decision {}", req.key)))?;
        if let Some(r) = &d.standing {
            return Err(fail(StatusCode::CONFLICT, format!("it stands for {r} already")));
        }
        let repo = crate::build::get(&core, &id)
            .ok()
            .flatten()
            .map(|t| t.repo)
            .or_else(|| st.review.as_ref().and_then(|r| r.key.split('#').next().map(str::to_string)))
            .or_else(|| st.home_cwd().and_then(|c| core.build.repo_of(&crate::paths::expand(c))).map(|(r, _)| r));
        (d, repo)
    };
    let repo = repo.ok_or_else(|| fail(StatusCode::CONFLICT, format!("{id} isn't in a repo Wheelhouse knows, so there's no repo to make it standing for")))?;
    if d.answer.is_none() {
        return Err(fail(StatusCode::CONFLICT, "it has no answer yet"));
    }
    let rule = crate::decisions::rule(&d, &id);
    let view = settings_now(&shared);
    let mut patch = json!({});
    for part in ["build", "review"] {
        let Some(r) = view[part]["config"]["repos"].get(&repo) else { continue };
        if let Some(notes) = crate::decisions::with_rule(r["notes"].as_str().unwrap_or(""), &rule) {
            patch[part] = json!({ "repos": { &repo: { "notes": notes } } });
        }
    }
    if patch.as_object().is_some_and(|o| o.is_empty()) {
        return Err(fail(StatusCode::CONFLICT, format!("{repo} has no notes in Build's or Review's config, or the decision's there already")));
    }
    crate::settings::change(&patch).map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    let mut core = shared.lock();
    core.build.config = None;
    core.review.config = None;
    let s = core.append(&id, "standing", &Who::Owner, Some(rule.clone()), json!({ "repo": repo, "key": d.key, "question": d.question, "answer": d.answer })).map_err(internal)?;
    Ok(Json(json!({ "seq": s.seq, "repo": repo, "rule": rule })))
}

/// Run a docket's witnessed checks now; the entries say how they went.
async fn run_checks(State(shared): State<Shared>, Path(id): Path<String>) -> ApiResult {
    let t = crate::checks::target(&shared, &id).map_err(bad)?;
    if t.specs.is_empty() {
        return Err(fail(StatusCode::CONFLICT, format!("{} has no checks yet: add them in Settings, under Repos, or in build.lua", t.repo)));
    }
    if crate::checks::running(&id) {
        return Err(fail(StatusCode::CONFLICT, format!("{id}'s checks are already running")));
    }
    let names: Vec<String> = t.specs.iter().map(|s| s.name.clone()).collect();
    let run = crate::checks::spawn(&shared, &id, t, "asked");
    Ok(Json(json!({ "run": run, "checks": names })))
}

/// Show the docket's pane in its runtime's app (Rex.app, the Herdr app).
async fn open_in_rex(State(shared): State<Shared>, Path(id): Path<String>, axum::extract::Query(q): axum::extract::Query<PaneQuery>) -> ApiResult {
    let (session, block, window) = {
        let core = shared.lock();
        let (_, b) = pane_of(&core, &id, q.block.as_deref())?;
        let p = core.mirror.panes.get(&b).ok_or_else(|| fail(StatusCode::CONFLICT, "the pane closed"))?;
        (p.session_id.clone(), p.block_id.clone(), p.window_id.clone())
    };
    crate::runtime::for_id(&block).focus(&session, &block, window.as_deref()).await.map_err(internal)?;
    Ok(Json(json!({ "ok": true })))
}

/// A pane of the docket's, open now: `want` when it's the docket's, or its
/// lead. `(session, block)`.
fn pane_of(core: &crate::hub::Core, id: &str, want: Option<&str>) -> std::result::Result<(String, String), (StatusCode, Json<Value>)> {
    let st = core.store.state(id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
    let b = agents::pane_on(core, &st, want).map_err(|e| fail(StatusCode::CONFLICT, e))?;
    core.mirror.panes.get(&b).map(|p| (p.session_id.clone(), p.block_id.clone())).ok_or_else(|| fail(StatusCode::CONFLICT, format!("{id}'s pane isn't open")))
}

/// The pane's screen and recent scrollback, as text, for a phone or any
/// screen that isn't next to the pane's runtime.
async fn screen(State(shared): State<Shared>, Path(id): Path<String>, axum::extract::Query(q): axum::extract::Query<PaneQuery>) -> ApiResult {
    let (session, block, live) = {
        let core = shared.lock();
        let (s, b) = pane_of(&core, &id, q.block.as_deref())?;
        let live = agents::live(&core, &b);
        (s, b, live)
    };
    let text = crate::runtime::for_id(&block).capture(&session, &block, 400).await.map_err(internal)?;
    Ok(Json(json!({ "text": text, "live": live, "at": crate::time::now_ms() })))
}

#[derive(Deserialize)]
struct DiffQuery {
    from: Option<String>,
    to: Option<String>,
    path: Option<String>,
    old_path: Option<String>,
    /// `1` or `true`: the files and counts, no patch.
    stat: Option<String>,
}

/// A docket's code changes: from its base (or any point on its record) to
/// its HEAD, a later point, or what isn't committed yet (`crate::diffs`).
async fn docket_diff(State(shared): State<Shared>, Path(id): Path<String>, axum::extract::Query(q): axum::extract::Query<DiffQuery>) -> ApiResult {
    if shared.lock().store.state(&id).map_err(internal)?.is_none() {
        return Err(fail(StatusCode::NOT_FOUND, format!("no docket {id}")));
    }
    let ask = crate::diffs::Ask { from: q.from, to: q.to, path: q.path, old_path: q.old_path, stat: matches!(q.stat.as_deref(), Some("1" | "true")) };
    ask.check().map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    crate::diffs::diff(&shared, &id, &ask).await.map(Json).map_err(|e| fail(StatusCode::CONFLICT, format!("{e:#}")))
}

#[derive(Deserialize)]
struct SendReq {
    text: String,
    /// When the agent is busy: queue it for when its turn ends, instead of refusing.
    #[serde(default)]
    queue: bool,
    /// Which of the docket's agents: its lead when not given.
    #[serde(default)]
    block: Option<String>,
    /// While it works: send it now, for it to read at its next step
    /// (Claude Code, Codex), instead of when the turn ends.
    #[serde(default)]
    steer: bool,
}

/// Take back a prompt queued for the agent, before it goes in.
async fn unqueue(State(shared): State<Shared>, Path((id, q)): Path<(String, i64)>) -> ApiResult {
    let mut core = shared.lock();
    if !core.store.set_prompt_status(q, &id, "cancelled").map_err(internal)? {
        return Err(fail(StatusCode::CONFLICT, "it already went in"));
    }
    core.live_changed();
    let _ = core.events.send(json!({ "type": "docket", "docket": id }));
    Ok(Json(json!({ "ok": true })))
}

/// Type a message into the docket's agent and press Enter, if it's between
/// turns (see `agents::can_send`).
async fn send(State(shared): State<Shared>, Path(id): Path<String>, Json(req): Json<SendReq>) -> ApiResult {
    let text = req.text.trim_end().to_string();
    if text.trim().is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "an empty message"));
    }
    if text.len() > 16_000 {
        return Err(fail(StatusCode::BAD_REQUEST, "that's too long to paste into a pane"));
    }
    let (session, block, app, agent_session, steer) = {
        let mut core = shared.lock();
        let (s, b) = pane_of(&core, &id, req.block.as_deref())?;
        let pane = core.mirror.panes.get(&b).ok_or_else(|| fail(StatusCode::CONFLICT, "the pane closed"))?;
        // Steering an agent at work: it reads it at its next step.
        if req.steer && matches!(pane.root().map(|r| r.state.as_str()), Some("working" | "starting")) {
            let app = pane.app().unwrap_or("agent").to_string();
            if !crate::runtime::steers(&app) {
                return Err(fail(StatusCode::CONFLICT, format!("{} takes messages when its turn ends: queue it", crate::mirror::app_name(&app))));
            }
            if !core.mirror.connected_for(&b) {
                return Err(fail(StatusCode::CONFLICT, format!("Wheelhouse can't see {} right now, so it won't type into the pane", crate::runtime::of(&b).title())));
            }
            let agent_session = core.agents.by_block.get(&b).and_then(|t| t.session.clone());
            (s, b, app, agent_session, true)
        } else {
            if let Err(why) = agents::can_send(pane) {
                // Busy, or waiting on a prompt: it goes in when the turn ends.
                if req.queue && matches!(pane.root().map(|r| r.state.as_str()), Some("working" | "blocked")) {
                    let q = core.store.queue_prompt(&id, &b, &text).map_err(internal)?;
                    core.live_changed();
                    let _ = core.events.send(json!({ "type": "docket", "docket": id }));
                    return Ok(Json(json!({ "ok": true, "queued": q })));
                }
                return Err(fail(StatusCode::CONFLICT, why));
            }
            // A mirror that lost the pane's runtime may be stale.
            if !core.mirror.connected_for(&b) {
                return Err(fail(StatusCode::CONFLICT, format!("Wheelhouse can't see {} right now, so it won't type into the pane", crate::runtime::of(&b).title())));
            }
            let session = core.agents.by_block.get(&b).and_then(|t| t.session.clone());
            (s, b, pane.app().unwrap_or("agent").to_string(), session, false)
        }
    };
    if steer {
        crate::runtime::for_id(&block).steer(&session, &block, &text).await.map_err(|e| fail(StatusCode::CONFLICT, format!("{e:#}")))?;
        let mut core = shared.lock();
        // On the record as yours, sent mid-turn; the agent's own hook, if it
        // reports it too, is told it's already there.
        core.append(&id, "note", &Who::Owner, Some(text.clone()), json!({ "via": "prompt", "session": agent_session, "block": block, "from": "wheelhouse", "steer": true }))
            .map_err(internal)?;
        if let Some(t) = core.agents.by_block.get_mut(&block) {
            t.pending_prompt = Some(text.clone());
        }
        core.agents.remote.insert(block.clone());
        core.live_changed();
        return Ok(Json(json!({ "ok": true, "steered": true })));
    }
    crate::runtime::for_id(&block).submit(&session, &block, &text).await.map_err(internal)?;
    // The agent's prompt hook records it when installed; otherwise the hub does.
    if !crate::hooks::installed_for(&app) {
        let mut core = shared.lock();
        agents::note_prompt(&mut core, &id, &text, agent_session.as_deref(), Some(&block), Some("wheelhouse")).map_err(internal)?;
    }
    let mut core = shared.lock();
    core.agents.remote.insert(block.clone());
    core.live_changed();
    Ok(Json(json!({ "ok": true })))
}

async fn approve(State(shared): State<Shared>, Path(id): Path<String>, Json(seen): Json<crate::approve::Seen>) -> ApiResult {
    crate::approve::answer(&shared, &id, true, &seen).await.map(Json).map_err(|e| fail(StatusCode::CONFLICT, format!("{e:#}")))
}

async fn deny(State(shared): State<Shared>, Path(id): Path<String>, Json(seen): Json<crate::approve::Seen>) -> ApiResult {
    crate::approve::answer(&shared, &id, false, &seen).await.map(Json).map_err(|e| fail(StatusCode::CONFLICT, format!("{e:#}")))
}

async fn export(State(shared): State<Shared>, Path(id): Path<String>) -> Response {
    let core = shared.lock();
    match export_lines(&core, &id) {
        Ok(text) => ([(header::CONTENT_TYPE, "application/x-ndjson")], text).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, format!("{e:#}")).into_response(),
    }
}

/// A docket as JSON lines: a header with the keys it uses, then each entry.
pub fn export_lines(core: &crate::hub::Core, id: &str) -> Result<String> {
    let entries = core.store.entries(id)?;
    anyhow::ensure!(!entries.is_empty(), "no docket {id}");
    let names: std::collections::BTreeMap<String, String> = core.store.keys()?.into_iter().collect();
    let used: std::collections::BTreeSet<String> = entries.iter().filter_map(record::Stored::keyid).collect();
    let keys: Vec<Value> = used.iter().map(|k| json!({ "keyid": k, "name": names.get(k) })).collect();
    let mut out = json!({ "wheelhouse": "record/v1", "docket": id, "keys": keys }).to_string();
    out.push('\n');
    for s in entries {
        out.push_str(&json!({ "seq": s.seq, "id": s.id, "chain": s.chain, "envelope": s.envelope }).to_string());
        out.push('\n');
    }
    Ok(out)
}

#[derive(Deserialize)]
struct IntentReq {
    #[serde(default)]
    body: String,
    #[serde(default)]
    block: Option<String>,
    /// Take the linked issue's description as the intent again; from then on
    /// it follows Linear.
    #[serde(default)]
    take_linear: bool,
}

/// A new version of the docket's intent: yours, or an agent's from its pane.
async fn set_intent(State(shared): State<Shared>, Path(id): Path<String>, Json(req): Json<IntentReq>) -> ApiResult {
    let mut core = shared.lock();
    let st = core.store.state(&id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
    if req.take_linear {
        let l = st.linear.clone().ok_or_else(|| fail(StatusCode::CONFLICT, format!("{id} has no Linear issue")))?;
        let body = l.description.clone().unwrap_or_default();
        let owner = core.owner.clone();
        let s = core.append(&id, "intent", &Who::Hub, Some(body), json!({ "source": "linear", "key": l.key, "updated_at": l.updated_at, "taken_by": owner })).map_err(internal)?;
        return Ok(Json(json!({ "seq": s.seq })));
    }
    let body = req.body.trim_end().to_string();
    if st.intent.as_ref().is_some_and(|t| t.body == body) {
        return Ok(Json(json!({ "unchanged": true })));
    }
    let (who, source) = match req.block.as_deref().and_then(|b| core.mirror.panes.get(b)).filter(|p| p.is_agent()) {
        Some(p) => (
            Who::Agent { app: p.app().unwrap_or("agent").to_string(), block: p.block_id.clone(), session: core.agents.by_block.get(&p.block_id).and_then(|t| t.session.clone()) },
            "agent",
        ),
        None => (Who::Owner, "person"),
    };
    let s = core.append(&id, "intent", &who, Some(body), json!({ "source": source })).map_err(internal)?;
    Ok(Json(json!({ "seq": s.seq })))
}

#[derive(Deserialize)]
struct LinearKey {
    key: String,
}

fn linear_error(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    let msg = format!("{e:#}");
    let status = if msg.contains("wh setup-linear") { StatusCode::PRECONDITION_REQUIRED } else { StatusCode::BAD_GATEWAY };
    (status, Json(json!({ "error": msg, "setup": status == StatusCode::PRECONDITION_REQUIRED })))
}

/// A new docket from a Linear issue: its title, its link, its description as
/// the intent, and its comments on the thread.
async fn from_linear(State(shared): State<Shared>, Json(req): Json<LinearKey>) -> ApiResult {
    let issue = crate::linear::issue(req.key.trim()).await.map_err(linear_error)?;
    let mut core = shared.lock();
    if let Some(d) = core.store.states().map_err(internal)?.into_iter().find(|s| s.linear.as_ref().is_some_and(|l| l.key == issue.identifier) && s.is_open()) {
        return Ok(Json(json!({ "docket": d.id, "existing": true })));
    }
    let id = core.open(&Who::Owner, &issue.title, json!({ "protocol": "none", "title_source": "linear", "linear": issue.identifier })).map_err(internal)?;
    crate::linear::link(&mut core, &id, &issue, &Who::Owner).map_err(internal)?;
    Ok(Json(json!({ "docket": id })))
}

async fn link_linear(State(shared): State<Shared>, Path(id): Path<String>, Json(req): Json<LinearKey>) -> ApiResult {
    let issue = crate::linear::issue(req.key.trim()).await.map_err(linear_error)?;
    let mut core = shared.lock();
    let st = core.store.state(&id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
    if st.linear.as_ref().is_some_and(|l| l.key == issue.identifier) {
        return Ok(Json(json!({ "unchanged": true })));
    }
    crate::linear::link(&mut core, &id, &issue, &Who::Owner).map_err(internal)?;
    Ok(Json(json!({ "linked": issue.identifier })))
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}

async fn linear_search(axum::extract::Query(s): axum::extract::Query<Search>) -> ApiResult {
    let issues = crate::linear::search(&s.q).await.map_err(linear_error)?;
    Ok(Json(json!({ "issues": issues })))
}

async fn brief_of(State(shared): State<Shared>, Path(id): Path<String>, axum::extract::Query(q): axum::extract::Query<PaneQuery>) -> Response {
    let core = shared.lock();
    match crate::brief::brief(&core, &id, q.block.as_deref()) {
        Ok(t) => ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], t).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, format!("{e:#}")).into_response(),
    }
}

#[derive(Deserialize)]
struct PaneQ {
    block: String,
}

/// The brief for the docket a pane is on: what `wh brief` runs from an agent.
async fn brief_of_pane(State(shared): State<Shared>, axum::extract::Query(q): axum::extract::Query<PaneQ>) -> Response {
    let core = shared.lock();
    let Some(id) = agents::docket_of(&core, &q.block) else {
        return (StatusCode::NOT_FOUND, "this pane isn't on a docket: name one, as wh brief D-12").into_response();
    };
    match crate::brief::brief(&core, &id, Some(&q.block)) {
        Ok(t) => ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], t).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, format!("{e:#}")).into_response(),
    }
}

// ---- messages ----

#[derive(Deserialize)]
struct SendMsg {
    to: String,
    body: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    block: Option<String>,
}

fn bad(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    fail(StatusCode::BAD_REQUEST, format!("{e:#}"))
}

async fn messages_send(State(shared): State<Shared>, Json(req): Json<SendMsg>) -> ApiResult {
    let mut core = shared.lock();
    let from = crate::messages::caller(&core, req.block.as_deref());
    let m = crate::messages::send(&mut core, &from, &req.to, req.subject, &req.body, req.options).map_err(bad)?;
    Ok(Json(crate::messages::view(&core, &m)))
}

#[derive(Deserialize)]
struct ReplyMsg {
    #[serde(default)]
    body: String,
    #[serde(default)]
    choice: Option<String>,
    #[serde(default)]
    block: Option<String>,
}

async fn messages_reply(State(shared): State<Shared>, Path(n): Path<i64>, Json(req): Json<ReplyMsg>) -> ApiResult {
    let mut core = shared.lock();
    let from = crate::messages::caller(&core, req.block.as_deref());
    let m = crate::messages::reply(&mut core, &from, n, &req.body, req.choice.as_deref()).map_err(bad)?;
    Ok(Json(crate::messages::view(&core, &m)))
}

#[derive(Deserialize)]
struct PaneQuery {
    #[serde(default)]
    block: Option<String>,
}

async fn messages_list(State(shared): State<Shared>, axum::extract::Query(q): axum::extract::Query<PaneQuery>) -> ApiResult {
    let core = shared.lock();
    let who = crate::messages::caller(&core, q.block.as_deref());
    let list = crate::messages::inbox(&core, &who).map_err(internal)?;
    Ok(Json(json!(list.iter().map(|m| crate::messages::view(&core, m)).collect::<Vec<_>>())))
}

async fn messages_read(State(shared): State<Shared>, Path(n): Path<i64>, axum::extract::Query(q): axum::extract::Query<PaneQuery>) -> ApiResult {
    let mut core = shared.lock();
    let who = crate::messages::caller(&core, q.block.as_deref());
    let m = crate::messages::read(&mut core, &who, n).map_err(bad)?;
    Ok(Json(crate::messages::view(&core, &m)))
}

async fn messages_done(State(shared): State<Shared>, Path(n): Path<i64>) -> ApiResult {
    let mut core = shared.lock();
    core.store.set_message_status(n, "done", None).map_err(internal)?;
    core.live_changed();
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct WaitQ {
    #[serde(default)]
    block: Option<String>,
    #[serde(default)]
    thread: Option<i64>,
    #[serde(default)]
    timeout: Option<u64>,
}

/// Wait for a message to this pane (or a reply in one thread), up to a timeout.
async fn messages_wait(State(shared): State<Shared>, axum::extract::Query(q): axum::extract::Query<WaitQ>) -> ApiResult {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(q.timeout.unwrap_or(300).min(900));
    let mut rx = shared.lock().events.subscribe();
    let pending = |core: &crate::hub::Core| -> Option<crate::store::Message> {
        let who = crate::messages::caller(core, q.block.as_deref());
        let list = crate::messages::inbox(core, &who).ok()?;
        list.into_iter().find(|m| q.thread.is_none_or(|t| m.reply_to == Some(t) || m.id == t))
    };
    loop {
        {
            let mut core = shared.lock();
            if let Some(m) = pending(&core) {
                let who = crate::messages::caller(&core, q.block.as_deref());
                let m = crate::messages::read(&mut core, &who, m.id).map_err(bad)?;
                return Ok(Json(crate::messages::view(&core, &m)));
            }
        }
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Err(_) => return Err(fail(StatusCode::REQUEST_TIMEOUT, "nothing came before the timeout")),
            Ok(_) => continue,
        }
    }
}

#[derive(Deserialize)]
struct NameReq {
    block: String,
    name: String,
}

#[derive(Deserialize)]
struct JoinReq {
    block: String,
    docket: String,
    #[serde(default)]
    role: Option<String>,
}

/// An agent already running joins a docket, beside the agents on it.
async fn join_docket(State(shared): State<Shared>, Json(req): Json<JoinReq>) -> ApiResult {
    let mut core = shared.lock();
    let role = req.role.as_deref().map(str::trim).filter(|r| !r.is_empty());
    agents::join(&mut core, &req.block, req.docket.trim(), role).map(Json).map_err(|e| fail(StatusCode::CONFLICT, format!("{e:#}")))
}

#[derive(Deserialize)]
struct LeaveReq {
    block: String,
    #[serde(default)]
    reason: Option<String>,
}

/// The pane leaves its docket; the agents still on it keep it.
async fn leave_docket(State(shared): State<Shared>, Json(req): Json<LeaveReq>) -> ApiResult {
    let mut core = shared.lock();
    agents::leave(&mut core, &req.block, req.reason.as_deref()).map(Json).map_err(|e| fail(StatusCode::CONFLICT, format!("{e:#}")))
}

/// Name a pane, so others can reach it by that name.
async fn pane_name(State(shared): State<Shared>, Json(req): Json<NameReq>) -> ApiResult {
    let name = req.name.trim().to_string();
    // A name goes into message headers pasted into other agents' panes, and
    // onto a runtime's command line: letters, digits, `.`, `_` and `-`, not leading.
    let ok = !name.is_empty() && name.len() <= 40 && !name.starts_with(['-', '.']) && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !ok {
        return Err(fail(StatusCode::BAD_REQUEST, "a name is one word of letters, digits, '.', '_' or '-', up to 40, not starting with '-' or '.'"));
    }
    let session = {
        let core = shared.lock();
        core.mirror.panes.get(&req.block).map(|p| p.session_id.clone()).ok_or_else(|| fail(StatusCode::NOT_FOUND, "that pane isn't open"))?
    };
    crate::runtime::for_id(&req.block).rename(&session, &req.block, &name).await.map_err(internal)?;
    let mut core = shared.lock();
    if let Some(p) = core.mirror.panes.get_mut(&req.block) {
        p.label = name.clone();
    }
    let addr = core.mirror.panes.get(&req.block).map(|p| crate::messages::addr_of(&core, p)).unwrap_or(name);
    Ok(Json(json!({ "addr": addr })))
}

async fn whoami(State(shared): State<Shared>, axum::extract::Query(q): axum::extract::Query<PaneQuery>) -> ApiResult {
    let core = shared.lock();
    let who = crate::messages::caller(&core, q.block.as_deref());
    let docket = who.block.as_deref().and_then(|b| agents::docket_of(&core, b));
    Ok(Json(json!({ "addr": who.addr, "docket": docket, "agent": who.block.is_some() })))
}

#[derive(Deserialize)]
struct StartReq {
    #[serde(default)]
    app: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    title: Option<String>,
    /// Work on this docket instead of opening a new one.
    #[serde(default)]
    docket: Option<String>,
}

async fn start_agent(State(shared): State<Shared>, Json(req): Json<StartReq>) -> ApiResult {
    crate::start::start(&shared, &req.app, &req.cwd, req.prompt.as_deref(), req.title.as_deref(), req.docket.as_deref())
        .await
        .map(Json)
        .map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))
}

#[derive(Deserialize)]
struct FoldersQ {
    #[serde(default)]
    docket: Option<String>,
}

/// Where an agent could start, and where this docket's should.
async fn folders_list(State(shared): State<Shared>, axum::extract::Query(q): axum::extract::Query<FoldersQ>) -> ApiResult {
    crate::start::folders(&shared, q.docket.as_deref()).await.map(Json).map_err(internal)
}

/// Stop this hub, so `wh up` can hand over to launchd. Not on the tailnet.
async fn hub_stop(req: Request) -> ApiResult {
    if req.extensions().get::<ViaTailnet>().is_some() {
        return Err(fail(StatusCode::FORBIDDEN, "not from the tailnet"));
    }
    say!("stopping: asked to hand over (wh up)");
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        std::process::exit(0);
    });
    Ok(Json(json!({ "stopping": true })))
}

// ---- push ----

async fn push_key() -> ApiResult {
    Ok(Json(json!({ "public_key": crate::push::public_key().map_err(internal)? })))
}

#[derive(Deserialize)]
struct PushSub {
    subscription: Value,
    #[serde(default)]
    agent: Option<String>,
}

async fn push_subscribe(State(shared): State<Shared>, Json(req): Json<PushSub>) -> ApiResult {
    crate::push::subscribe(&shared, &req.subscription, req.agent.as_deref()).map_err(bad)?;
    Ok(Json(json!({ "subscribed": true })))
}

#[derive(Deserialize)]
struct PushUnsub {
    endpoint: String,
}

async fn push_unsubscribe(State(shared): State<Shared>, Json(req): Json<PushUnsub>) -> ApiResult {
    shared.lock().store.push_unsubscribe(&req.endpoint).map_err(internal)?;
    Ok(Json(json!({ "subscribed": false })))
}

async fn push_test(State(shared): State<Shared>) -> ApiResult {
    let note = json!({ "title": "Wheelhouse", "body": "Notifications work. You'll hear when an agent needs you.", "tag": "test", "url": "/" });
    let sent = crate::push::send_all(&shared, &note).await.map_err(internal)?;
    Ok(Json(json!({ "sent": sent })))
}

// ---- Build ----

async fn build_status(State(shared): State<Shared>) -> ApiResult {
    let _ = crate::build::config(&shared);
    Ok(Json(crate::build::watch::status(&shared)))
}

#[derive(Deserialize)]
struct BuildStart {
    /// `D-12`, `owner/name#45`, a GitHub issue URL, or `ENG-142`.
    from: String,
    #[serde(default)]
    repo: Option<String>,
}

async fn build_start(State(shared): State<Shared>, Json(req): Json<BuildStart>) -> ApiResult {
    crate::build::steps::start_from(&shared, &req.from, req.repo.as_deref()).await.map(Json).map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))
}

#[derive(Deserialize)]
struct BuildSettings {
    #[serde(default)]
    paused: Option<bool>,
}

async fn build_settings(State(shared): State<Shared>, Json(req): Json<BuildSettings>) -> ApiResult {
    if let Some(p) = req.paused {
        shared.lock().store.meta_set("build.paused", if p { "1" } else { "0" }).map_err(internal)?;
    }
    shared.lock().build.check_now.notify_one();
    Ok(Json(crate::build::watch::status(&shared)))
}

#[derive(Deserialize)]
struct BuildDiscard {
    docket: String,
    #[serde(default)]
    why: Option<String>,
}

async fn build_discard(State(shared): State<Shared>, Json(req): Json<BuildDiscard>) -> ApiResult {
    let lock = crate::build::lock(&shared, &req.docket);
    let _g = lock.lock().await;
    let mut t = crate::build::get(&shared.lock(), &req.docket).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("{} has no Build", req.docket)))?;
    if t.finished() {
        return Err(fail(StatusCode::CONFLICT, format!("{}'s Build is {} already", req.docket, t.state)));
    }
    crate::build::steps::discard(&shared, &mut t, req.why.as_deref().unwrap_or("you discarded it")).await.map_err(internal)?;
    crate::build::save(&mut shared.lock(), &mut t).map_err(internal)?;
    Ok(Json(json!({ "docket": t.docket, "state": t.state })))
}

// ---- Recap ----

#[derive(Deserialize)]
struct RecapQ {
    /// The period's bounds, from the app (in its time zone).
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    label: Option<String>,
    /// Or `today`, `week`, `30d`, ending now (UTC days), for the CLI.
    #[serde(default)]
    period: Option<String>,
}

async fn recap(State(shared): State<Shared>, axum::extract::Query(q): axum::extract::Query<RecapQ>) -> ApiResult {
    use time::format_description::well_known::Rfc3339;
    let now = time::OffsetDateTime::now_utc();
    let (since, until, label) = match (q.since, q.until) {
        (Some(s), Some(u)) => (s, u, q.label.unwrap_or_else(|| "this period".into())),
        _ => {
            let midnight = now.replace_time(time::Time::MIDNIGHT);
            let (start, label) = match q.period.as_deref().unwrap_or("week") {
                "today" => (midnight, "today"),
                "30d" | "month" => (midnight - time::Duration::days(29), "the last 30 days"),
                _ => (midnight - time::Duration::days(6), "this week"),
            };
            let end = midnight + time::Duration::days(1);
            (start.format(&Rfc3339).unwrap_or_default(), end.format(&Rfc3339).unwrap_or_default(), label.to_string())
        }
    };
    let gathered = {
        let core = shared.lock();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        crate::recap::gather(&core, &since, &until).and_then(|g| Ok((g, crate::recap::bounds(&core, &since, &until, now)?)))
    };
    let ((out, work, names), (from, read_from, to)) = gathered.map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    // The tokens: what the store keeps (it outlasts the transcripts), with
    // what was written since the last snapshot read over it, off the lock.
    let v = tokio::task::spawn_blocking(move || {
        let live = crate::recap::read(&work, read_from).rows;
        let kept = shared.lock().store.usage_between(from, to);
        let rows = match kept {
            Ok(kept) => crate::recap::merge(kept, live),
            Err(e) => {
                say!("Recap couldn't read the kept tokens, so it counts what's on disk: {e:#}");
                live
            }
        };
        crate::recap::finish(out, &names, &rows, &label)
    })
    .await
    .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok(Json(v))
}

// ---- The Review protocol ----

async fn review_status(State(shared): State<Shared>) -> ApiResult {
    let _ = crate::review::config(&shared);
    Ok(Json(crate::review::watch::status(&shared)))
}

async fn review_check(State(shared): State<Shared>) -> ApiResult {
    shared.lock().review.check_now.notify_one();
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ReviewSettings {
    /// Turn the protocol on (importing rex-reviewer's ledger) or off.
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    paused: Option<bool>,
    /// `ask`, `auto`, or `config` (the config's own).
    #[serde(default)]
    post: Option<String>,
    /// Turn on even though rex-reviewer still runs.
    #[serde(default)]
    force: bool,
}

async fn review_settings(State(shared): State<Shared>, Json(req): Json<ReviewSettings>) -> ApiResult {
    if req.enabled == Some(true) {
        if crate::review::rex_reviewer_running() && !req.force {
            return Err(fail(
                StatusCode::CONFLICT,
                "rex-reviewer's watcher is still running (paused or not): stop it first with rex-reviewer down, so two tools don't review the same PRs",
            ));
        }
        crate::review::config(&shared).map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    }
    let mut core = shared.lock();
    if let Some(on) = req.enabled {
        if on && core.store.meta_get("review.paused").map_err(internal)?.is_none() {
            core.store.meta_set("review.paused", "1").map_err(internal)?;
        }
        core.store.meta_set("review.enabled", if on { "1" } else { "0" }).map_err(internal)?;
    }
    if let Some(p) = req.paused {
        core.store.meta_set("review.paused", if p { "1" } else { "0" }).map_err(internal)?;
    }
    match req.post.as_deref() {
        Some("ask" | "auto") => core.store.meta_set("review.post", req.post.as_deref().unwrap_or_default()).map_err(internal)?,
        Some("config") => core.store.meta_set("review.post", "").map_err(internal)?,
        Some(other) => return Err(fail(StatusCode::BAD_REQUEST, format!("post is ask, auto or config, not {other}"))),
        None => {}
    }
    core.review.check_now.notify_one();
    core.live_changed();
    drop(core);
    Ok(Json(crate::review::watch::status(&shared)))
}

// ---- Settings ----

fn settings_now(shared: &Shared) -> Value {
    let b = crate::build::config(shared).map_err(|e| format!("{e:#}"));
    let r = crate::review::config(shared).map_err(|e| format!("{e:#}"));
    let core = shared.lock();
    crate::settings::view(&core, b, r)
}

async fn settings_view(State(shared): State<Shared>) -> ApiResult {
    Ok(Json(settings_now(&shared)))
}

/// Change settings: `{ "build": {…}, "review": {…} }`, laid over what's
/// set, as build.lua and review.lua are shaped. `null` removes a value.
async fn settings_change(State(shared): State<Shared>, Json(patch): Json<Value>) -> ApiResult {
    crate::settings::change(&patch).map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    {
        let mut core = shared.lock();
        // Posting set in Settings is what you want now: it ends any
        // `wh review post-mode ask|auto` that would override it.
        if patch["review"].get("post").is_some() {
            core.store.meta_set("review.post", "").map_err(internal)?;
        }
        core.build.config = None;
        core.review.config = None;
        core.build.check_now.notify_one();
        core.review.check_now.notify_one();
        core.live_changed();
    }
    Ok(Json(settings_now(&shared)))
}

#[derive(Deserialize)]
struct DocQ {
    id: String,
}

async fn settings_doc(axum::extract::Query(q): axum::extract::Query<DocQ>) -> ApiResult {
    let d = crate::settings::doc(&q.id).map_err(|e| fail(StatusCode::NOT_FOUND, format!("{e:#}")))?;
    Ok(Json(d.json(true)))
}

#[derive(Deserialize)]
struct DocSave {
    id: String,
    text: String,
}

async fn settings_doc_save(Json(req): Json<DocSave>) -> ApiResult {
    let d = crate::settings::save_doc(&req.id, &req.text).map_err(bad)?;
    Ok(Json(d.json(true)))
}

async fn settings_doc_reset(Json(q): Json<DocQ>) -> ApiResult {
    crate::settings::reset_doc(&q.id).map_err(bad)?;
    Ok(Json(crate::settings::doc(&q.id).map(|d| d.json(true)).unwrap_or_else(|_| json!({ "id": q.id, "removed": true }))))
}

#[derive(Deserialize)]
struct ImportReq {
    #[serde(default)]
    force: bool,
}

/// Move over from rex-reviewer, once: its config and guides become
/// Wheelhouse's, and its ledger's PRs come in with their rounds.
async fn import_rex_reviewer(State(shared): State<Shared>, Json(req): Json<ImportReq>) -> ApiResult {
    if crate::review::rex_reviewer_running() && !req.force {
        return Err(fail(StatusCode::CONFLICT, "rex-reviewer's watcher is still running: stop it first with rex-reviewer down"));
    }
    let config = if crate::review::config::path().exists() {
        Vec::new()
    } else {
        crate::review::config::import_rex_reviewer().map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))?
    };
    let mut core = shared.lock();
    let prs = crate::review::import(&mut core).map_err(internal)?;
    core.live_changed();
    Ok(Json(json!({
        "config": config.iter().map(|p| crate::paths::tilde(&p.to_string_lossy())).collect::<Vec<_>>(),
        "prs": prs,
    })))
}

#[derive(Deserialize)]
struct PickupReq {
    repo: String,
    pr: i64,
}

async fn review_pickup(State(shared): State<Shared>, Json(req): Json<PickupReq>) -> ApiResult {
    crate::review::rounds::pickup(&shared, &req.repo, req.pr).await.map(Json).map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))
}

#[derive(Deserialize)]
struct ReviewAct {
    /// `owner/name#n`.
    key: String,
    /// The pane asking, from `wh` (`$REX_BLOCK`, `$HC_TERMINAL_ID`); none from the app.
    #[serde(default)]
    block: Option<String>,
    /// The digest of the draft the asker saw: a post refuses a changed one.
    #[serde(default)]
    digest: Option<String>,
}

/// Who's asking to post: you, the PR's checker, or another agent.
fn asker(shared: &Shared, key: &str, block: Option<&str>) -> crate::review::rounds::Asker {
    use crate::review::rounds::Asker;
    let Some(b) = block else { return Asker::Owner };
    let core = shared.lock();
    let e = crate::review::get(&core, key).ok().flatten();
    let pane = core.mirror.panes.get(b);
    let is_checker = e.as_ref().is_some_and(|e| {
        e.checker_block.as_deref() == Some(b) || pane.is_some_and(|p| Some(p.label.as_str()) == e.checker_pane.as_deref() && Some(p.session_id.as_str()) == e.session_id.as_deref())
    });
    if is_checker {
        return Asker::Checker;
    }
    if core.agents.by_block.contains_key(b) || pane.is_some_and(|p| p.is_agent()) {
        return Asker::Agent(b.to_string());
    }
    Asker::Owner
}

async fn review_act(shared: Shared, req: ReviewAct, what: &str) -> ApiResult {
    let key = req.key.trim().to_string();
    if crate::review::pr::parse_key(&key).is_none() {
        return Err(fail(StatusCode::BAD_REQUEST, format!("{key}: give the PR as owner/name#n")));
    }
    let who = asker(&shared, &key, req.block.as_deref());
    if what == "dismiss" && who != crate::review::rounds::Asker::Owner {
        return Err(fail(StatusCode::FORBIDDEN, "only you can dismiss a review"));
    }
    // In a task of its own: a client that stops waiting (the app gives up
    // after a while; a post with many replies takes longer) doesn't cut a
    // post off between GitHub taking it and the record saying so.
    let what = what.to_string();
    let work = tokio::spawn(async move {
        let lock = crate::review::lock(&shared, &key);
        let _g = lock.lock().await;
        match what.as_str() {
            "preview" => crate::review::rounds::post(&shared, &key, true, who, None).await,
            "post" => crate::review::rounds::post(&shared, &key, false, who, req.digest.as_deref()).await,
            _ => crate::review::rounds::dismiss(&shared, &key).await,
        }
    });
    let r = work.await.map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    r.map(Json).map_err(|e| fail(StatusCode::CONFLICT, format!("{e:#}")))
}

async fn review_native_preview(State(shared): State<Shared>, Json(req): Json<ReviewAct>) -> ApiResult {
    review_act(shared, req, "preview").await
}

async fn review_native_post(State(shared): State<Shared>, Json(req): Json<ReviewAct>) -> ApiResult {
    review_act(shared, req, "post").await
}

async fn review_native_dismiss(State(shared): State<Shared>, Json(req): Json<ReviewAct>) -> ApiResult {
    review_act(shared, req, "dismiss").await
}

#[derive(Deserialize)]
struct ShareReq {
    port: u16,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    ask: Option<String>,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    block: Option<String>,
}

async fn share_start(State(shared): State<Shared>, Json(req): Json<ShareReq>) -> ApiResult {
    let from = crate::messages::caller(&shared.lock(), req.block.as_deref());
    if from.block.is_none() {
        return Err(fail(StatusCode::BAD_REQUEST, "share from an agent's pane: the preview lives as long as it does"));
    }
    crate::preview::share(&shared, &from, req.port, req.title, req.ask, req.options).await.map(Json).map_err(|e| fail(StatusCode::BAD_REQUEST, format!("{e:#}")))
}

#[derive(Deserialize)]
struct ShareStop {
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    block: Option<String>,
}

async fn share_stop(State(shared): State<Shared>, Json(req): Json<ShareStop>) -> ApiResult {
    let from = crate::messages::caller(&shared.lock(), req.block.as_deref());
    let n = crate::preview::stop(&shared, &from, req.port).await.map_err(internal)?;
    Ok(Json(json!({ "stopped": n })))
}

async fn verify_all(State(shared): State<Shared>) -> ApiResult {
    let core = shared.lock();
    let mut reports = Vec::new();
    for id in core.store.docket_ids().map_err(internal)? {
        reports.push(core.verify_full(&id).map_err(internal)?);
    }
    reports.sort_by_key(|r| r.docket.trim_start_matches("D-").parse::<i64>().unwrap_or(0));
    Ok(Json(json!({ "ok": reports.iter().all(|r| r.ok), "dockets": reports })))
}

/// One docket's full check: the record, and every file's bytes.
async fn verify_one(State(shared): State<Shared>, Path(id): Path<String>) -> ApiResult {
    let core = shared.lock();
    // The workspace's member log has no docket page, and checks all the same.
    let log = core.team.as_ref().is_some_and(|t| t.log_id() == id);
    if !log && core.store.state(&id).map_err(internal)?.is_none() {
        return Err(fail(StatusCode::NOT_FOUND, format!("no docket {id}")));
    }
    Ok(Json(serde_json::to_value(core.verify_full(&id).map_err(internal)?).unwrap_or_default()))
}

/// The record as the app reads it: every entry with its id, chain value,
/// key and mark, the keys that signed, and the full check, with each
/// problem placed on its entry.
async fn record_view(State(shared): State<Shared>, Path(id): Path<String>) -> ApiResult {
    let core = shared.lock();
    let st = core.store.state(&id).map_err(internal)?.ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no docket {id}")))?;
    let names: std::collections::BTreeMap<String, String> = core.store.keys().map_err(internal)?.into_iter().collect();
    let report = core.verify_full(&id).map_err(internal)?;
    let on = |seq: i64| -> Vec<&String> { report.problems.iter().filter(|p| p.starts_with(&format!("#{seq}:"))).collect() };
    let mut entries = Vec::new();
    let mut signed: std::collections::BTreeMap<String, usize> = Default::default();
    for s in core.store.entries(&id).map_err(internal)? {
        let e = s.entry().map_err(internal)?;
        let keyid = s.keyid();
        if let Some(k) = &keyid {
            *signed.entry(k.clone()).or_default() += 1;
        }
        entries.push(json!({
            "seq": s.seq, "id": s.id, "chain": s.chain, "kind": e.kind, "at": e.at, "actor": e.actor,
            "body": e.body, "data": e.data, "blobs": e.blobs, "replaces": e.replaces, "mark": mark(&e),
            "keyid": keyid, "key": keyid.as_ref().and_then(|k| names.get(k)), "problems": on(s.seq),
        }));
    }
    let keys: Vec<Value> = signed.iter().map(|(k, n)| json!({ "keyid": k, "name": names.get(k), "entries": n })).collect();
    Ok(Json(json!({ "docket": id, "title": st.title, "head": st.head, "short_head": st.short_head(), "keys": keys, "verify": report, "entries": entries })))
}

/// One entry exactly as it's stored and signed: its envelope, and the
/// payload the signature covers.
async fn record_entry(State(shared): State<Shared>, Path((id, seq)): Path<(String, i64)>) -> ApiResult {
    let core = shared.lock();
    let s = core.store.entries(&id).map_err(internal)?.into_iter().find(|s| s.seq == seq).ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("{id} has no entry #{seq}")))?;
    let env: record::Envelope = serde_json::from_str(&s.envelope).map_err(|e| internal(e.into()))?;
    let payload = env.payload_bytes().map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    Ok(Json(
        json!({ "seq": s.seq, "id": s.id, "chain": s.chain, "envelope": s.envelope, "payload": payload, "export_line": json!({ "seq": s.seq, "id": s.id, "chain": s.chain, "envelope": s.envelope }).to_string() }),
    ))
}

async fn artifacts_list(State(shared): State<Shared>, Path(id): Path<String>) -> ApiResult {
    let core = shared.lock();
    Ok(Json(json!({ "artifacts": crate::artifacts::list(&core, &id).map_err(internal)? })))
}

#[derive(Deserialize)]
struct Attach {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// The file's bytes, base64.
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    block: Option<String>,
}

/// Keep a file or a link on a docket: `wh attach`, or the app.
async fn artifacts_attach(State(shared): State<Shared>, Path(id): Path<String>, Json(req): Json<Attach>) -> ApiResult {
    use base64::Engine;
    let mut core = shared.lock();
    if core.store.state(&id).map_err(internal)?.is_none() {
        return Err(fail(StatusCode::NOT_FOUND, format!("no docket {id}")));
    }
    let from = crate::messages::caller(&core, req.block.as_deref());
    let bytes = match &req.content {
        Some(c) => Some(base64::engine::general_purpose::STANDARD.decode(c).map_err(|e| fail(StatusCode::BAD_REQUEST, format!("content isn't base64: {e}")))?),
        None => None,
    };
    let name = req.name.clone().or_else(|| req.url.as_ref().map(|u| u.rsplit('/').find(|p| !p.is_empty()).unwrap_or("link").to_string())).unwrap_or_else(|| "file".into());
    let source = if matches!(from.who, Who::Agent { .. }) { "agent" } else { "you" };
    let title = req.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| crate::artifacts::clean_name(&name));
    let a = crate::artifacts::New { title, name, bytes, url: req.url.clone(), source: source.into(), group: Some("Attached".into()), extra: json!({}) };
    let (seq, new) = crate::artifacts::attach(&mut core, &id, &from.who, a).map_err(bad)?;
    Ok(Json(json!({ "seq": seq, "new": new })))
}

#[derive(Deserialize)]
struct BlobQ {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    download: Option<String>,
}

/// A kept file, by its hash. It can't act as the app: HTML and SVG are
/// sandboxed (scripts, if any, run with no access to this origin), and
/// nothing is sniffed into another type.
async fn blob(State(shared): State<Shared>, Path(digest): Path<String>, axum::extract::Query(q): axum::extract::Query<BlobQ>) -> Response {
    let hex = digest.strip_prefix("sha256:").unwrap_or(&digest).to_string();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (StatusCode::BAD_REQUEST, "not a sha256 digest").into_response();
    }
    let found = shared.lock().store.blob(&hex);
    let bytes = match found {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such file").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
    };
    let name = crate::artifacts::clean_name(q.name.as_deref().unwrap_or("file"));
    let mut ty = crate::artifacts::media_type(&name).to_string();
    if ty.starts_with("text/") || ty == "application/json" || ty == "image/svg+xml" {
        ty.push_str("; charset=utf-8");
    }
    let disposition = format!("{}; filename=\"{}\"", if q.download.is_some() { "attachment" } else { "inline" }, name.replace('"', ""));
    // A PDF or a raster image can't script, and a sandbox stops the
    // browser's PDF viewer; everything else is sandboxed.
    let plain = ty.starts_with("application/pdf") || (ty.starts_with("image/") && !ty.starts_with("image/svg"));
    let csp = if plain { "frame-ancestors 'self'" } else { "sandbox allow-scripts allow-popups; frame-ancestors 'self'" };
    (
        [
            (header::CONTENT_TYPE, ty),
            (header::CONTENT_DISPOSITION, disposition),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            (header::CONTENT_SECURITY_POLICY, csp.into()),
            (header::CACHE_CONTROL, "private, max-age=31536000, immutable".into()),
        ],
        bytes,
    )
        .into_response()
}

#[derive(Deserialize)]
struct Hook {
    event: String,
    agent: String,
    block: String,
    #[serde(default)]
    payload: Value,
}

async fn hook(State(shared): State<Shared>, Json(req): Json<Hook>) -> ApiResult {
    let mut core = shared.lock();
    let v = agents::hook(&mut core, &req.event, &req.agent, &req.block, &req.payload).map_err(internal)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
struct EventsQ {
    client: Option<String>,
}

/// Counts a connected Wheelhouse.app for as long as its stream lives.
struct MacApp(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Drop for MacApp {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// What changed, as it happens. `?client=mac` is Wheelhouse.app: while it
/// listens, the Mac's notifications are its to post.
async fn events(State(shared): State<Shared>, axum::extract::Query(q): axum::extract::Query<EventsQ>) -> impl IntoResponse {
    let (rx, app) = {
        let core = shared.lock();
        let app = (q.client.as_deref() == Some("mac")).then(|| {
            core.mac_apps.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            MacApp(core.mac_apps.clone())
        });
        (core.events.subscribe(), app)
    };
    // A hello at once, so a client knows it's connected before anything
    // changes (a named event: the web app's onmessage doesn't see it).
    let hello = tokio_stream::iter([Ok::<Event, axum::Error>(Event::default().event("hello").data("{}"))]);
    let stream = hello.chain(BroadcastStream::new(rx).filter_map(|r| r.ok()).map(move |ev| {
        let _held = &app;
        Event::default().json_data(ev)
    }));
    // nosniff: without it, URLSession (Wheelhouse.app) holds the first bytes
    // back to guess their type, and the stream looks dead for a while.
    ([(header::X_CONTENT_TYPE_OPTIONS, "nosniff"), (header::CACHE_CONTROL, "no-cache")], Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(5))))
}

// ---- the web app ----

async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    // WH_WEB_DIR serves a built app from disk (web/dist), so a change to the
    // app shows on reload without rebuilding the hub.
    if let Some(dir) = std::env::var_os("WH_WEB_DIR") {
        let dir = std::path::PathBuf::from(dir);
        let safe = !path.split('/').any(|p| p == ".." || p.starts_with('.'));
        let file = if safe && !path.is_empty() && dir.join(path).is_file() { dir.join(path) } else { dir.join("index.html") };
        return match tokio::fs::read(&file).await {
            Ok(bytes) => page(&file.to_string_lossy(), "no-cache", bytes),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{}: {e}", file.display())).into_response(),
        };
    }
    // Anything that isn't a file is a page of the app.
    let file = WEB.get_file(path).or_else(|| WEB.get_file("index.html"));
    let Some(file) = file else { return (StatusCode::NOT_FOUND, "not found").into_response() };
    let name = file.path().to_string_lossy();
    // Built assets have hashed names and never change; the page always might.
    let cache = if name.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    page(&name, cache, file.contents().to_vec())
}

/// The app's own scripts and nothing else run in it; no other site can
/// frame it (a click on Approve or Post is always yours); the files agents
/// keep open only in sandboxed frames of their own.
const APP_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https:; \
     font-src 'self' data:; connect-src 'self'; frame-src 'self'; worker-src 'self'; manifest-src 'self'; \
     object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn page(name: &str, cache: &'static str, bytes: Vec<u8>) -> Response {
    let ty = mime_of(name);
    let mut r = ([(header::CONTENT_TYPE, ty), (header::CACHE_CONTROL, cache), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], bytes).into_response();
    if ty.starts_with("text/html") {
        let h = r.headers_mut();
        h.insert(header::CONTENT_SECURITY_POLICY, axum::http::HeaderValue::from_static(APP_CSP));
        h.insert(header::X_FRAME_OPTIONS, axum::http::HeaderValue::from_static("DENY"));
        h.insert(header::REFERRER_POLICY, axum::http::HeaderValue::from_static("same-origin"));
    }
    r
}

fn mime_of(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    fn app(owner: Option<&str>) -> (Router, Router) {
        let mut core = crate::hub::test_core();
        core.tailnet_owner = owner.map(str::to_string);
        let shared = Shared::new(core);
        let tailnet = router(shared.clone()).layer(middleware::from_fn_with_state(shared.clone(), owner_only));
        (router(shared), tailnet)
    }

    async fn get(app: Router, host: &str, login: Option<&str>) -> StatusCode {
        let mut req = HttpRequest::builder().uri("/api/status").header("host", host);
        if let Some(l) = login {
            req = req.header("tailscale-user-login", l);
        }
        app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap().status()
    }

    #[tokio::test]
    async fn a_review_act_finishes_when_its_caller_stops_waiting() {
        let shared = Shared::new(crate::hub::test_core());
        let key = "acme/web#7";
        let mut e = crate::review::pr::Pr { repo: "acme/web".into(), pr: 7, state: "reviewing".into(), phase: Some("reviewer".into()), round: 1, ..Default::default() };
        crate::review::save(&mut shared.lock(), &mut e).unwrap();
        // The watcher has the PR, so the act waits its turn, and the app
        // gives up meanwhile (as it did on a post with many replies).
        let lock = crate::review::lock(&shared, key);
        let held = lock.lock().await;
        let req = ReviewAct { key: key.into(), block: None, digest: None };
        assert!(tokio::time::timeout(Duration::from_millis(100), review_act(shared.clone(), req, "dismiss")).await.is_err());
        drop(held);
        let mut state = String::new();
        for _ in 0..100 {
            state = crate::review::get(&shared.lock(), key).unwrap().unwrap().state;
            if state != "reviewing" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(state, "posted", "the act went on without its caller");
    }

    #[tokio::test]
    async fn the_tailnet_answers_only_its_owner() {
        let (local, tailnet) = app(Some("you@example.com"));
        assert_eq!(get(local.clone(), "127.0.0.1:7500", None).await, StatusCode::OK);
        // A tailnet name on the local listener: refused, even with a login.
        assert_eq!(get(local.clone(), "mac.tail1.ts.net:7500", Some("you@example.com")).await, StatusCode::MISDIRECTED_REQUEST);
        assert_eq!(get(local, "evil.example", None).await, StatusCode::MISDIRECTED_REQUEST);
        // The tailnet listener: the owner, and only the owner.
        assert_eq!(get(tailnet.clone(), "mac.tail1.ts.net:7500", Some("You@Example.com")).await, StatusCode::OK);
        assert_eq!(get(tailnet.clone(), "mac.tail1.ts.net:7500", Some("someone@else.com")).await, StatusCode::FORBIDDEN);
        assert_eq!(get(tailnet.clone(), "mac.tail1.ts.net:7500", None).await, StatusCode::FORBIDDEN);
        let (_, unknown) = app(None);
        assert_eq!(get(unknown, "mac.tail1.ts.net:7500", Some("you@example.com")).await, StatusCode::SERVICE_UNAVAILABLE);
    }
}
