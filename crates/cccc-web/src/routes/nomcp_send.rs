use axum::Router;
use axum::extract::{Form, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use base64::Engine;
use cccc_contracts::DaemonRequest;
use cccc_core::nomcp::Store;
use serde::Deserialize;
use serde_json::{Map, json};

use super::nomcp::{auth_failure, authorize, failure};
use super::nomcp_render;
use crate::AppState;

#[derive(Debug, Deserialize)]
struct TokenQuery {
    token: String,
}
#[derive(Debug, Deserialize)]
struct SendQuery {
    token: String,
    msg_id: String,
    text_b64url: String,
    title: Option<String>,
}
#[derive(Debug, Deserialize)]
struct SendForm {
    msg_id: String,
    text: String,
    title: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/nomcp/s/{sid}/send", get(send_get).post(send_post))
}

async fn send_post(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(token): Query<TokenQuery>,
    Form(form): Form<SendForm>,
) -> Response {
    send(
        &state,
        &sid,
        &token.token,
        &form.msg_id,
        &form.text,
        form.title.as_deref().unwrap_or(""),
    )
    .await
}

async fn send_get(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(query): Query<SendQuery>,
) -> Response {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(query.text_b64url)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let Some(text) = decoded else {
        return failure(StatusCode::BAD_REQUEST, "invalid base64url text");
    };
    send(
        &state,
        &sid,
        &query.token,
        &query.msg_id,
        &text,
        query.title.as_deref().unwrap_or(""),
    )
    .await
}

async fn send(
    state: &AppState,
    sid: &str,
    token: &str,
    msg_id: &str,
    text: &str,
    title: &str,
) -> Response {
    let Ok((session, _)) = authorize(state, sid, token) else {
        return auth_failure(state, sid, token);
    };
    if text.len() > 12 * 1024 {
        return failure(StatusCode::PAYLOAD_TOO_LARGE, "message is too large");
    }
    if session.sent_message_ids.contains(msg_id) {
        return Html(nomcp_render::html("No-MCP Advisory", "duplicate_ignored")).into_response();
    }
    let args = json!({
        "group_id":session.group_id,"by":"nomcp-advisory","to":[session.recipient],
        "text":text,"title":title,"reply_to":session.reply_to_event_id,"source_platform":"nomcp",
        "source_user_id":sid,"client_id":format!("nomcp:{sid}:{msg_id}"),
        "refs":[{"source":"nomcp","cannot_execute_local_tools":true}]
    });
    let response = state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: "message_send".into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        })
        .await;
    if !response.is_ok_and(|item| item.ok) {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "daemon send failed");
    }
    let store = match Store::new(state.home.clone()) {
        Ok(store) => store,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    if let Err(error) = store.record_message(sid, msg_id) {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    Html(nomcp_render::html("No-MCP Advisory", "accepted")).into_response()
}
