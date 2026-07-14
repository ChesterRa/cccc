use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::utc_now;
use cccc_core::integration_state;
use serde_json::{Map, Value, json};
use std::io;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

const STORE_KEY: &str = "space_providers";

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
    let item = provider_value(&state, &provider)?;
    Ok(success(
        json!({"provider":provider,"credential":credential_payload(&provider,&item)}),
    ))
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
    update_provider(&state, &provider, |item| {
        if clear {
            item.remove("auth_json");
        } else if !raw.is_empty() {
            item.insert("auth_json".into(), Value::String(raw.into()));
        }
        item.insert("credential_updated_at".into(), Value::String(utc_now()));
        Ok(())
    })?;
    let item = provider_value(&state, &provider)?;
    Ok(success(
        json!({"provider":provider,"credential":credential_payload(&provider,&item)}),
    ))
}

async fn health(State(state): State<AppState>, Path(provider): Path<String>) -> ApiResult {
    validate_provider(&provider)?;
    let item = provider_value(&state, &provider)?;
    let surface = state.browser_surfaces.info(&browser_key(&provider)).await;
    let healthy = item["auth_json"]
        .as_str()
        .is_some_and(|value| !value.is_empty())
        || surface["active"].as_bool().unwrap_or(false);
    Ok(success(json!({
        "provider":provider,"healthy":healthy,
        "health":{"checked_at":utc_now(),"browser_active":surface["active"]},
        "provider_state":provider_state(&provider,healthy),
        "credential":credential_payload(&provider,&item),
        "error":if healthy{Value::Null}else{json!({"code":"auth_required","message":"NotebookLM authentication is not configured"})}
    })))
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
            let profile = state
                .home
                .root()
                .join("browser-profiles/space-auth")
                .join(&provider);
            state
                .browser_surfaces
                .open(
                    &browser_key(&provider),
                    &profile,
                    provider_url(&provider),
                    1366,
                    900,
                )
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?;
            update_provider(&state, &provider, |item| {
                item.insert("auth_state".into(), Value::String("running".into()));
                item.insert("auth_started_at".into(), Value::String(utc_now()));
                Ok(())
            })?;
        }
        "cancel" | "disconnect" => {
            state
                .browser_surfaces
                .close(&browser_key(&provider))
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?;
            update_provider(&state, &provider, |item| {
                item.insert("auth_state".into(), Value::String("canceled".into()));
                item.insert("auth_finished_at".into(), Value::String(utc_now()));
                if body["action"] == "disconnect" {
                    item.remove("auth_json");
                }
                Ok(())
            })?;
        }
        "status" => {}
        _ => return Err(ApiError::bad("unsupported provider auth action")),
    }
    auth_payload(&state, &provider).await
}

async fn auth_ws(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    validate_provider(&provider)?;
    let key = browser_key(&provider);
    Ok(ws.on_upgrade(move |socket| async move {
        crate::browser_surface::serve_socket(socket, &state.browser_surfaces, &key).await;
    }))
}

async fn auth_payload(state: &AppState, provider: &str) -> ApiResult {
    validate_provider(provider)?;
    let item = provider_value(state, provider)?;
    let surface = state.browser_surfaces.info(&browser_key(provider)).await;
    let active = surface["active"].as_bool().unwrap_or(false);
    let configured = item["auth_json"]
        .as_str()
        .is_some_and(|value| !value.is_empty());
    let auth_state = if active {
        "running"
    } else if configured {
        "succeeded"
    } else {
        item["auth_state"].as_str().unwrap_or("idle")
    };
    Ok(success(json!({
        "provider":provider,"provider_state":provider_state(provider,configured||active),
        "credential":credential_payload(provider,&item),
        "auth":{"provider":provider,"state":auth_state,"phase":if active{"browser_login"}else{"idle"},
            "delivery":"projected_browser","started_at":item["auth_started_at"],"updated_at":utc_now(),
            "message":if active{"Complete sign-in in the projected browser."}else{"Authentication browser is idle."},
            "error":null,"projected_browser":surface}
    })))
}

fn provider_value(state: &AppState, provider: &str) -> Result<Value, ApiError> {
    Ok(integration_state::global_get(&state.home, STORE_KEY)
        .map_err(io_error)?
        .get(provider)
        .cloned()
        .unwrap_or_else(|| json!({})))
}

fn update_provider<T>(
    state: &AppState,
    provider: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> Result<T, ApiError> {
    integration_state::global_update(&state.home, STORE_KEY, |value| {
        if !value.is_object() {
            *value = json!({});
        }
        let providers = value.as_object_mut().expect("providers initialized");
        let item = providers.entry(provider).or_insert_with(|| json!({}));
        if !item.is_object() {
            *item = json!({});
        }
        change(item.as_object_mut().expect("provider initialized"))
    })
    .map_err(io_error)
}

fn credential_payload(provider: &str, item: &Value) -> Value {
    let raw = item["auth_json"].as_str().unwrap_or("");
    json!({"provider":provider,"key":format!("{}_auth_json",provider),"configured":!raw.is_empty(),"source":if raw.is_empty(){"none"}else{"store"},"env_configured":false,"store_configured":!raw.is_empty(),"updated_at":item["credential_updated_at"],"masked_value":if raw.is_empty(){""}else{"********"}})
}
fn provider_state(provider: &str, ready: bool) -> Value {
    json!({"provider":provider,"enabled":true,"real_enabled":ready,"mode":if ready{"active"}else{"degraded"},"real_adapter_enabled":ready,"stub_adapter_enabled":!ready,"auth_configured":ready,"write_ready":ready,"readiness_reason":if ready{"ready"}else{"authentication required"}})
}
fn provider_url(provider: &str) -> &'static str {
    match provider {
        "notebooklm" => "https://notebooklm.google.com/",
        _ => "https://notebooklm.google.com/",
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
fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
