//! A member's hub's API for teams: what `wh team` and the apps call, on
//! the hub's own socket and loopback (never the workspace's).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::member;
use crate::hub::{Shared, Who};

type ApiResult = std::result::Result<Json<Value>, (StatusCode, Json<Value>)>;

fn fail(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    let status = match e.downcast_ref::<member::Refused>() {
        Some(r) if r.status == 403 => StatusCode::FORBIDDEN,
        Some(_) => StatusCode::UNPROCESSABLE_ENTITY,
        None => StatusCode::BAD_REQUEST,
    };
    (status, Json(json!({ "error": format!("{e:#}") })))
}

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/team", get(status))
        .route("/api/team/create", post(create))
        .route("/api/team/invite", post(invite))
        .route("/api/team/join", post(join))
        .route("/api/team/share", post(share))
        .route("/api/team/new", post(new_shared))
        .route("/api/team/role", post(role))
        .route("/api/team/remove", post(remove))
        .route("/api/team/outbox/{n}", post(outbox))
        .route("/api/team/sync", post(sync))
        .route("/api/team/dockets/{id}/people", post(people))
        .route("/api/team/dockets/{id}/trust", post(trust))
}

async fn status(State(shared): State<Shared>) -> ApiResult {
    Ok(Json(member::status(&shared.lock())))
}

#[derive(Deserialize)]
struct CreateReq {
    url: String,
    setup: String,
    name: String,
    prefix: String,
    handle: String,
    #[serde(default)]
    login: Option<String>,
}

async fn create(State(shared): State<Shared>, Json(r): Json<CreateReq>) -> ApiResult {
    member::create(&shared, r.url.trim(), &r.setup, r.name.trim(), r.prefix.trim(), r.handle.trim(), r.login).await.map(Json).map_err(fail)
}

#[derive(Deserialize)]
struct InviteReq {
    handle: String,
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    hours: Option<i64>,
}

async fn invite(State(shared): State<Shared>, Json(r): Json<InviteReq>) -> ApiResult {
    member::invite(&shared, r.handle.trim(), if r.admin { "admin" } else { "member" }, r.hours.unwrap_or(super::roster::INVITE_HOURS)).await.map(Json).map_err(fail)
}

#[derive(Deserialize)]
struct JoinReq {
    code: String,
    #[serde(default)]
    login: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

async fn join(State(shared): State<Shared>, Json(r): Json<JoinReq>) -> ApiResult {
    member::join(&shared, &r.code, r.login, r.name).await.map(Json).map_err(fail)
}

#[derive(Deserialize)]
struct ShareReq {
    docket: String,
}

async fn share(State(shared): State<Shared>, Json(r): Json<ShareReq>) -> ApiResult {
    member::share(&shared, r.docket.trim()).await.map(Json).map_err(fail)
}

#[derive(Deserialize)]
struct NewReq {
    title: String,
}

async fn new_shared(State(shared): State<Shared>, Json(r): Json<NewReq>) -> ApiResult {
    member::new_shared(&shared, &r.title).await.map(Json).map_err(fail)
}

#[derive(Deserialize)]
struct RoleReq {
    handle: String,
    role: String,
}

async fn role(State(shared): State<Shared>, Json(r): Json<RoleReq>) -> ApiResult {
    member::log_entry(&shared, "role", json!({ "handle": r.handle.trim(), "role": r.role.trim() })).await.map(Json).map_err(fail)
}

#[derive(Deserialize)]
struct RemoveReq {
    handle: String,
    #[serde(default)]
    reason: Option<String>,
}

async fn remove(State(shared): State<Shared>, Json(r): Json<RemoveReq>) -> ApiResult {
    member::log_entry(&shared, "remove", json!({ "handle": r.handle.trim(), "reason": r.reason })).await.map(Json).map_err(fail)
}

#[derive(Deserialize)]
struct OutboxReq {
    /// `send` (a held entry goes after all) or `drop`.
    action: String,
}

async fn outbox(State(shared): State<Shared>, Path(n): Path<i64>, Json(r): Json<OutboxReq>) -> ApiResult {
    let send = match r.action.as_str() {
        "send" => true,
        "drop" => false,
        other => return Err(fail(anyhow::anyhow!("send or drop, not {other}"))),
    };
    let mut core = shared.lock();
    member::release_held(&mut core, n, send).map_err(fail)?;
    Ok(Json(json!({ "ok": true })))
}

/// Send and pull now, and say how it went: for `wh team sync` and tests.
async fn sync(State(shared): State<Shared>) -> ApiResult {
    member::send(&shared).await.map_err(fail)?;
    member::pull(&shared, 0).await.map_err(fail)?;
    Ok(Json(member::status(&shared.lock())))
}

#[derive(Deserialize)]
struct PeopleReq {
    /// Whose place changes; none to change only who answers.
    #[serde(default)]
    handle: Option<String>,
    /// `owner` or `member`.
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    remove: bool,
    /// Who answers agents' questions: `owners` or `members`.
    #[serde(default)]
    answer: Option<String>,
}

async fn people(State(shared): State<Shared>, Path(id): Path<String>, Json(r): Json<PeopleReq>) -> ApiResult {
    let mut data = json!({});
    if let Some(h) = r.handle.as_deref().map(str::trim) {
        data["handle"] = json!(h);
        data["role"] = json!(r.role.as_deref().unwrap_or("member"));
        if r.remove {
            data["action"] = json!("remove");
        }
    }
    if let Some(a) = r.answer {
        data["answer"] = json!(a);
    }
    let mut core = shared.lock();
    let s = core.append(&id, "people", &Who::Owner, None, data).map_err(fail)?;
    Ok(Json(json!({ "id": s.id })))
}

#[derive(Deserialize)]
struct TrustReq {
    #[serde(default)]
    teammate: Option<String>,
    #[serde(default = "yes")]
    on: bool,
    #[serde(default)]
    private_prompts: Option<bool>,
}

fn yes() -> bool {
    true
}

async fn trust(State(shared): State<Shared>, Path(id): Path<String>, Json(r): Json<TrustReq>) -> ApiResult {
    let mut core = shared.lock();
    let mut data = json!({ "on": r.on });
    if let Some(t) = r.teammate {
        data["teammate"] = json!(t.trim());
    }
    if let Some(p) = r.private_prompts {
        data["private_prompts"] = json!(p);
    }
    let s = core.append(&id, "trust", &Who::Owner, None, data).map_err(fail)?;
    Ok(Json(json!({ "id": s.id })))
}
