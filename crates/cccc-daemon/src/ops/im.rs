use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use std::io;

use crate::dispatch::{OpError, OpResult, object, required_arg};

const KEY: &str = "im_bridge";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "im_status" => status(home, request),
        "im_config" => config(home, request),
        "im_set" => set(home, request),
        "im_unset" => unset(home, request),
        "im_start" => running(home, request, true),
        "im_stop" => running(home, request, false),
        "im_bind_chat" => bind(home, request),
        "im_list_pending" => list(home, request, "pending"),
        "im_list_authorized" => list(home, request, "authorized"),
        "im_reject_pending" => reject(home, request),
        "im_revoke_chat" => revoke(home, request),
        _ => return None,
    })
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    object(status_payload(&group_id, &load(home, &group_id)?))
}

fn config(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let value = load(home, &group_id)?;
    object(json!({"group_id":group_id,"im":value.get("config").cloned().unwrap_or(Value::Null)}))
}

fn set(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let platform = required_arg(request, "platform")?.to_ascii_lowercase();
    if !matches!(
        platform.as_str(),
        "telegram" | "slack" | "discord" | "feishu" | "dingtalk" | "wecom" | "weixin"
    ) {
        return Err(OpError::new("invalid_args", "unsupported IM platform"));
    }
    let mut config: Map<String, Value> = request
        .args
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    normalize_config(&platform, &mut config)?;
    update(home, &group_id, |state| {
        state.insert("config".into(), Value::Object(config));
        state.insert("enabled".into(), Value::Bool(false));
        state.insert("running".into(), Value::Bool(false));
        state.insert("updated_at".into(), json!(utc_now()));
        Ok(())
    })?;
    object(json!({"group_id":group_id,"configured":true,"platform":platform}))
}

fn unset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    update(home, &group_id, |state| {
        state.clear();
        Ok(())
    })?;
    object(json!({"group_id":group_id,"configured":false}))
}

fn running(home: &HomeLayout, request: &DaemonRequest, running: bool) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let current = load(home, &group_id)?;
    if running && !current.get("config").is_some_and(Value::is_object) {
        return Err(OpError::new("invalid_state", "IM bridge is not configured"));
    }
    if running {
        update(home, &group_id, |state| {
            state.insert("enabled".into(), Value::Bool(true));
            state.insert("running".into(), Value::Bool(false));
            state.insert("pid".into(), Value::Null);
            state.insert("adapter_available".into(), Value::Bool(false));
            state.insert(
                "last_error".into(),
                json!("Rust network adapter is unavailable"),
            );
            state.insert("updated_at".into(), json!(utc_now()));
            Ok(())
        })?;
        return Err(OpError::new(
            "adapter_unavailable",
            "Rust network adapter is unavailable for the configured IM platform",
        ));
    }
    update(home, &group_id, |state| {
        state.insert("enabled".into(), Value::Bool(false));
        state.insert("running".into(), Value::Bool(false));
        state.insert("pid".into(), Value::Null);
        state.insert("last_error".into(), Value::Null);
        state.insert("updated_at".into(), json!(utc_now()));
        Ok(())
    })?;
    object(status_payload(&group_id, &load(home, &group_id)?))
}

fn normalize_config(platform: &str, config: &mut Map<String, Value>) -> Result<(), OpError> {
    config.insert("platform".into(), json!(platform));
    let aliases: &[(&str, &str)] = match platform {
        "telegram" | "discord" => &[("token_env", "bot_token_env")],
        "feishu" => &[
            ("app_key_env", "feishu_app_id"),
            ("app_secret_env", "feishu_app_secret"),
        ],
        "dingtalk" => &[
            ("app_key_env", "dingtalk_app_key"),
            ("app_secret_env", "dingtalk_app_secret"),
        ],
        _ => &[],
    };
    for (from, to) in aliases {
        if let Some(value) = config.get(*from).cloned().filter(non_empty) {
            config.entry(*to).or_insert(value);
        }
    }
    let required: &[&str] = match platform {
        "telegram" | "discord" | "slack" => &["bot_token_env"],
        "feishu" => &["feishu_app_id", "feishu_app_secret"],
        "dingtalk" => &["dingtalk_app_key", "dingtalk_app_secret"],
        "wecom" => &["wecom_bot_id", "wecom_secret"],
        "weixin" => &[],
        _ => unreachable!(),
    };
    if required
        .iter()
        .any(|key| config.get(*key).is_none_or(|value| !non_empty(value)))
    {
        return Err(OpError::new(
            "invalid_args",
            format!("missing credentials for {platform}"),
        ));
    }
    Ok(())
}

fn non_empty(value: &Value) -> bool {
    value.as_str().is_some_and(|value| !value.trim().is_empty())
}

fn status_payload(group_id: &str, value: &Value) -> Value {
    let config = value.get("config").filter(|value| value.is_object());
    json!({
        "group_id":group_id,
        "configured":config.is_some(),
        "enabled":value["enabled"].as_bool().unwrap_or(false),
        "platform":config.and_then(|value|value["platform"].as_str()).unwrap_or(""),
        "running":value["running"].as_bool().unwrap_or(false),
        "adapter_available":value["adapter_available"].as_bool().unwrap_or(false),
        "last_error":value.get("last_error").cloned().unwrap_or(Value::Null),
        "pid":value.get("pid").cloned().unwrap_or(Value::Null),
        "subscribers":value.get("authorized").and_then(Value::as_array).map_or(0,Vec::len)
    })
}
fn bind(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let key = required_arg(request, "key")?;
    let item = update(home, &group_id, |state| {
        let pending = array(state, "pending");
        let index = pending
            .iter()
            .position(|item| item["key"] == key)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pending request not found"))?;
        let item = pending.remove(index);
        array(state, "authorized").push(item.clone());
        Ok(item)
    })?;
    object(json!({"group_id":group_id,"authorized":item}))
}
fn list(home: &HomeLayout, request: &DaemonRequest, key: &str) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let value = load(home, &group_id)?;
    object(json!({"group_id":group_id,key:value.get(key).cloned().unwrap_or_else(||json!([]))}))
}
fn reject(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let key = required_arg(request, "key")?;
    let rejected = update(home, &group_id, |state| {
        let items = array(state, "pending");
        let before = items.len();
        items.retain(|item| item["key"] != key);
        Ok(items.len() != before)
    })?;
    object(json!({"group_id":group_id,"rejected":rejected}))
}
fn revoke(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let chat_id = required_arg(request, "chat_id")?;
    let thread_id = request
        .args
        .get("thread_id")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let revoked = update(home, &group_id, |state| {
        let items = array(state, "authorized");
        let before = items.len();
        items.retain(|item| {
            item["chat_id"] != chat_id || item["thread_id"].as_i64().unwrap_or(0) != thread_id
        });
        Ok(items.len() != before)
    })?;
    object(json!({"group_id":group_id,"revoked":revoked}))
}
fn load(home: &HomeLayout, group_id: &str) -> Result<Value, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_get(&store, group_id, KEY).map_err(OpError::io)
}
fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> Result<T, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_update(&store, group_id, KEY, |value| {
        if !value.is_object() {
            *value = json!({});
        }
        change(value.as_object_mut().expect("IM state initialized"))
    })
    .map_err(OpError::io)
}
fn array<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    let value = state.entry(key).or_insert_with(|| json!([]));
    if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("array initialized")
}
