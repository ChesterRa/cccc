use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::im_state;
use cccc_core::{GroupStore, HomeLayout, settings};
use serde_json::{Map, Value, json};
use std::io;

use crate::dispatch::{OpError, OpResult, object, required_arg};

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "im_status" => Operation::new(Read, status),
        "im_config" => Operation::new(Write, config),
        "im_set" => Operation::new(Write, set),
        "im_unset" => Operation::new(Write, unset),
        "im_start" => Operation::new(Write, |home, request| running(home, request, true)),
        "im_stop" => Operation::new(Write, |home, request| running(home, request, false)),
        "im_bind_chat" => Operation::new(Write, bind),
        "im_list_pending" => Operation::new(Read, |home, request| list(home, request, "pending")),
        "im_list_authorized" => {
            Operation::new(Read, |home, request| list(home, request, "authorized"))
        }
        "im_reject_pending" => Operation::new(Write, reject),
        "im_revoke_chat" => Operation::new(Write, revoke),
        _ => return None,
    })
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    object(status_payload(&group_id, &load(home, &group_id)?))
}

fn config(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    let value = load(home, &group_id)?;
    object(json!({"group_id":group_id,"im":value.get("config").cloned().unwrap_or(Value::Null)}))
}

fn set(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    let platform = required_arg(request, "platform")?.to_ascii_lowercase();
    if !matches!(
        platform.as_str(),
        "telegram"
            | "slack"
            | "discord"
            | "feishu"
            | "dingtalk"
            | "wecom"
            | "weixin"
            | "mattermost"
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
    let current = load(home, &group_id)?;
    let delegated = update(home, &group_id, |state| {
        // Check ownership and write under the same config lock so stale snapshots cannot bypass Web worker ownership.
        if platform == "mattermost" || web_owns_config(state.get("config")) {
            return Ok(true);
        }
        preserve_config_policy(
            &platform,
            &mut config,
            current.get("config").and_then(Value::as_object),
        );
        state.insert("config".into(), Value::Object(config.clone()));
        state.insert("enabled".into(), Value::Bool(false));
        state.insert("running".into(), Value::Bool(false));
        state.insert("updated_at".into(), json!(utc_now()));
        Ok(false)
    })?;
    if delegated {
        config.insert("group_id".into(), json!(group_id));
        let mut result = delegate_im_action(home, "set", &Value::Object(config))?;
        result.insert("group_id".into(), json!(group_id));
        return Ok(result);
    }
    object(json!({"group_id":group_id,"configured":true,"platform":platform}))
}

fn unset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    let delegated = update(home, &group_id, |state| {
        if web_owns_config(state.get("config")) {
            return Ok(true);
        }
        state.clear();
        Ok(false)
    })?;
    if delegated {
        return delegate_worker_action(home, &group_id, "unset");
    }
    object(json!({"group_id":group_id,"configured":false}))
}

fn running(home: &HomeLayout, request: &DaemonRequest, running: bool) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    let current = load(home, &group_id)?;
    if running && !current.get("config").is_some_and(Value::is_object) {
        return Err(OpError::new("invalid_state", "IM bridge is not configured"));
    }
    let web_owned = web_owns_config(current.get("config"));
    if running {
        return delegate_start(home, &group_id).inspect_err(|error| {
            let _ = update(home, &group_id, |state| {
                if web_owned || web_owns_config(state.get("config")) {
                    return Ok(());
                }
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
    delegate_worker_action(home, &group_id, "stop").inspect_err(|error| {
        let _ = update(home, &group_id, |state| {
            if web_owned || web_owns_config(state.get("config")) {
                return Ok(());
            }
            state.insert("enabled".into(), Value::Bool(false));
            state.insert("running".into(), Value::Bool(false));
            state.insert("pid".into(), Value::Null);
            state.insert("adapter_available".into(), Value::Bool(false));
            state.insert("last_error".into(), json!(error.message));
            state.insert("updated_at".into(), json!(utc_now()));
            Ok(())
        });
    })
}

fn web_owns_config(config: Option<&Value>) -> bool {
    config.and_then(|value| value["platform"].as_str()) == Some("mattermost")
}

fn delegate_start(home: &HomeLayout, group_id: &str) -> OpResult {
    delegate_worker_action(home, group_id, "start")
}

fn delegate_worker_action(home: &HomeLayout, group_id: &str, action: &str) -> OpResult {
    delegate_im_action(home, action, &json!({"group_id":group_id}))
}

fn delegate_im_action(home: &HomeLayout, action: &str, body: &Value) -> OpResult {
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
        .post(format!("http://{}:{port}/api/im/{action}", url_host(host)))
        .json(body);
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
            format!(
                "Rust IM network worker {action} requires the Web service; run `cccc` ({error})"
            ),
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
            .unwrap_or("Rust Web rejected the IM worker request");
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
    let normalized = im_state::canonicalize_config(platform, config)
        .ok_or_else(|| OpError::new("invalid_args", "unsupported IM platform"))?;
    if platform == "mattermost"
        && normalized
            .get("mattermost_url")
            .and_then(Value::as_str)
            .and_then(im_state::normalize_mattermost_url)
            .is_none()
    {
        return Err(OpError::new(
            "invalid_args",
            "Mattermost site URL is invalid",
        ));
    }
    if !im_state::has_required_credentials(platform, &normalized) {
        return Err(OpError::new(
            "invalid_args",
            format!("missing credentials for {platform}"),
        ));
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
        "subscribers":value.get("subscribers").and_then(Value::as_array).map_or(0,|items| {
            items.iter().filter(|item| item["subscribed"].as_bool().unwrap_or(true)).count()
        })
    })
}
fn bind(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    let key = required_im_arg(request, "key", "missing_key")?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let bound = im_state::update(&store, &group_id, |value| {
        let state = value
            .as_object_mut()
            .ok_or_else(|| io::Error::other("IM state is not an object"))?;
        let pending = array(state, "pending");
        let index = pending
            .iter()
            .position(|item| item["key"] == key)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))?;
        let item = pending.remove(index);
        let chat_id = item["chat_id"].as_str().unwrap_or("").to_owned();
        let thread_value = item.get("thread_id").cloned().unwrap_or_else(|| json!(0));
        let thread_id = thread_id_value(&thread_value);
        let platform = item["platform"].as_str().unwrap_or("").to_owned();
        if chat_id.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pending request has no chat_id",
            ));
        }

        let authorized = array(state, "authorized");
        authorized.retain(|item| !same_chat_target(item, &chat_id, &thread_id));
        authorized.push(json!({
            "chat_id":chat_id.clone(),
            "thread_id":thread_value.clone(),
            "platform":platform.clone(),
            "authorized_at":epoch_seconds(),
            "key_used":key
        }));

        let subscribers = array(state, "subscribers");
        if let Some(existing) = subscribers
            .iter_mut()
            .find(|item| same_chat_target(item, &chat_id, &thread_id))
        {
            existing["subscribed"] = Value::Bool(true);
            if existing["platform"].as_str().unwrap_or("").is_empty() {
                existing["platform"] = json!(platform.clone());
            }
        } else {
            subscribers.push(json!({
                "chat_id":chat_id.clone(),
                "thread_id":thread_value.clone(),
                "platform":platform.clone(),
                "subscribed":true,
                "verbose":false,
                "subscribed_at":utc_now(),
                "chat_title":""
            }));
        }
        Ok((chat_id, thread_value, platform))
    })
    .map_err(|error| {
        if error.kind() == io::ErrorKind::InvalidInput {
            OpError::new("invalid_key", "key not found or expired")
        } else if error.kind() == io::ErrorKind::NotFound {
            OpError::new("group_not_found", format!("group not found: {group_id}"))
        } else {
            OpError::io(error)
        }
    })?;
    object(json!({"chat_id":bound.0,"thread_id":bound.1,"platform":bound.2}))
}
fn list(home: &HomeLayout, request: &DaemonRequest, key: &str) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    let value = load(home, &group_id)?;
    object(json!({"group_id":group_id,key:value.get(key).cloned().unwrap_or_else(||json!([]))}))
}
fn reject(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    let key = required_im_arg(request, "key", "missing_key")?;
    let rejected = update(home, &group_id, |state| {
        let items = array(state, "pending");
        let before = items.len();
        items.retain(|item| item["key"] != key);
        Ok(items.len() != before)
    })?;
    object(json!({"group_id":group_id,"rejected":rejected}))
}
fn revoke(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_im_arg(request, "group_id", "missing_group_id")?;
    let chat_id = required_im_arg(request, "chat_id", "missing_chat_id")?;
    let thread_id = request
        .args
        .get("thread_id")
        .map(thread_id_value)
        .unwrap_or_default();
    let (revoked, unsubscribed) = update(home, &group_id, |state| {
        let mut revoked = false;
        array(state, "authorized").retain_mut(|item| {
            if !same_chat_target(item, &chat_id, &thread_id) {
                return true;
            }
            if is_weixin_target(item) {
                if item["subscribed"].as_bool() != Some(false) {
                    item["subscribed"] = Value::Bool(false);
                    revoked = true;
                }
                true
            } else {
                revoked = true;
                false
            }
        });
        let mut unsubscribed = false;
        for subscriber in array(state, "subscribers") {
            if same_chat_target(subscriber, &chat_id, &thread_id)
                && subscriber["subscribed"].as_bool().unwrap_or(true)
            {
                subscriber["subscribed"] = Value::Bool(false);
                unsubscribed = true;
            }
        }
        Ok((revoked, unsubscribed))
    })?;
    object(json!({"revoked":revoked,"unsubscribed":unsubscribed}))
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
    im_state::load(&store, group_id).map_err(|error| map_state_error(error, group_id))
}
fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> Result<T, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    im_state::update(&store, group_id, |value| {
        if !value.is_object() {
            *value = json!({});
        }
        change(value.as_object_mut().expect("IM state initialized"))
    })
    .map_err(|error| map_state_error(error, group_id))
}

fn required_im_arg(request: &DaemonRequest, name: &str, code: &str) -> Result<String, OpError> {
    request
        .args
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| OpError::new(code, format!("{name} is required")))
}

fn map_state_error(error: io::Error, group_id: &str) -> OpError {
    if error.kind() == io::ErrorKind::NotFound {
        OpError::new("group_not_found", format!("group not found: {group_id}"))
    } else {
        OpError::io(error)
    }
}

fn epoch_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
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
    use super::{
        bind, delegate_start, normalize_config, preserve_config_policy, revoke, running, set,
        status_payload, unset, url_host,
    };
    use cccc_contracts::DaemonRequest;
    use cccc_core::{GroupStore, HomeLayout, im_state, settings};
    use serde_json::{Value, json};
    use std::io::{Read, Write};

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        // Accepted sockets may inherit nonblocking mode on Windows; normalize it before reading the full HTTP request with a timeout.
        stream.set_nonblocking(false).expect("blocking request");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .expect("request timeout");
        let mut bytes = [0_u8; 4096];
        let mut used = 0;
        loop {
            let count = stream.read(&mut bytes[used..]).expect("read request");
            assert!(count > 0, "incomplete or oversized fixture request");
            used += count;
            if let Some(end) = bytes[..used]
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
            {
                let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let length = header
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .expect("fixture content length")
                    .trim()
                    .parse::<usize>()
                    .expect("valid content length");
                if used >= end + 4 + length {
                    return String::from_utf8(bytes[..used].to_vec()).expect("request utf8");
                }
            }
        }
    }

    #[test]
    fn http_fixture_waits_for_delayed_headers_and_body() {
        for nonblocking in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
            let address = listener.local_addr().expect("address");
            let mut client = std::net::TcpStream::connect(address).expect("connect");
            let (mut stream, _) = listener.accept().expect("accept queued connection");
            // Connect may finish before a nonblocking accept is ready. Exercise the
            // reader's socket-mode normalization without depending on handshake timing.
            stream.set_nonblocking(nonblocking).expect("mode");
            let reader = std::thread::spawn(move || read_http_request(&mut stream));
            // Connect first, then send the request in fragments; one read does not constitute a complete HTTP request.
            std::thread::sleep(std::time::Duration::from_millis(20));
            client
                .write_all(b"POST /api/im/stop HTTP/1.1\r\nContent-Length: 2\r\n\r\n")
                .expect("headers");
            std::thread::sleep(std::time::Duration::from_millis(20));
            client.write_all(b"{}").expect("body");
            assert_eq!(
                reader.join().expect("reader"),
                "POST /api/im/stop HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}"
            );
        }
    }

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
        let now = chrono::Utc::now().timestamp() as f64;
        im_state::update(&store, &group.group_id, |value| {
            *value = json!({"pending":[
                {"key":"expired","chat_id":"old","created_at":0.0},
                {"key":"active","chat_id":"new","created_at":now}
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
        let state = im_state::load(&store, &group.group_id).expect("state");
        assert_eq!(state["pending"].as_array().expect("pending").len(), 1);
        let result = bind(&home, &request("active")).expect("active key binds");
        assert_eq!(result["chat_id"], "new");
    }

    #[test]
    fn binding_preserves_legacy_object_authorizations() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("Legacy IM bind", "").expect("group");
        let now = chrono::Utc::now().timestamp() as f64;
        im_state::update(&store, &group.group_id, |value| {
            *value = json!({
                "authorized":{
                    "old-chat":{"platform":"telegram"}
                },
                "pending":{"active":{
                    "chat_id":"new-chat","thread_id":"1710000000.100",
                    "platform":"slack","created_at":now
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

        let state = im_state::load(&store, &group.group_id).expect("state");
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
        im_state::update(&store, &group.group_id, |value| {
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
        let state = im_state::load(&store, &group.group_id).expect("state");
        assert!(
            state["authorized"]
                .as_array()
                .expect("authorized")
                .is_empty()
        );
        assert_eq!(state["subscribers"][0]["subscribed"], false);
    }

    #[test]
    fn revoke_preserves_other_legacy_object_subscriptions() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("Legacy IM revoke", "").expect("group");
        im_state::update(&store, &group.group_id, |value| {
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
        let state = im_state::load(&store, &group.group_id).expect("state");
        assert_eq!(state["authorized"].as_array().expect("authorized").len(), 1);
        assert_eq!(state["authorized"][0]["chat_id"], "chat-2");
        let subscribers = state["subscribers"].as_array().expect("subscribers");
        assert_eq!(subscribers.len(), 2);
        assert!(
            subscribers
                .iter()
                .any(|item| { item["chat_id"] == "chat-1" && item["subscribed"] == false })
        );
        assert!(
            subscribers
                .iter()
                .any(|item| { item["chat_id"] == "chat-2" && item["verbose"] == true })
        );
    }

    #[test]
    fn revoke_preserves_weixin_unsubscribe_tombstone() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("Weixin revoke", "").expect("group");
        im_state::update(&store, &group.group_id, |value| {
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
        let state = im_state::load(&store, &group.group_id).expect("state");
        assert_eq!(state["authorized"][0]["subscribed"], false);
        assert_eq!(status_payload(&group.group_id, &state)["subscribers"], 0);
    }

    #[test]
    fn daemon_config_uses_canonical_credentials_and_preserves_policy() {
        let mut config = json!({
            "app_key_env":"FEISHU_APP_ID",
            "app_secret_env":"raw-secret"
        })
        .as_object()
        .cloned()
        .expect("config");
        normalize_config("feishu", &mut config).expect("normalize");
        preserve_config_policy(
            "feishu",
            &mut config,
            json!({"files":{"enabled":false,"max_mb":7},"skip_pending_on_start":false}).as_object(),
        );
        assert_eq!(config["feishu_app_id_env"], "FEISHU_APP_ID");
        assert_eq!(config["feishu_app_secret"], "raw-secret");
        assert_eq!(config["files"]["max_mb"], 7);
        assert!(config.get("skip_pending_on_start").is_none());
        assert!(!config.contains_key("app_key_env"));
    }

    #[test]
    fn mattermost_config_reports_site_errors_without_echoing_input() {
        for (raw, message) in [
            (
                json!({"bot_token":"test-token"}),
                "Mattermost site URL is invalid",
            ),
            (
                json!({"bot_token":"test-token","mattermost_url":"https://user:secret@mm.example.test"}),
                "Mattermost site URL is invalid",
            ),
            (
                json!({"mattermost_url":"https://mm.example.test"}),
                "missing credentials for mattermost",
            ),
        ] {
            let mut config = raw.as_object().expect("object").clone();
            let error = normalize_config("mattermost", &mut config).expect_err("invalid");
            assert_eq!(error.code, "invalid_args");
            assert_eq!(error.message, message);
            assert_eq!(Value::Object(config), raw);
        }
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
            let request = read_http_request(&mut stream);
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

    #[test]
    fn daemon_im_stop_delegates_to_the_web_owned_worker() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("IM stop", "").expect("group");
        im_state::update(&store, &group.group_id, |value| {
            *value = json!({
                "config":{"platform":"weixin"},
                "enabled":true,
                "running":true,
                "adapter_available":true
            });
            Ok(())
        })
        .expect("state");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("address").port();
        let mut global = settings::load(&home).expect("settings");
        global.remote_access = json!({"web_host":"127.0.0.1","web_port":port})
            .as_object()
            .cloned()
            .expect("remote access");
        settings::save(&home, &global).expect("save settings");

        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = read_http_request(&mut stream);
                        let body = r#"{"ok":true,"result":{"group_id":"g_test","running":false,"adapter_available":false}}"#;
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .expect("write response");
                        return request;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            }
            String::new()
        });

        let request = DaemonRequest {
            v: 1,
            op: "im_stop".into(),
            args: json!({"group_id":group.group_id})
                .as_object()
                .cloned()
                .expect("args"),
        };
        let result = running(&home, &request, false).expect("delegated stop");
        assert_eq!(result["running"], false);
        let observed = server.join().expect("server");
        assert!(observed.starts_with("POST /api/im/stop HTTP/1.1"));
        assert!(observed.contains(&format!("\"group_id\":\"{}\"", group.group_id)));
    }

    fn mock_management_web(
        home: &HomeLayout,
        respond: impl FnOnce(&str) -> Value + Send + 'static,
    ) -> std::thread::JoinHandle<String> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        listener.set_nonblocking(true).expect("nonblocking");
        let mut global = settings::load(home).expect("settings");
        global.remote_access = json!({"web_host":"127.0.0.1","web_port":listener.local_addr().expect("address").port()})
            .as_object().cloned().expect("remote access");
        settings::save(home, &global).expect("settings");
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = read_http_request(&mut stream);
                        let body = respond(&request).to_string();
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).expect("response");
                        return request;
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("expected delegated management request: {error}"),
                }
            }
        })
    }

    #[test]
    fn mattermost_config_changes_delegate_complete_payload_without_local_state_writes() {
        for (from, target) in [
            ("telegram", "mattermost"),
            ("mattermost", "mattermost"),
            ("mattermost", "slack"),
            ("mattermost", "unset"),
        ] {
            let temp = tempfile::tempdir().expect("temp");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            home.initialize().expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let group_id = store.create("IM delegation", "").expect("group").group_id;
            im_state::update(&store, &group_id, |value| {
                *value = json!({"config":{"platform":from,"bot_token_env":"OLD_TOKEN","mattermost_url":"https://old.example.test"},"enabled":true,"running":true,"adapter_available":true,"pid":42,"last_error":"unchanged"});
                Ok(())
            }).expect("state");
            let before = im_state::load(&store, &group_id).expect("before");
            let expected_id = group_id.clone();
            let server = mock_management_web(&home, move |request| {
                let action = if target == "unset" { "unset" } else { "set" };
                assert!(request.starts_with(&format!("POST /api/im/{action} HTTP/1.1")));
                let payload: Value =
                    serde_json::from_str(request.split_once("\r\n\r\n").expect("body").1)
                        .expect("json");
                assert_eq!(payload["group_id"], expected_id);
                assert!(payload.get("by").is_none());
                if target != "unset" {
                    assert_eq!(payload["platform"], target);
                    assert_eq!(payload["bot_token_env"], "NEW_TOKEN");
                    assert_eq!(payload["files"]["enabled"], false);
                    if target == "mattermost" {
                        assert_eq!(payload["mattermost_url"], "https://new.example.test/chat");
                    } else {
                        assert_eq!(payload["app_token_env"], "APP_TOKEN");
                    }
                }
                json!({"ok":true,"result":{"group_id":expected_id,"configured":target != "unset","platform":target}})
            });
            let request = DaemonRequest { v: 1, op: "im_set".into(), args: json!({"group_id":group_id,"by":"user","platform":target,"bot_token_env":"NEW_TOKEN","app_token_env":"APP_TOKEN","mattermost_url":"https://new.example.test/chat/","files":{"enabled":false,"max_mb":3}}).as_object().cloned().expect("args") };
            let result = if target == "unset" {
                unset(&home, &request)
            } else {
                set(&home, &request)
            }
            .expect("delegate");
            server.join().expect("server");
            assert_eq!(result["group_id"], group_id);
            assert_eq!(result["configured"], target != "unset");
            // The mock Web server only acknowledges the request; the daemon must not independently change config or runtime state.
            assert_eq!(im_state::load(&store, &group_id).expect("after"), before);
        }
    }

    #[test]
    fn mattermost_delegation_errors_preserve_web_state_and_legacy_errors_keep_old_behavior() {
        for (from, to) in [
            ("mattermost", "mattermost"),
            ("mattermost", "telegram"),
            ("telegram", "mattermost"),
            ("telegram", "telegram"),
        ] {
            for action in ["start", "stop"] {
                let temp = tempfile::tempdir().expect("temp");
                let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
                home.initialize().expect("home");
                let store = GroupStore::new(home.clone()).expect("store");
                let group_id = store.create("IM failure", "").expect("group").group_id;
                let initial = json!({"config":{"platform":from,"bot_token_env":"TOKEN","mattermost_url":"https://mm.example.test"},"enabled":false,"running":true,"adapter_available":true,"pid":42,"last_error":"new owner"});
                im_state::update(&store, &group_id, |value| {
                    *value = initial.clone();
                    Ok(())
                })
                .expect("state");
                let mut replacement = initial;
                replacement["config"]["platform"] = json!(to);
                let expected = replacement.clone();
                let server_home = home.clone();
                let server_group = group_id.clone();
                let server = mock_management_web(&home, move |_| {
                    let store = GroupStore::new(server_home).expect("store");
                    im_state::update(&store, &server_group, |value| {
                        *value = replacement;
                        Ok(())
                    })
                    .expect("replacement");
                    json!({"ok":false,"error":{"code":"fixture","message":"Web rejected"}})
                });
                let request = DaemonRequest {
                    v: 1,
                    op: format!("im_{action}"),
                    args: json!({"group_id":group_id})
                        .as_object()
                        .cloned()
                        .expect("args"),
                };
                let error = running(&home, &request, action == "start").expect_err("rejected");
                assert_eq!(error.code, "adapter_unavailable");
                assert_eq!(error.message, "Web rejected");
                server.join().expect("server");
                let actual = im_state::load(&store, &group_id).expect("after");
                if from == "mattermost" || to == "mattermost" {
                    // Normalize the expected value through the same entry point to include native defaults.
                    im_state::update(&store, &group_id, |value| {
                        *value = expected;
                        Ok(())
                    })
                    .expect("expected");
                    assert_eq!(
                        actual,
                        im_state::load(&store, &group_id).expect("normalized expected")
                    );
                } else {
                    assert_eq!(actual["enabled"], action == "start");
                    assert_eq!(actual["running"], false);
                    assert_eq!(actual["adapter_available"], false);
                    assert_eq!(actual["pid"], Value::Null);
                    assert_eq!(actual["last_error"], "Web rejected");
                }
            }
        }
    }

    #[test]
    fn mattermost_set_and_unset_fail_closed_without_web_but_legacy_config_stays_local() {
        for platform in ["mattermost", "telegram"] {
            let temp = tempfile::tempdir().expect("temp");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            home.initialize().expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let group_id = store.create("IM no Web", "").expect("group").group_id;
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("unused Web address");
            let mut global = settings::load(&home).expect("settings");
            global.remote_access = json!({"web_host":"127.0.0.1","web_port":listener.local_addr().expect("address").port()}).as_object().cloned().expect("remote");
            settings::save(&home, &global).expect("settings");
            drop(listener);
            im_state::update(&store, &group_id, |value| { *value = json!({"config":{"platform":platform,"bot_token_env":"OLD","mattermost_url":"https://old.example.test"},"running":true}); Ok(()) }).expect("state");
            let before = im_state::load(&store, &group_id).expect("before");
            let request = DaemonRequest { v:1, op:"im_set".into(), args:json!({"group_id":group_id,"platform":platform,"bot_token_env":"NEW","mattermost_url":"https://new.example.test"}).as_object().cloned().expect("args") };
            let saved = set(&home, &request);
            if platform == "mattermost" {
                assert_eq!(saved.expect_err("no Web").code, "adapter_unavailable");
                assert_eq!(
                    im_state::load(&store, &group_id).expect("after save"),
                    before
                );
                assert_eq!(
                    unset(&home, &request).expect_err("no Web").code,
                    "adapter_unavailable"
                );
                assert_eq!(
                    im_state::load(&store, &group_id).expect("after unset"),
                    before
                );
            } else {
                assert_eq!(saved.expect("local save")["configured"], true);
                assert_eq!(
                    im_state::load(&store, &group_id).expect("saved")["config"]["bot_token_env"],
                    "NEW"
                );
                assert_eq!(
                    unset(&home, &request).expect("local unset")["configured"],
                    false
                );
                assert!(im_state::load(&store, &group_id).expect("cleared")["config"].is_null());
            }
        }
    }
}
