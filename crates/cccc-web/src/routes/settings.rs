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
    let group = response
        .0
        .get("result")
        .and_then(|result| result.get("group"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let settings = projected_settings(&group);
    response.0 = json!({"ok":true,"result":{"settings":settings}});
    Ok(response)
}

fn projected_settings(group: &Value) -> Value {
    let mut stored = group.get("settings").cloned().unwrap_or_else(|| json!({}));
    if !stored.is_object() {
        stored = json!({});
    }
    if let Some(target) = stored.as_object_mut() {
        for (legacy_key, section, canonical_key) in [
            ("default_send_to", "messaging", "default_send_to"),
            ("min_interval_seconds", "delivery", "min_interval_seconds"),
            (
                "mail_notice_after_seconds",
                "delivery",
                "mail_notice_after_seconds",
            ),
            (
                "reply_notice_after_seconds",
                "delivery",
                "reply_notice_after_seconds",
            ),
            (
                "terminal_transcript_visibility",
                "terminal_transcript",
                "visibility",
            ),
            (
                "terminal_transcript_notify_tail",
                "terminal_transcript",
                "notify_tail",
            ),
            (
                "terminal_transcript_notify_lines",
                "terminal_transcript",
                "notify_lines",
            ),
            ("panorama_enabled", "features", "panorama_enabled"),
        ] {
            if let Some(value) = group
                .get(section)
                .and_then(Value::as_object)
                .and_then(|values| values.get(canonical_key))
            {
                target.insert(legacy_key.into(), value.clone());
            }
        }
    }
    if let (Some(target), Some(automation)) = (
        stored.as_object_mut(),
        group.get("automation").and_then(Value::as_object),
    ) {
        for key in cccc_core::group::AUTOMATION_TIMING_KEYS {
            if let Some(value) = automation.get(*key) {
                target.insert((*key).into(), value.clone());
            }
        }
    }
    let mut settings = json!({
        "default_send_to":"foreman",
        "actor_idle_timeout_seconds":0,
        "keepalive_delay_seconds":120,
        "keepalive_max_per_actor":3,
        "silence_timeout_seconds":0,
        "help_nudge_interval_seconds":600,
        "help_nudge_min_messages":10,
        "min_interval_seconds":0,
        "mail_notice_after_seconds":1800,
        "reply_notice_after_seconds":900,
        "terminal_transcript_visibility":"foreman",
        "terminal_transcript_notify_tail":true,
        "terminal_transcript_notify_lines":20,
        "panorama_enabled":false
    });
    if let (Some(target), Some(source)) = (settings.as_object_mut(), stored.as_object()) {
        cccc_core::settings::merge(target, source);
    }
    settings
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
        automation_update_args(group_id, body)?,
    )
    .await
}

fn automation_update_args(
    group_id: String,
    body: Value,
) -> Result<serde_json::Map<String, Value>, crate::api::ApiError> {
    let mut body = body_object(body)?;
    let rules = body.remove("rules").unwrap_or_else(|| json!([]));
    let snippets = body.remove("snippets").unwrap_or_else(|| json!({}));
    let by = body
        .remove("by")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "user".into());
    let mut args = object(json!({
        "group_id":group_id,
        "ruleset":{"rules":rules,"snippets":snippets},
        "by":by
    }));
    if let Some(expected_version) = body.remove("expected_version")
        && !expected_version.is_null()
    {
        args.insert("expected_version".into(), expected_version);
    }
    Ok(args)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_projection_prefers_semantic_sections_and_automation() {
        let settings = projected_settings(&json!({
            "settings": {
                "default_send_to":"broadcast",
                "min_interval_seconds":1,
                "native_extension":{"keep":true}
            },
            "messaging":{"default_send_to":"foreman"},
            "delivery":{"min_interval_seconds":42,"mail_notice_after_seconds":1801,"reply_notice_after_seconds":901},
            "terminal_transcript":{"visibility":"all","notify_tail":false,"notify_lines":37},
            "features":{"panorama_enabled":true}
        }));

        assert_eq!(settings["default_send_to"], json!("foreman"));
        assert_eq!(settings["min_interval_seconds"], json!(42));
        assert_eq!(settings["mail_notice_after_seconds"], json!(1801));
        assert_eq!(settings["reply_notice_after_seconds"], json!(901));
        assert_eq!(settings["terminal_transcript_visibility"], json!("all"));
        assert_eq!(settings["terminal_transcript_notify_tail"], json!(false));
        assert_eq!(settings["terminal_transcript_notify_lines"], json!(37));
        assert_eq!(settings["panorama_enabled"], json!(true));
        assert_eq!(settings["native_extension"], json!({"keep":true}));
    }

    #[test]
    fn automation_update_wraps_the_native_web_payload_in_the_daemon_contract() {
        let args = automation_update_args(
            "g_demo".into(),
            json!({
                "rules":[{"id":"standup"}],
                "snippets":{"standup":"check in"},
                "expected_version":7,
                "by":"user"
            }),
        )
        .expect("automation args");

        assert_eq!(args["group_id"], json!("g_demo"));
        assert_eq!(args["ruleset"]["rules"][0]["id"], json!("standup"));
        assert_eq!(args["ruleset"]["snippets"]["standup"], json!("check in"));
        assert_eq!(args["expected_version"], json!(7));
        assert_eq!(args["by"], json!("user"));
        assert!(args.get("patch").is_none());
    }
}
