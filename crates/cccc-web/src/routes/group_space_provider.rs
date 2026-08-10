use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, call, object, success};

#[derive(Debug, Default, Deserialize)]
struct ViewerQuery {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    viewer_mode: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/space/providers/{provider}/credential",
            get(credential).post(update_credential),
        )
        .route("/api/v1/space/providers/{provider}/health", post(health))
        .route(
            "/api/v1/space/providers/{provider}/auth",
            get(auth_status).post(auth_control),
        )
        .route(
            "/api/v1/space/providers/{provider}/auth/browser_surface/ws",
            get(auth_ws),
        )
}

async fn credential(State(state): State<AppState>, Path(provider): Path<String>) -> ApiResult {
    validate_provider(&provider)?;
    call(
        &state,
        "group_space_provider_credential_status",
        object(json!({"provider":provider,"by":"user"})),
    )
    .await
}

async fn update_credential(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_provider(&provider)?;
    let clear = body["clear"].as_bool().unwrap_or(false);
    let raw = body["auth_json"].as_str().unwrap_or("").trim();
    if !clear && !raw.is_empty() {
        serde_json::from_str::<Value>(raw)
            .map_err(|error| ApiError::bad(format!("auth_json is invalid: {error}")))?;
    }
    if !clear && raw.is_empty() {
        return Err(ApiError::bad("auth_json is required"));
    }
    call(
        &state,
        "group_space_provider_credential_update",
        object(json!({"provider":provider,"by":"user","clear":clear,"auth_json":raw})),
    )
    .await
}

async fn health(State(state): State<AppState>, Path(provider): Path<String>) -> ApiResult {
    validate_provider(&provider)?;
    call(
        &state,
        "group_space_provider_health_check",
        object(json!({"provider":provider,"by":"user"})),
    )
    .await
}

async fn auth_status(State(state): State<AppState>, Path(provider): Path<String>) -> ApiResult {
    auth_payload(&state, &provider).await
}

async fn auth_control(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_provider(&provider)?;
    match body["action"].as_str().unwrap_or("status") {
        "start" => {
            crate::notebooklm_auth::remove_legacy_profile(&state.home).await;
            state
                .notebooklm_auth
                .start(
                    state.home.clone(),
                    state.client.clone(),
                    state.browser_surfaces.clone(),
                    body["timeout_seconds"].as_u64().unwrap_or(900),
                    body["force_reauth"].as_bool().unwrap_or(false),
                )
                .await;
        }
        "refresh" | "complete" | "status" => {}
        "cancel" => {
            state
                .notebooklm_auth
                .cancel(&state.browser_surfaces, "Connect canceled.")
                .await;
        }
        "disconnect" => {
            state
                .notebooklm_auth
                .cancel(&state.browser_surfaces, "Google account disconnected.")
                .await;
            let _ = call(
                &state,
                "group_space_provider_credential_update",
                object(json!({"provider":provider,"by":"user","clear":true})),
            )
            .await?;
            crate::notebooklm_auth::remove_legacy_profile(&state.home).await;
        }
        _ => return Err(ApiError::bad("unsupported provider auth action")),
    }
    auth_payload(&state, &provider).await
}

async fn auth_ws(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<ViewerQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    validate_provider(&provider)?;
    let key = browser_key(&provider);
    if state.web_mode.is_read_only() {
        return Ok(ws.on_upgrade(|socket| async move {
            crate::readonly::reject_socket(
                socket,
                "read_only_browser_surface",
                "Provider auth browser surface is disabled in read-only mode.",
            )
            .await;
        }));
    }
    let vnc = query.mode.trim().eq_ignore_ascii_case("vnc");
    let viewer_mode = query.viewer_mode;
    Ok(ws.on_upgrade(move |socket| async move {
        if vnc {
            crate::browser_surface::serve_vnc_socket(
                socket,
                &state.browser_surfaces,
                &key,
                state.shutdown.subscribe(),
            )
            .await;
        } else {
            crate::browser_surface::serve_socket(
                socket,
                &state.browser_surfaces,
                &key,
                &viewer_mode,
                state.shutdown.subscribe(),
            )
            .await;
        }
    }))
}

async fn auth_payload(state: &AppState, provider: &str) -> ApiResult {
    validate_provider(provider)?;
    let mut auth = state
        .notebooklm_auth
        .snapshot(&state.browser_surfaces)
        .await;
    let daemon_payload = call(
        state,
        "group_space_provider_auth",
        object(json!({"provider":provider,"by":"user","action":"status"})),
    )
    .await?
    .0;
    let durable = daemon_payload["result"].clone();
    let credential = durable["credential"].clone();
    let provider_state = durable["provider_state"].clone();
    let configured = credential["configured"].as_bool().unwrap_or(false);
    let verified = provider_state["write_ready"].as_bool() == Some(true);
    reconcile_auth_state(&mut auth, configured, verified, &provider_state);
    Ok(success(json!({
        "provider":provider,"provider_state":provider_state,
        "credential":credential,
        "auth":auth
    })))
}

fn reconcile_auth_state(
    auth: &mut Value,
    configured: bool,
    verified: bool,
    provider_state: &Value,
) {
    let state = auth["state"].as_str().unwrap_or("idle");
    if state == "running" {
        return;
    }
    if verified && state == "idle" {
        auth["state"] = json!("succeeded");
        auth["phase"] = json!("done");
        auth["message"] = json!("Saved Google session is verified.");
        auth["error"] = Value::Null;
    } else if configured && !verified && matches!(state, "idle" | "succeeded") {
        auth["state"] = json!("failed");
        auth["phase"] = json!("waiting_user_login");
        auth["message"] = json!("Saved Google session requires verification.");
        if let Some(message) = provider_state["last_error"]
            .as_str()
            .filter(|message| !message.trim().is_empty())
        {
            auth["error"] = json!({"code":"space_provider_auth_invalid","message":message});
        }
    } else if !configured && state == "succeeded" {
        auth["state"] = json!("idle");
        auth["phase"] = json!("idle");
        auth["message"] = json!("Google account is not connected.");
        auth["error"] = Value::Null;
    }
}
fn browser_key(provider: &str) -> String {
    format!("space-provider::{provider}")
}
fn validate_provider(provider: &str) -> Result<(), ApiError> {
    (!provider.is_empty()
        && provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(())
    .ok_or_else(|| ApiError::bad("invalid provider"))
}
#[cfg(test)]
mod tests {
    use super::reconcile_auth_state;
    use serde_json::json;

    #[test]
    fn saved_credentials_are_connected_only_after_verification() {
        let provider = json!({"write_ready":false,"last_error":"expired"});
        let mut auth = json!({"state":"idle","phase":"idle","error":null});
        reconcile_auth_state(&mut auth, true, false, &provider);
        assert_eq!(auth["state"], "failed");
        assert_eq!(auth["error"]["message"], "expired");

        let provider = json!({"write_ready":true,"last_error":null});
        let mut auth = json!({"state":"idle","phase":"idle","error":null});
        reconcile_auth_state(&mut auth, true, true, &provider);
        assert_eq!(auth["state"], "succeeded");
    }
}
