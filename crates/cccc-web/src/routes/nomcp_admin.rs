use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use cccc_core::nomcp::{CreateSpec, Session, Store};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{nomcp_render, nomcp_resources};
use crate::AppState;

#[derive(Debug, Deserialize)]
struct CreateBody {
    group_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    brief: String,
    #[serde(default)]
    reply_to_event_id: String,
    #[serde(default = "recipient")]
    recipient: String,
    #[serde(default)]
    scope_key: String,
    #[serde(default)]
    allowed_paths: Vec<String>,
    #[serde(default = "expiry")]
    expires_in_seconds: i64,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    group_id: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/nomcp/sessions", get(list).post(create))
        .route("/api/v1/nomcp/sessions/{sid}", get(show).delete(revoke))
}

async fn list(State(state): State<AppState>, Query(query): Query<ListQuery>) -> Response {
    let store = match Store::new(state.home.clone()) {
        Ok(store) => store,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    match store.list() {
        Ok(items) => {
            let sessions: Vec<_> = items
                .iter()
                .filter(|item| {
                    query
                        .group_id
                        .as_ref()
                        .is_none_or(|group| &item.group_id == group)
                })
                .map(|item| masked(&state, item, false))
                .collect();
            Json(json!({"ok":true,"result":{"sessions":sessions}})).into_response()
        }
        Err(error) => failure(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn create(State(state): State<AppState>, Json(body): Json<CreateBody>) -> Response {
    let store = match Store::new(state.home.clone()) {
        Ok(store) => store,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    match store.create(CreateSpec {
        group_id: body.group_id,
        title: body.title,
        brief: body.brief,
        reply_to_event_id: body.reply_to_event_id,
        recipient: body.recipient,
        scope_key: body.scope_key,
        allowed_paths: body.allowed_paths,
        expires_in_seconds: body.expires_in_seconds,
    }) {
        Ok(created) => {
            let mut session = masked(&state, &created.session, true);
            if let Some(object) = session.as_object_mut() {
                let base = public_url();
                object.insert(
                    "session_url".into(),
                    json!(format!("{base}/nomcp/s/{}", created.session.sid)),
                );
                object.insert(
                    "session_url_with_token".into(),
                    json!(format!(
                        "{base}/nomcp/s/{}?token={}",
                        created.session.sid, created.secret
                    )),
                );
                object.insert("secret_available".into(), Value::Bool(true));
            }
            Json(json!({"ok":true,"result":{"session":session,"secret":created.secret}}))
                .into_response()
        }
        Err(error) => failure(StatusCode::BAD_REQUEST, error),
    }
}

async fn show(State(state): State<AppState>, Path(sid): Path<String>) -> Response {
    let store = match Store::new(state.home.clone()) {
        Ok(store) => store,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    match store.get(&sid) {
        Ok(Some(session)) => {
            Json(json!({"ok":true,"result":{"session":masked(&state,&session,false)}}))
                .into_response()
        }
        Ok(None) => failure(StatusCode::NOT_FOUND, "session not found"),
        Err(error) => failure(StatusCode::BAD_REQUEST, error),
    }
}

async fn revoke(State(state): State<AppState>, Path(sid): Path<String>) -> Response {
    let store = match Store::new(state.home.clone()) {
        Ok(store) => store,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    match store.revoke(&sid) {
        Ok(true) => Json(json!({"ok":true,"result":{"sid":sid,"revoked":true}})).into_response(),
        Ok(false) => failure(StatusCode::NOT_FOUND, "session not found"),
        Err(error) => failure(StatusCode::BAD_REQUEST, error),
    }
}

fn masked(state: &AppState, session: &Session, with_url: bool) -> Value {
    let root = nomcp_resources::root(&state.home, session).ok();
    let resource_count = root
        .as_ref()
        .and_then(|root| nomcp_resources::resources(root, session).ok())
        .map_or(0, |items| items.len());
    let changed_file_count = root
        .as_ref()
        .and_then(|root| {
            nomcp_render::git_status(root)
                .get("changed_files")
                .and_then(Value::as_array)
                .map(Vec::len)
        })
        .unwrap_or(0);
    let mut value = json!({"schema":session.schema,"sid":session.sid,"group_id":session.group_id,"title":session.title,"brief":session.brief,"reply_to_event_id":session.reply_to_event_id,"recipient":session.recipient,"scope_key":session.scope_key,"allowed_paths":session.allowed_paths,"created_at":session.created_at,"expires_at":session.expires_at,"revoked":!session.revoked_at.is_empty(),"resource_count":resource_count,"changed_file_count":changed_file_count,"secret_available":false});
    if with_url {
        value.as_object_mut().map(|object| {
            object.insert(
                "session_url".into(),
                json!(format!("{}/nomcp/s/{}", public_url(), session.sid)),
            )
        });
    }
    value
}

fn public_url() -> String {
    std::env::var("CCCC_WEB_PUBLIC_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8848".into())
        .trim_end_matches('/')
        .trim_end_matches("/ui")
        .into()
}

fn failure(status: StatusCode, error: impl std::fmt::Display) -> Response {
    (status, Json(json!({"ok":false,"error":{"code":"nomcp_error","message":error.to_string(),"details":{}}}))).into_response()
}
fn recipient() -> String {
    "user".into()
}
const fn expiry() -> i64 {
    86_400
}
