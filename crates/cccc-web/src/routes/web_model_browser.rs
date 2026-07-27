use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::utc_now;
use cccc_core::GroupStore;
use cccc_core::integration_state;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::io;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

const TARGETS_KEY: &str = "web_model_browser_targets";

#[derive(Debug, Deserialize)]
struct SessionQuery {
    group_id: String,
    actor_id: String,
    #[serde(default)]
    inspect: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/web-model/browser-session", get(info))
        .route("/api/v1/web-model/browser-session/open", post(open))
        .route("/api/v1/web-model/browser-session/close", post(close))
        .route(
            "/api/v1/web-model/browser-session/bind-current",
            post(bind_current),
        )
        .route("/api/v1/web-model/browser-session/ws", get(upgrade))
}

async fn info(State(state): State<AppState>, Query(query): Query<SessionQuery>) -> ApiResult {
    let group_id = required_identifier(&query.group_id, "group_id")?;
    let actor_id = required_identifier(&query.actor_id, "actor_id")?;
    validate_actor(&state, group_id, actor_id)?;
    let _ = query.inspect;
    payload(&state, group_id, actor_id).await
}

async fn open(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let width = dimension(&body, "width", 1366, 640, 2560);
    let height = dimension(&body, "height", 900, 480, 1600);
    let provider = super::web_model_connectors::for_actor(&state, &group_id, &actor_id)
        .and_then(|item| item["provider"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "chatgpt".into());
    let profile = state
        .home
        .root()
        .join("browser-profiles/web-model")
        .join(safe_segment(&group_id)?)
        .join(safe_segment(&actor_id)?);
    state
        .browser_surfaces
        .ensure_open(
            &key(&group_id, &actor_id),
            &profile,
            provider_url(&provider),
            width,
            height,
        )
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?;
    payload(&state, &group_id, &actor_id).await
}

async fn close(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    state
        .browser_surfaces
        .close(&key(&group_id, &actor_id))
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?;
    payload(&state, &group_id, &actor_id).await
}

async fn bind_current(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let clear = body.get("clear").and_then(Value::as_bool).unwrap_or(false);
    let current = state
        .browser_surfaces
        .info(&key(&group_id, &actor_id))
        .await;
    let mut url = body
        .get("conversation_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if url.is_empty() {
        url = current["url"].as_str().unwrap_or("").to_owned();
    }
    let new_chat = body
        .get("new_chat")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let target = if clear {
        json!({})
    } else if new_chat {
        json!({"state":"new_chat_armed","kind":"new_chat","url":url,"saved_at":utc_now(),"next_delivery":"new_chat"})
    } else {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ApiError::bad("a browser conversation URL is required"));
        }
        json!({"state":"bound_existing_chat","kind":"existing_chat","url":url,"saved_at":utc_now(),"next_delivery":"existing_chat"})
    };
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    integration_state::group_update(&store, &group_id, TARGETS_KEY, |value| {
        let targets = ensure_object(value);
        if clear {
            targets.remove(&actor_id);
        } else {
            targets.insert(actor_id.clone(), target);
        }
        Ok(())
    })
    .map_err(io_error)?;
    payload(&state, &group_id, &actor_id).await
}

async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let group_id = required_identifier(&query.group_id, "group_id")?;
    let actor_id = required_identifier(&query.actor_id, "actor_id")?;
    validate_actor(&state, group_id, actor_id)?;
    let session_key = key(group_id, actor_id);
    if state.web_mode.is_read_only() {
        return Ok(ws.on_upgrade(|socket| async move {
            crate::readonly::reject_socket(
                socket,
                "read_only_browser_surface",
                "Web-model browser surface is disabled in read-only mode.",
            )
            .await;
        }));
    }
    Ok(ws.on_upgrade(move |socket| async move {
        crate::browser_surface::serve_socket(socket, &state.browser_surfaces, &session_key).await;
    }))
}

async fn payload(state: &AppState, group_id: &str, actor_id: &str) -> ApiResult {
    let surface = state.browser_surfaces.info(&key(group_id, actor_id)).await;
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    let targets = integration_state::group_get(&store, group_id, TARGETS_KEY).map_err(io_error)?;
    let target = targets.get(actor_id).cloned().unwrap_or_else(|| json!({}));
    let active = surface["active"].as_bool().unwrap_or(false);
    let url = surface["url"].as_str().unwrap_or("");
    let browser = json!({
        "active":active,
        "ready":active,
        "state":if active{"ready"}else{"idle"},
        "tab_url":url,
        "last_tab_url":url,
        "conversation_url":target["url"].as_str().unwrap_or(""),
        "pending_new_chat_bind":target["kind"] == "new_chat",
        "delivery_target":target
    });
    let health = json!({
        "schema":"cccc.web_model.health.v1","group_id":group_id,"actor_id":actor_id,
        "tone":if active{"ready"}else{"needs"},
        "summary":if active{"Browser session is ready."}else{"Open the browser session to continue."},
        "browser":{"state":if active{"ready"}else{"idle"},"active":active,"ready":active,"url":url},
        "delivery_target":browser["delivery_target"]
    });
    Ok(success(json!({
        "browser_session":browser,"browser_surface":surface,"health_snapshot":health
    })))
}

fn validate_actor(state: &AppState, group_id: &str, actor_id: &str) -> Result<(), ApiError> {
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))?;
    group
        .actors
        .iter()
        .any(|actor| actor.id == actor_id)
        .then_some(())
        .ok_or_else(|| ApiError::not_found(format!("actor not found: {actor_id}")))
}

fn provider_url(provider: &str) -> &'static str {
    match provider.trim().to_ascii_lowercase().as_str() {
        "claude" => "https://claude.ai/",
        "gemini" => "https://gemini.google.com/",
        "grok" => "https://grok.com/",
        _ => "https://chatgpt.com/",
    }
}

fn key(group_id: &str, actor_id: &str) -> String {
    format!("web-model::{group_id}::{actor_id}")
}

fn required(body: &Value, key: &str) -> Result<String, ApiError> {
    let value = body.get(key).and_then(Value::as_str).unwrap_or_default();
    required_identifier(value, key).map(str::to_owned)
}

fn required_identifier<'a>(value: &'a str, key: &str) -> Result<&'a str, ApiError> {
    let value = value.trim();
    (!value.is_empty())
        .then_some(value)
        .ok_or_else(|| ApiError::bad(format!("{key} is required")))
}

fn safe_segment(value: &str) -> Result<&str, ApiError> {
    (!value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    .then_some(value)
    .ok_or_else(|| ApiError::bad("invalid browser profile identifier"))
}

fn dimension(body: &Value, key: &str, default: u32, min: u32, max: u32) -> u32 {
    body.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(default)
        .clamp(min, max)
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("object initialized")
}

fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::required_identifier;

    #[test]
    fn query_identifiers_are_required_and_trimmed() {
        assert_eq!(
            required_identifier(" g_one ", "group_id").expect("group id"),
            "g_one"
        );
        assert_eq!(
            required_identifier(" chatgpt-web-1 ", "actor_id").expect("actor id"),
            "chatgpt-web-1"
        );
        assert_eq!(
            required_identifier(" ", "group_id")
                .expect_err("empty group id")
                .to_string(),
            "invalid_request: group_id is required"
        );
        assert_eq!(
            required_identifier("", "actor_id")
                .expect_err("empty actor id")
                .to_string(),
            "invalid_request: actor_id is required"
        );
    }
}
