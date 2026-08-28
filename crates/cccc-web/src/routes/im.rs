use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::utc_now;
use cccc_core::GroupStore;
use cccc_core::im_state;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::io;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::auth::Principal;

const PLATFORMS: &[&str] = &[
    "telegram", "slack", "discord", "feishu", "dingtalk", "wecom", "weixin",
];

#[derive(Debug, Deserialize)]
struct GroupQuery {
    group_id: String,
    #[serde(default)]
    chat_id: String,
    #[serde(default)]
    thread_id: String,
    #[serde(default)]
    verbose: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/im/status", get(status))
        .route("/api/im/config", get(config))
        .route("/api/im/set", post(set))
        .route("/api/im/unset", post(unset))
        .route("/api/im/start", post(start))
        .route("/api/im/stop", post(stop))
        .route("/api/im/weixin/login/status", get(weixin_status))
        .route("/api/im/weixin/login/start", post(weixin_start))
        .route("/api/im/weixin/login/verify", post(weixin_verify))
        .route("/api/im/weixin/logout", post(weixin_logout))
        .route("/api/im/authorized", get(authorized))
        .route("/api/im/pending", get(pending))
        .route("/api/im/bind", post(bind))
        .route("/api/im/pending/reject", post(reject))
        .route("/api/im/revoke", post(revoke))
        .route("/api/im/verbose", post(verbose))
}

async fn status(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    let mut value = load(&state, &query.group_id)?;
    reconcile_runtime_state(&state, &query.group_id, &mut value)?;
    Ok(success(status_payload(&query.group_id, &value)))
}

async fn config(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    let value = load(&state, &query.group_id)?;
    Ok(success(
        json!({"im":value.get("config").cloned().unwrap_or(Value::Null)}),
    ))
}

async fn set(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    ensure_access(&principal, &group_id)?;
    let platform = required(&body, "platform")?.to_ascii_lowercase();
    if !PLATFORMS.contains(&platform.as_str()) {
        return Err(ApiError::bad("unsupported IM platform"));
    }
    let mut config = body.as_object().cloned().unwrap_or_default();
    config.remove("group_id");
    normalize_config(&platform, &mut config)?;
    let current = load(&state, &group_id)?;
    preserve_config_policy(
        &platform,
        &mut config,
        current.get("config").and_then(Value::as_object),
    );
    update(&state, &group_id, |value| {
        let state = object(value);
        state.insert("config".into(), Value::Object(config.clone()));
        state.insert("enabled".into(), Value::Bool(false));
        state.insert("running".into(), Value::Bool(false));
        state.insert("updated_at".into(), Value::String(utc_now()));
        Ok(())
    })?;
    state.im_workers.stop(&group_id).await;
    Ok(success(json!({"configured":true,"platform":platform})))
}

async fn unset(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    ensure_access(&principal, &group_id)?;
    state.im_workers.stop(&group_id).await;
    update(&state, &group_id, |value| {
        *value = json!({});
        Ok(())
    })?;
    Ok(success(json!({"configured":false,"group_id":group_id})))
}

async fn start(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    set_running(&state, &principal, &body, true).await
}

async fn stop(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    set_running(&state, &principal, &body, false).await
}

async fn set_running(
    state: &AppState,
    principal: &Principal,
    body: &Value,
    running: bool,
) -> ApiResult {
    let group_id = required(body, "group_id")?;
    ensure_access(principal, &group_id)?;
    let current = load(state, &group_id)?;
    if running && !current.get("config").is_some_and(Value::is_object) {
        return Err(ApiError::bad("IM bridge is not configured"));
    }
    if running {
        let config = current
            .get("config")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::bad("IM bridge is not configured"))?;
        if let Err(error) = state
            .im_workers
            .start(state.home.clone(), state.client.clone(), &group_id, &config)
            .await
        {
            update(state, &group_id, |value| {
                let state = object(value);
                state.insert("enabled".into(), Value::Bool(true));
                state.insert("running".into(), Value::Bool(false));
                state.insert("pid".into(), Value::Null);
                state.insert("adapter_available".into(), Value::Bool(false));
                state.insert("last_error".into(), json!(error));
                state.insert("updated_at".into(), Value::String(utc_now()));
                Ok(())
            })?;
            return Err(ApiError::bad(error));
        }
        update(state, &group_id, |value| {
            let state = object(value);
            state.insert("enabled".into(), Value::Bool(true));
            state.insert("running".into(), Value::Bool(true));
            state.insert("pid".into(), json!(std::process::id()));
            state.insert("adapter_available".into(), Value::Bool(true));
            state.insert("last_error".into(), Value::Null);
            state.insert("updated_at".into(), Value::String(utc_now()));
            Ok(())
        })?;
        return Ok(success(status_payload(&group_id, &load(state, &group_id)?)));
    }
    state.im_workers.stop(&group_id).await;
    update(state, &group_id, |value| {
        let state = object(value);
        state.insert("enabled".into(), Value::Bool(false));
        state.insert("running".into(), Value::Bool(false));
        state.insert("pid".into(), Value::Null);
        state.insert("last_error".into(), Value::Null);
        state.insert("updated_at".into(), Value::String(utc_now()));
        Ok(())
    })?;
    Ok(success(status_payload(&group_id, &load(state, &group_id)?)))
}

fn reconcile_runtime_state(
    state: &AppState,
    group_id: &str,
    value: &mut Value,
) -> Result<(), ApiError> {
    if !value
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || state.im_workers.is_running(group_id)
    {
        return Ok(());
    }
    update(state, group_id, |stored| {
        mark_worker_stopped(object(stored));
        Ok(())
    })?;
    *value = load(state, group_id)?;
    Ok(())
}

fn mark_worker_stopped(stored: &mut Map<String, Value>) {
    stored.insert("running".into(), Value::Bool(false));
    stored.insert("pid".into(), Value::Null);
    stored.insert("adapter_available".into(), Value::Bool(false));
    if stored.get("last_error").is_none_or(Value::is_null) {
        stored.insert("last_error".into(), json!("IM network worker stopped"));
    }
    stored.insert("updated_at".into(), Value::String(utc_now()));
}

async fn weixin_status(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    let status = state
        .im_workers
        .weixin_login_status(&state.home, &query.group_id)
        .await
        .map_err(ApiError::bad)?;
    Ok(success(status))
}

async fn weixin_start(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    ensure_access(&principal, &group_id)?;
    let status = state
        .im_workers
        .start_weixin_login(&state.home, &group_id)
        .await
        .map_err(ApiError::bad)?;
    Ok(success(status))
}

async fn weixin_verify(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let verify_code = required(&body, "verify_code")?;
    ensure_access(&principal, &group_id)?;
    let status = state
        .im_workers
        .verify_weixin_login(&state.home, &group_id, &verify_code)
        .await
        .map_err(ApiError::bad)?;
    Ok(success(status))
}

async fn weixin_logout(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    ensure_access(&principal, &group_id)?;
    let status = state
        .im_workers
        .logout_weixin(&state.home, &group_id)
        .await
        .map_err(ApiError::bad)?;
    Ok(success(status))
}

async fn authorized(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    let value = load(&state, &query.group_id)?;
    let mut authorized = array_field(&value, "authorized");
    super::im_authorization::enrich_verbose(&mut authorized, &array_field(&value, "subscribers"));
    super::im_authorization::retain_active(&mut authorized);
    Ok(success(json!({"authorized":authorized})))
}

async fn pending(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    let now = chrono_now() as f64;
    let pending = update(&state, &query.group_id, |value| {
        let items = array_mut(object(value), "pending");
        items.retain(|item| item["expires_at"].as_f64().unwrap_or(0.0) > now);
        for item in items.iter_mut() {
            item["expires_in_seconds"] =
                json!((item["expires_at"].as_f64().unwrap_or(now) - now).max(0.0) as i64);
        }
        Ok(items.clone())
    })?;
    Ok(success(json!({"pending":pending})))
}

async fn bind(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let key = required(&body, "key")?;
    ensure_access(&principal, &group_id)?;
    let bound = update(&state, &group_id, |value| {
        let state = object(value);
        let pending = array_mut(state, "pending");
        let index = pending
            .iter()
            .position(|item| {
                item["key"] == key
                    && item["expires_at"].as_f64().unwrap_or(0.0) > chrono_now() as f64
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pending request not found"))?;
        let item = pending.remove(index);
        Ok(super::im_authorization::bind_authorized(state, item))
    })?;
    Ok(success(bound))
}

async fn reject(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let key = required(&body, "key")?;
    ensure_access(&principal, &group_id)?;
    let rejected = update(&state, &group_id, |value| {
        let items = array_mut(object(value), "pending");
        let before = items.len();
        items.retain(|item| item["key"] != key);
        Ok(items.len() != before)
    })?;
    Ok(success(json!({"rejected":rejected})))
}

async fn revoke(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    let (revoked, unsubscribed) = update(&state, &query.group_id, |value| {
        Ok(super::im_authorization::revoke(
            object(value),
            &query.chat_id,
            &query.thread_id,
        ))
    })?;
    Ok(success(
        json!({"revoked":revoked,"unsubscribed":unsubscribed}),
    ))
}

async fn verbose(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    let changed = update(&state, &query.group_id, |value| {
        super::im_authorization::set_verbose(
            object(value),
            &query.chat_id,
            &query.thread_id,
            query.verbose,
        )
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "authorized chat not found"))
    })?;
    Ok(success(changed))
}

fn normalize_config(platform: &str, config: &mut Map<String, Value>) -> Result<(), ApiError> {
    let normalized = im_state::canonicalize_config(platform, config)
        .ok_or_else(|| ApiError::bad("unsupported IM platform"))?;
    if !im_state::has_required_credentials(platform, &normalized) {
        return Err(ApiError::bad(format!("missing credentials for {platform}")));
    }
    *config = normalized;
    Ok(())
}

fn preserve_config_policy(
    platform: &str,
    config: &mut Map<String, Value>,
    previous: Option<&Map<String, Value>>,
) {
    for key in ["files"] {
        if !config.contains_key(key)
            && let Some(value) = previous.and_then(|previous| previous.get(key)).cloned()
        {
            config.insert(key.into(), value);
        }
    }
    config.entry("files").or_insert_with(|| {
        json!({
            "enabled":true,
            "max_mb":if matches!(platform, "telegram" | "slack") { 20 } else { 10 }
        })
    });
}

fn load(state: &AppState, group_id: &str) -> Result<Value, ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    im_state::load(&store, group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))
}

fn update<T>(
    state: &AppState,
    group_id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> Result<T, ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    im_state::update(&store, group_id, change).map_err(io_error)
}

fn status_payload(group_id: &str, value: &Value) -> Value {
    let config = value.get("config").filter(|value| value.is_object());
    json!({
        "group_id":group_id,"configured":config.is_some(),
        "enabled":value["enabled"].as_bool().unwrap_or(false),
        "platform":config.and_then(|value|value["platform"].as_str()).unwrap_or(""),
        "running":value["running"].as_bool().unwrap_or(false),
        "adapter_available":value["adapter_available"].as_bool().unwrap_or(false),
        "last_error":value.get("last_error").cloned().unwrap_or(Value::Null),
        "pid":value.get("pid").cloned().unwrap_or(Value::Null),
        "subscribers":array_field(value,"subscribers").into_iter()
            .filter(|item|item["subscribed"].as_bool().unwrap_or(true)).count()
    })
}

fn ensure_access(principal: &Principal, group_id: &str) -> Result<(), ApiError> {
    principal
        .allows(group_id)
        .then_some(())
        .ok_or_else(|| ApiError::forbidden("group access denied"))
}

fn object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("object initialized")
}

fn array_mut<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    let value = state.entry(key).or_insert_with(|| json!([]));
    if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("array initialized")
}

fn array_field(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn required(body: &Value, key: &str) -> Result<String, ApiError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::bad(format!("{key} is required")))
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_cli_credential_aliases() {
        for (platform, input, expected) in [
            (
                "telegram",
                json!({"token_env":"TELEGRAM_TOKEN"}),
                vec![("bot_token_env", "TELEGRAM_TOKEN")],
            ),
            (
                "feishu",
                json!({"app_key_env":"APP_ID","app_secret_env":"APP_SECRET"}),
                vec![
                    ("feishu_app_id_env", "APP_ID"),
                    ("feishu_app_secret_env", "APP_SECRET"),
                ],
            ),
            (
                "dingtalk",
                json!({"app_key_env":"APP_KEY","app_secret_env":"APP_SECRET"}),
                vec![
                    ("dingtalk_app_key_env", "APP_KEY"),
                    ("dingtalk_app_secret_env", "APP_SECRET"),
                ],
            ),
        ] {
            let mut config = input.as_object().cloned().expect("config");
            normalize_config(platform, &mut config).expect("normalize");
            for (key, value) in expected {
                assert_eq!(config[key], value);
            }
        }
    }

    #[test]
    fn config_updates_preserve_policy_and_apply_platform_default() {
        let previous = json!({
            "files":{"enabled":false,"max_mb":7},
            "skip_pending_on_start":false
        });
        let mut config = json!({"platform":"discord"})
            .as_object()
            .cloned()
            .expect("config");
        preserve_config_policy("discord", &mut config, previous.as_object());
        assert_eq!(config["files"]["max_mb"], 7);
        assert!(config.get("skip_pending_on_start").is_none());

        let mut fresh = json!({"platform":"discord"})
            .as_object()
            .cloned()
            .expect("config");
        preserve_config_policy("discord", &mut fresh, None);
        assert_eq!(fresh["files"], json!({"enabled":true,"max_mb":10}));
    }

    #[test]
    fn stopped_worker_preserves_specific_terminal_error() {
        let mut stored = json!({
            "running":true,
            "adapter_available":true,
            "last_error":"WeCom authentication failed: invalid secret"
        });
        mark_worker_stopped(stored.as_object_mut().expect("state"));
        assert_eq!(
            stored["last_error"],
            "WeCom authentication failed: invalid secret"
        );
        assert_eq!(stored["running"], false);
    }

    #[test]
    fn status_excludes_unsubscribed_weixin_tombstones() {
        let state = json!({
            "subscribers":[
                {"chat_id":"wx-old","platform":"weixin","subscribed":false},
                {"chat_id":"tg-live","platform":"telegram","subscribed":true}
            ]
        });

        assert_eq!(status_payload("group", &state)["subscribers"], 1);
    }
}
