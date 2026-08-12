use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use cccc_core::GroupStore;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

#[derive(Debug, Deserialize)]
struct SlotQuery {
    slot: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    viewer_mode: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/presentation/browser_surface/session",
            get(info).post(open),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/browser_surface/session/close",
            axum::routing::post(close),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/browser_surface/ws",
            get(upgrade),
        )
}

async fn info(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SlotQuery>,
) -> ApiResult {
    validate_group(&state, &group_id)?;
    let slot = normalize_slot(&query.slot)?;
    Ok(success(
        json!({"group_id":group_id,"browser_surface":state.browser_surfaces.info(&key(&group_id,&slot)).await}),
    ))
}

async fn open(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_group(&state, &group_id)?;
    let slot = normalize_slot(body.get("slot").and_then(Value::as_str).unwrap_or(""))?;
    let url = body.get("url").and_then(Value::as_str).unwrap_or("");
    let width = body
        .get("width")
        .and_then(Value::as_u64)
        .unwrap_or(1280)
        .clamp(640, 2560) as u32;
    let height = body
        .get("height")
        .and_then(Value::as_u64)
        .unwrap_or(800)
        .clamp(480, 1600) as u32;
    let profile = state
        .home
        .root()
        .join("state/presentation_browser")
        .join(&group_id)
        .join(&slot)
        .join("profile");
    let surface = state
        .browser_surfaces
        .open(&key(&group_id, &slot), &profile, url, width, height)
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(success(
        json!({"group_id":group_id,"browser_surface":surface}),
    ))
}

async fn close(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_group(&state, &group_id)?;
    let slot = normalize_slot(body.get("slot").and_then(Value::as_str).unwrap_or(""))?;
    let closed = state
        .browser_surfaces
        .close(&key(&group_id, &slot))
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(success(
        json!({"group_id":group_id,"closed":closed,"browser_surface":state.browser_surfaces.info(&key(&group_id,&slot)).await}),
    ))
}

async fn upgrade(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SlotQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if state.web_mode.is_read_only() {
        return ws.on_upgrade(|socket| async move {
            crate::readonly::reject_socket(
                socket,
                "read_only_browser_surface",
                "Presentation browser surface is disabled in read-only mode.",
            )
            .await;
        });
    }
    if let Err(error) = validate_group(&state, &group_id) {
        return error.into_response();
    }
    let slot = match normalize_slot(&query.slot) {
        Ok(slot) => slot,
        Err(error) => return error.into_response(),
    };
    let vnc = query.mode.trim().eq_ignore_ascii_case("vnc");
    ws.on_upgrade(move |socket| serve(socket, state, key(&group_id, &slot), vnc, query.viewer_mode))
}

async fn serve(socket: WebSocket, state: AppState, key: String, vnc: bool, viewer_mode: String) {
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
}

fn validate_group(state: &AppState, group_id: &str) -> Result<(), ApiError> {
    let store =
        GroupStore::new(state.home.clone()).map_err(|error| ApiError::bad(error.to_string()))?;
    match store.load(group_id) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(
            ApiError::not_found_code("group_not_found", format!("group not found: {group_id}")),
        ),
        Err(error) => Err(ApiError::bad(error.to_string())),
    }
}

fn normalize_slot(value: &str) -> Result<String, ApiError> {
    let mut slot = value.trim().to_ascii_lowercase().replace('_', "-");
    if slot.chars().all(|character| character.is_ascii_digit()) && !slot.is_empty() {
        slot = format!("slot-{}", slot.parse::<u8>().unwrap_or_default());
    }
    if !matches!(slot.as_str(), "slot-1" | "slot-2" | "slot-3" | "slot-4") {
        return Err(ApiError::bad(
            "slot must be one of: slot-1, slot-2, slot-3, slot-4",
        ));
    }
    Ok(slot)
}

fn key(group_id: &str, slot: &str) -> String {
    format!("{group_id}::{slot}")
}
