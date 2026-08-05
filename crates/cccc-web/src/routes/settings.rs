use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/settings",
            get(settings_get).put(settings_update),
        )
        .route(
            "/api/v1/groups/{group_id}/automation",
            get(automation_get).put(automation_update),
        )
        .route(
            "/api/v1/groups/{group_id}/automation/manage",
            post(automation_manage),
        )
        .route(
            "/api/v1/groups/{group_id}/automation/reset_baseline",
            post(automation_reset),
        )
}

async fn settings_get(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    let mut response = call(&state, "group_show", object(json!({"group_id":group_id}))).await?;
    let stored = response
        .0
        .get("result")
        .and_then(|result| result.get("group"))
        .and_then(|group| group.get("settings"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut settings = json!({
        "nudge_after_seconds":300,
        "reply_required_nudge_after_seconds":300,
        "attention_ack_nudge_after_seconds":600,
        "unread_nudge_after_seconds":0,
        "nudge_digest_min_interval_seconds":120,
        "nudge_max_repeats_per_obligation":3,
        "nudge_escalate_after_repeats":2,
        "actor_idle_timeout_seconds":0,
        "keepalive_delay_seconds":120,
        "keepalive_max_per_actor":3,
        "silence_timeout_seconds":0,
        "help_nudge_interval_seconds":600,
        "help_nudge_min_messages":10,
        "min_interval_seconds":0,
        "auto_mark_on_delivery":true
    });
    if let (Some(target), Some(source)) = (settings.as_object_mut(), stored.as_object()) {
        cccc_core::settings::merge(target, source);
    }
    response.0 = json!({"ok":true,"result":{"settings":settings}});
    Ok(response)
}
async fn settings_update(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    call(
        &state,
        "group_settings_update",
        object(json!({"group_id":group_id,"patch":body,"by":"user"})),
    )
    .await
}
async fn automation_get(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(
        &state,
        "group_automation_state",
        object(json!({"group_id":group_id,"by":"user"})),
    )
    .await
}
async fn automation_update(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    call(
        &state,
        "group_automation_update",
        object(json!({"group_id":group_id,"patch":body,"by":"user"})),
    )
    .await
}
async fn automation_manage(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("by".into(), Value::String("user".into()));
    call(&state, "group_automation_manage", args).await
}
async fn automation_reset(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("by".into(), Value::String("user".into()));
    call(&state, "group_automation_reset_baseline", args).await
}
