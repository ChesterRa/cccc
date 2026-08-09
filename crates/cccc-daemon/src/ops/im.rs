use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout, settings};
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
        return delegate_start(home, &group_id).inspect_err(|error| {
            let _ = update(home, &group_id, |state| {
                state.insert("enabled".into(), Value::Bool(true));
                state.insert("running".into(), Value::Bool(false));
                state.insert("pid".into(), Value::Null);
                state.insert("adapter_available".into(), Value::Bool(false));
                state.insert("last_error".into(), json!(error.message));
                state.insert("updated_at".into(), json!(utc_now()));
                Ok(())
            });
        });
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

fn delegate_start(home: &HomeLayout, group_id: &str) -> OpResult {
    let global = settings::load(home).map_err(OpError::io)?;
    let host = global
        .remote_access
        .get("web_host")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("127.0.0.1");
    let host = if matches!(host, "0.0.0.0" | "::") {
        "127.0.0.1"
    } else {
        host
    };
    let port = global
        .remote_access
        .get("web_port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(8848);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(OpError::invalid)?;
    let mut request = client
        .post(format!("http://{}:{port}/api/im/start", url_host(host)))
        .json(&json!({"group_id":group_id}));
    if let Some(token) = AccessTokenStore::new(home.clone())
        .map_err(OpError::io)?
        .list()
        .map_err(OpError::io)?
        .into_iter()
        .find(|token| token.is_admin)
    {
        request = request.bearer_auth(token.token);
    }
    let response = request.send().map_err(|error| {
        OpError::new(
            "adapter_unavailable",
            format!("Rust IM network workers require the Web service; run `cccc` ({error})"),
        )
    })?;
    let status = response.status();
    let body = response.json::<Value>().map_err(|error| {
        OpError::new(
            "adapter_unavailable",
            format!("Rust Web returned an invalid IM response: {error}"),
        )
    })?;
    if !status.is_success() || body.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Rust Web rejected the IM start request");
        return Err(OpError::new("adapter_unavailable", message));
    }
    body.get("result")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| OpError::new("adapter_unavailable", "Rust Web returned no IM result"))
}

fn url_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
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
        "telegram" | "discord" => &["bot_token_env"],
        "slack" => &["bot_token_env", "app_token_env"],
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
        "subscribers":active_authorization_count(value)
    })
}

fn active_authorization_count(value: &Value) -> usize {
    let is_active = |item: &Value| item["subscribed"].as_bool() != Some(false);
    match value.get("authorized") {
        Some(Value::Array(items)) => items.iter().filter(|item| is_active(item)).count(),
        Some(Value::Object(items)) => items.values().filter(|item| is_active(item)).count(),
        _ => 0,
    }
}
fn bind(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let key = required_arg(request, "key")?;
    let now = chrono::Utc::now().timestamp() as f64;
    let item = update(home, &group_id, |state| {
        let pending = array(state, "pending");
        pending.retain(|item| item["expires_at"].as_f64().unwrap_or(0.0) > now);
        let Some(index) = pending.iter().position(|item| item["key"] == key) else {
            return Ok(None);
        };
        let item = pending.remove(index);
        array(state, "authorized").push(item.clone());
        Ok(Some(item))
    })?
    .ok_or_else(|| OpError::new("invalid_key", "pending request not found or expired"))?;
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
        .map(thread_id_value)
        .unwrap_or_default();
    let revoked = update(home, &group_id, |state| {
        let mut revoked = false;
        for key in ["authorized", "subscribers"] {
            if let Some(items) = state.get_mut(key) {
                revoked |= remove_chat_target(items, &chat_id, &thread_id);
            }
        }
        Ok(revoked)
    })?;
    object(json!({"group_id":group_id,"revoked":revoked}))
}

fn remove_chat_target(items: &mut Value, chat_id: &str, thread_id: &str) -> bool {
    match items {
        Value::Array(items) => {
            let mut changed = false;
            items.retain_mut(|item| {
                if !same_chat_target(item, chat_id, thread_id) {
                    return true;
                }
                if is_weixin_target(item) {
                    changed |= item["subscribed"].as_bool() != Some(false);
                    item["subscribed"] = Value::Bool(false);
                    true
                } else {
                    changed = true;
                    false
                }
            });
            changed
        }
        Value::Object(items) => {
            let mut changed = false;
            items.retain(|key, item| {
                if !legacy_chat_key_matches(key, chat_id, thread_id)
                    && !same_chat_target(item, chat_id, thread_id)
                {
                    return true;
                }
                if is_weixin_target(item) {
                    changed |= item["subscribed"].as_bool() != Some(false);
                    item["chat_id"] = json!(chat_id);
                    item["thread_id"] = if thread_id.is_empty() {
                        json!(0)
                    } else {
                        json!(thread_id)
                    };
                    item["subscribed"] = Value::Bool(false);
                    true
                } else {
                    changed = true;
                    false
                }
            });
            changed
        }
        _ => false,
    }
}

fn is_weixin_target(item: &Value) -> bool {
    item["platform"]
        .as_str()
        .is_some_and(|platform| platform.eq_ignore_ascii_case("weixin"))
}

fn same_chat_target(item: &Value, chat_id: &str, thread_id: &str) -> bool {
    item["chat_id"].as_str() == Some(chat_id)
        && thread_id_value(&item["thread_id"]) == normalize_thread_id(thread_id)
}

fn legacy_chat_key_matches(key: &str, chat_id: &str, thread_id: &str) -> bool {
    if !thread_id.is_empty() {
        key == format!("{chat_id}:{thread_id}")
    } else {
        key == chat_id
    }
}

fn thread_id_value(value: &Value) -> String {
    match value {
        Value::String(value) => normalize_thread_id(value),
        Value::Number(value) => normalize_thread_id(&value.to_string()),
        _ => String::new(),
    }
}

fn normalize_thread_id(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value == "0" {
        String::new()
    } else {
        value.to_owned()
    }
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
    if value.is_object() {
        let object = std::mem::take(value)
            .as_object()
            .cloned()
            .unwrap_or_default();
        *value = Value::Array(
            object
                .into_iter()
                .map(|(object_key, mut item)| {
                    let Some(fields) = item.as_object_mut() else {
                        return item;
                    };
                    if key == "pending" {
                        fields.entry("key").or_insert_with(|| json!(object_key));
                    } else {
                        let (chat_id, thread_id) = legacy_target_from_key(&object_key);
                        fields.entry("chat_id").or_insert_with(|| json!(chat_id));
                        fields.entry("thread_id").or_insert(thread_id);
                    }
                    item
                })
                .collect(),
        );
    } else if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("array initialized")
}

fn legacy_target_from_key(key: &str) -> (String, Value) {
    key.rsplit_once(':')
        .filter(|(chat_id, thread_id)| !chat_id.is_empty() && !thread_id.is_empty())
        .map_or_else(
            || (key.to_owned(), json!(0)),
            |(chat_id, thread_id)| (chat_id.to_owned(), json!(thread_id)),
        )
}

#[cfg(test)]
mod tests {
    use super::{KEY, bind, delegate_start, revoke, status_payload, url_host};
    use cccc_contracts::DaemonRequest;
    use cccc_core::{GroupStore, HomeLayout, integration_state, settings};
    use serde_json::json;
    use std::io::{Read, Write};

    #[test]
    fn web_url_brackets_ipv6_hosts() {
        assert_eq!(url_host("::1"), "[::1]");
        assert_eq!(url_host("[::1]"), "[::1]");
        assert_eq!(url_host("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn binding_rejects_expired_pending_keys() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("IM bind", "").expect("group");
        let future = chrono::Utc::now().timestamp() as f64 + 3_600.0;
        integration_state::group_update(&store, &group.group_id, KEY, |value| {
            *value = json!({"pending":[
                {"key":"expired","chat_id":"old","expires_at":0.0},
                {"key":"active","chat_id":"new","expires_at":future}
            ]});
            Ok(())
        })
        .expect("state");

        let request = |key: &str| DaemonRequest {
            v: 1,
            op: "im_bind_chat".into(),
            args: json!({"group_id":group.group_id,"key":key})
                .as_object()
                .cloned()
                .expect("args"),
        };

        let error = bind(&home, &request("expired")).expect_err("expired key must fail");
        assert_eq!(error.code, "invalid_key");
        assert!(error.message.contains("expired"));
        let state = integration_state::group_get(&store, &group.group_id, KEY).expect("state");
        assert_eq!(state["pending"].as_array().expect("pending").len(), 1);
        let result = bind(&home, &request("active")).expect("active key binds");
        assert_eq!(result["authorized"]["chat_id"], "new");
    }

    #[test]
    fn binding_preserves_legacy_object_authorizations() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("Legacy IM bind", "").expect("group");
        let future = chrono::Utc::now().timestamp() as f64 + 3_600.0;
        integration_state::group_update(&store, &group.group_id, KEY, |value| {
            *value = json!({
                "authorized":{
                    "old-chat":{"platform":"telegram"}
                },
                "pending":{"active":{
                    "chat_id":"new-chat","thread_id":"1710000000.100",
                    "platform":"slack","expires_at":future
                }}
            });
            Ok(())
        })
        .expect("state");
        let request = DaemonRequest {
            v: 1,
            op: "im_bind_chat".into(),
            args: json!({"group_id":group.group_id,"key":"active"})
                .as_object()
                .cloned()
                .expect("args"),
        };

        bind(&home, &request).expect("bind");

        let state = integration_state::group_get(&store, &group.group_id, KEY).expect("state");
        let authorized = state["authorized"].as_array().expect("authorized");
        assert_eq!(authorized.len(), 2);
        assert!(authorized.iter().any(|item| item["chat_id"] == "old-chat"));
        assert!(authorized.iter().any(|item| {
            item["chat_id"] == "new-chat" && item["thread_id"] == "1710000000.100"
        }));
    }

    #[test]
    fn revoke_removes_both_authorization_and_subscription_state() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("IM revoke", "").expect("group");
        integration_state::group_update(&store, &group.group_id, KEY, |value| {
            *value = json!({
                "authorized":[{"chat_id":"chat-1","thread_id":0}],
                "subscribers":[{"chat_id":"chat-1","thread_id":0,"subscribed":true}]
            });
            Ok(())
        })
        .expect("state");
        let request = DaemonRequest {
            v: 1,
            op: "im_revoke_chat".into(),
            args: json!({"group_id":group.group_id,"chat_id":"chat-1"})
                .as_object()
                .cloned()
                .expect("args"),
        };

        let result = revoke(&home, &request).expect("revoke");

        assert_eq!(result["revoked"], true);
        let state = integration_state::group_get(&store, &group.group_id, KEY).expect("state");
        assert!(
            state["authorized"]
                .as_array()
                .expect("authorized")
                .is_empty()
        );
        assert!(
            state["subscribers"]
                .as_array()
                .expect("subscribers")
                .is_empty()
        );
    }

    #[test]
    fn revoke_preserves_other_legacy_object_subscriptions() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("Legacy IM revoke", "").expect("group");
        integration_state::group_update(&store, &group.group_id, KEY, |value| {
            *value = json!({
                "authorized":{
                    "chat-1":{"chat_id":"chat-1","thread_id":0},
                    "chat-2":{"chat_id":"chat-2","thread_id":0}
                },
                "subscribers":{
                    "chat-1":{"thread_id":0,"subscribed":true},
                    "chat-2":{"thread_id":0,"subscribed":true,"verbose":true}
                }
            });
            Ok(())
        })
        .expect("state");
        let request = DaemonRequest {
            v: 1,
            op: "im_revoke_chat".into(),
            args: json!({"group_id":group.group_id,"chat_id":"chat-1"})
                .as_object()
                .cloned()
                .expect("args"),
        };

        let result = revoke(&home, &request).expect("revoke");

        assert_eq!(result["revoked"], true);
        let state = integration_state::group_get(&store, &group.group_id, KEY).expect("state");
        assert_eq!(
            state["authorized"],
            json!({"chat-2":{"chat_id":"chat-2","thread_id":0}})
        );
        assert_eq!(
            state["subscribers"],
            json!({"chat-2":{"thread_id":0,"subscribed":true,"verbose":true}})
        );
    }

    #[test]
    fn revoke_preserves_weixin_unsubscribe_tombstone() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("Weixin revoke", "").expect("group");
        integration_state::group_update(&store, &group.group_id, KEY, |value| {
            *value = json!({"authorized":[{
                "chat_id":"wx-user","thread_id":0,"platform":"weixin",
                "subscribed":true,"authorization_source":"weixin_login"
            }]});
            Ok(())
        })
        .expect("state");
        let request = DaemonRequest {
            v: 1,
            op: "im_revoke_chat".into(),
            args: json!({"group_id":group.group_id,"chat_id":"wx-user"})
                .as_object()
                .cloned()
                .expect("args"),
        };

        let result = revoke(&home, &request).expect("revoke");

        assert_eq!(result["revoked"], true);
        let state = integration_state::group_get(&store, &group.group_id, KEY).expect("state");
        assert_eq!(state["authorized"][0]["subscribed"], false);
        assert_eq!(status_payload(&group.group_id, &state)["subscribers"], 0);
    }

    #[test]
    fn daemon_im_start_delegates_to_the_web_owned_worker() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("address").port();
        let mut global = settings::load(&home).expect("settings");
        global.remote_access = json!({"web_host":"127.0.0.1","web_port":port})
            .as_object()
            .cloned()
            .expect("remote access");
        settings::save(&home, &global).expect("save settings");

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("read request");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("POST /api/im/start HTTP/1.1"));
            assert!(request.contains("\"group_id\":\"g_test\""));
            let body = r#"{"ok":true,"result":{"group_id":"g_test","running":true,"adapter_available":true}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write response");
        });

        let result = delegate_start(&home, "g_test").expect("delegated start");
        assert_eq!(result["running"], true);
        assert_eq!(result["adapter_available"], true);
        server.join().expect("server");
    }
}
