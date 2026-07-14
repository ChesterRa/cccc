use axum::extract::{Extension, Query, State};
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
use crate::auth::Principal;

const STORE_KEY: &str = "im_bridge";
const PLATFORMS: &[&str] = &[
    "telegram", "slack", "discord", "feishu", "dingtalk", "wecom", "weixin",
];

#[derive(Debug, Deserialize)]
struct GroupQuery {
    group_id: String,
    #[serde(default)]
    chat_id: String,
    #[serde(default)]
    thread_id: i64,
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
    let value = load(&state, &query.group_id)?;
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
    update(&state, &group_id, |value| {
        let state = object(value);
        state.insert("config".into(), Value::Object(config.clone()));
        state.insert("enabled".into(), Value::Bool(false));
        state.insert("running".into(), Value::Bool(false));
        state.insert("updated_at".into(), Value::String(utc_now()));
        Ok(())
    })?;
    Ok(success(json!({"configured":true,"platform":platform})))
}

async fn unset(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    ensure_access(&principal, &group_id)?;
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
    set_running(&state, &principal, &body, true)
}

async fn stop(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    set_running(&state, &principal, &body, false)
}

fn set_running(state: &AppState, principal: &Principal, body: &Value, running: bool) -> ApiResult {
    let group_id = required(body, "group_id")?;
    ensure_access(principal, &group_id)?;
    let current = load(state, &group_id)?;
    if running && !current.get("config").is_some_and(Value::is_object) {
        return Err(ApiError::bad("IM bridge is not configured"));
    }
    if running {
        update(state, &group_id, |value| {
            let state = object(value);
            state.insert("enabled".into(), Value::Bool(true));
            state.insert("running".into(), Value::Bool(false));
            state.insert("pid".into(), Value::Null);
            state.insert("adapter_available".into(), Value::Bool(false));
            state.insert(
                "last_error".into(),
                json!("Rust network adapter is unavailable"),
            );
            state.insert("updated_at".into(), Value::String(utc_now()));
            Ok(())
        })?;
        return Err(ApiError::bad(
            "Rust network adapter is unavailable for the configured IM platform",
        ));
    }
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

async fn weixin_status(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    Ok(success(weixin_payload(&load(&state, &query.group_id)?)))
}

async fn weixin_start(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    ensure_access(&principal, &group_id)?;
    update(&state, &group_id, |value| {
        let state = object(value);
        let account = state
            .get("config")
            .and_then(|config| config.get("weixin_account_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        state.insert(
            "weixin_login".into(),
            json!({"status":if account.is_empty(){"waiting_qr"}else{"logged_in"},"logged_in":!account.is_empty(),"account_id":account,"running":true,"pid":std::process::id(),"updated_at":utc_now()}),
        );
        Ok(())
    })?;
    Ok(success(weixin_payload(&load(&state, &group_id)?)))
}

async fn weixin_logout(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    ensure_access(&principal, &group_id)?;
    update(&state, &group_id, |value| {
        object(value).insert(
            "weixin_login".into(),
            json!({"status":"logged_out","logged_in":false,"running":false,"pid":null,"updated_at":utc_now()}),
        );
        Ok(())
    })?;
    Ok(success(weixin_payload(&load(&state, &group_id)?)))
}

async fn authorized(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    Ok(success(
        json!({"authorized":array_field(&load(&state,&query.group_id)?,"authorized")}),
    ))
}

async fn pending(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    Ok(success(
        json!({"pending":array_field(&load(&state,&query.group_id)?,"pending")}),
    ))
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
            .position(|item| item["key"] == key)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pending request not found"))?;
        let mut item = pending.remove(index);
        item["authorized_at"] = json!(chrono_now());
        array_mut(state, "authorized").push(item.clone());
        Ok(item)
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
    let revoked = update(&state, &query.group_id, |value| {
        let items = array_mut(object(value), "authorized");
        let before = items.len();
        items.retain(|item| {
            item["chat_id"].as_str() != Some(&query.chat_id)
                || item["thread_id"].as_i64().unwrap_or(0) != query.thread_id
        });
        Ok(items.len() != before)
    })?;
    Ok(success(json!({"revoked":revoked})))
}

async fn verbose(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    ensure_access(&principal, &query.group_id)?;
    let changed = update(&state, &query.group_id, |value| {
        let item = array_mut(object(value), "authorized")
            .iter_mut()
            .find(|item| {
                item["chat_id"].as_str() == Some(&query.chat_id)
                    && item["thread_id"].as_i64().unwrap_or(0) == query.thread_id
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "authorized chat not found"))?;
        item["verbose"] = Value::Bool(query.verbose);
        Ok(item.clone())
    })?;
    Ok(success(changed))
}

fn normalize_config(platform: &str, config: &mut Map<String, Value>) -> Result<(), ApiError> {
    config.insert("platform".into(), Value::String(platform.into()));
    let required_fields: &[&str] = match platform {
        "telegram" | "discord" | "slack" => &["bot_token_env"],
        "feishu" => &["feishu_app_id", "feishu_app_secret"],
        "dingtalk" => &["dingtalk_app_key", "dingtalk_app_secret"],
        "wecom" => &["wecom_bot_id", "wecom_secret"],
        "weixin" => &[],
        _ => return Err(ApiError::bad("unsupported IM platform")),
    };
    if required_fields.iter().any(|key| {
        config
            .get(*key)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
    }) {
        return Err(ApiError::bad(format!("missing credentials for {platform}")));
    }
    Ok(())
}

fn load(state: &AppState, group_id: &str) -> Result<Value, ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    integration_state::group_get(&store, group_id, STORE_KEY)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))
}

fn update<T>(
    state: &AppState,
    group_id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> Result<T, ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    integration_state::group_update(&store, group_id, STORE_KEY, change).map_err(io_error)
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
        "subscribers":array_field(value,"authorized").len()
    })
}

fn weixin_payload(value: &Value) -> Value {
    value
        .get("weixin_login")
        .cloned()
        .unwrap_or_else(|| json!({"status":"idle","logged_in":false,"running":false,"pid":null}))
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
