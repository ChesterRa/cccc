use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_core::space_credentials;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io;

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
            space_credentials::clear(&state.home, &provider).map_err(io_error)?;
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
    let credential = credential_payload(state, provider)?;
    let configured = credential["configured"].as_bool().unwrap_or(false);
    if auth["state"] == "idle" && configured {
        auth["state"] = json!("succeeded");
        auth["phase"] = json!("done");
        auth["message"] = json!("A saved Google session is configured.");
    }
    let active = auth["state"] == "running";
    Ok(success(json!({
        "provider":provider,"provider_state":provider_state(provider,configured||active),
        "credential":credential,
        "auth":auth
    })))
}

fn credential_payload(state: &AppState, provider: &str) -> Result<Value, ApiError> {
    space_credentials::status(&state.home, provider).map_err(io_error)
}
fn provider_state(provider: &str, ready: bool) -> Value {
    json!({"provider":provider,"enabled":ready,"real_enabled":true,"mode":if ready{"active"}else{"disabled"},"real_adapter_enabled":true,"stub_adapter_enabled":false,"auth_configured":ready,"write_ready":ready,"readiness_reason":if ready{"authenticated Rust adapter"}else{"credential missing"}})
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
fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
