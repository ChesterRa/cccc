use serde_json::{Map, Value, json};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fs::{read_json, with_exclusive_lock, write_json_committed};
use crate::{GroupDoc, GroupStore};

const CONFIG_KEY: &str = "im";
const RUST_SHADOW_KEY: &str = "im_bridge";
const PENDING_TTL_SECONDS: f64 = 600.0;
const DURABLE_KEYS: &[&str] = &["config", "enabled", "authorized", "pending", "subscribers"];
const SUPPORTED_PLATFORMS: &[&str] = &[
    "telegram", "slack", "discord", "feishu", "dingtalk", "wecom", "weixin",
];
const CONFIG_INPUT_KEYS: &[&str] = &[
    "token_env",
    "token",
    "bot_token_env",
    "bot_token",
    "app_token_env",
    "app_token",
    "app_key_env",
    "app_secret_env",
    "domain",
    "robot_code_env",
    "robot_code",
    "feishu_domain",
    "feishu_app_id",
    "feishu_app_id_env",
    "feishu_app_secret",
    "feishu_app_secret_env",
    "dingtalk_app_key",
    "dingtalk_app_key_env",
    "dingtalk_app_secret",
    "dingtalk_app_secret_env",
    "dingtalk_robot_code",
    "dingtalk_robot_code_env",
    "wecom_bot_id",
    "wecom_bot_id_env",
    "wecom_secret",
    "wecom_secret_env",
    "wecom_agent_id",
    "weixin_account_id",
    "weixin_command",
];

#[derive(Clone, Copy)]
enum ItemKind {
    Authorized,
    Pending,
    Subscriber,
}

/// Normalize accepted IM configuration inputs to the stable persisted shape.
///
/// Environment-variable references use `*_env`; literal credentials use the
/// corresponding value key. Rust-only CLI aliases are accepted as input but
/// never persisted. Unknown non-credential extension fields are preserved.
pub fn canonicalize_config(platform: &str, raw: &Map<String, Value>) -> Option<Map<String, Value>> {
    let platform = platform.trim().to_ascii_lowercase();
    if !SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
        return None;
    }

    let mut output = Map::new();
    output.insert("platform".into(), json!(platform));
    if let Some(value) = raw.get("enabled") {
        output.insert("enabled".into(), Value::Bool(coerce_bool(value, false)));
    }
    if let Some(files) = raw.get("files").and_then(Value::as_object) {
        output.insert("files".into(), Value::Object(files.clone()));
    }
    match platform.as_str() {
        "telegram" | "discord" | "slack" => {
            set_secret_ref(
                &mut output,
                "bot_token_env",
                "bot_token",
                first_string(raw, &["bot_token_env", "bot_token", "token_env", "token"]),
            );
            if platform == "slack" {
                set_secret_ref(
                    &mut output,
                    "app_token_env",
                    "app_token",
                    first_string(raw, &["app_token_env", "app_token"]),
                );
            }
        }
        "feishu" => {
            if let Some(domain) = first_string(raw, &["feishu_domain", "domain"])
                .map(normalize_feishu_domain)
                .filter(|value| !value.is_empty())
            {
                output.insert("feishu_domain".into(), json!(domain));
            }
            set_secret_ref(
                &mut output,
                "feishu_app_id_env",
                "feishu_app_id",
                first_string(raw, &["feishu_app_id_env", "feishu_app_id", "app_key_env"]),
            );
            set_secret_ref(
                &mut output,
                "feishu_app_secret_env",
                "feishu_app_secret",
                first_string(
                    raw,
                    &[
                        "feishu_app_secret_env",
                        "feishu_app_secret",
                        "app_secret_env",
                    ],
                ),
            );
        }
        "dingtalk" => {
            set_secret_ref(
                &mut output,
                "dingtalk_app_key_env",
                "dingtalk_app_key",
                first_string(
                    raw,
                    &["dingtalk_app_key_env", "dingtalk_app_key", "app_key_env"],
                ),
            );
            set_secret_ref(
                &mut output,
                "dingtalk_app_secret_env",
                "dingtalk_app_secret",
                first_string(
                    raw,
                    &[
                        "dingtalk_app_secret_env",
                        "dingtalk_app_secret",
                        "app_secret_env",
                    ],
                ),
            );
            set_secret_ref(
                &mut output,
                "dingtalk_robot_code_env",
                "dingtalk_robot_code",
                first_string(
                    raw,
                    &[
                        "dingtalk_robot_code_env",
                        "dingtalk_robot_code",
                        "robot_code_env",
                        "robot_code",
                    ],
                ),
            );
        }
        "wecom" => {
            set_secret_ref(
                &mut output,
                "wecom_bot_id_env",
                "wecom_bot_id",
                first_string(raw, &["wecom_bot_id_env", "wecom_bot_id"]),
            );
            set_secret_ref(
                &mut output,
                "wecom_secret_env",
                "wecom_secret",
                first_string(raw, &["wecom_secret_env", "wecom_secret"]),
            );
        }
        "weixin" => {
            if let Some(account_id) = first_string(raw, &["weixin_account_id"]) {
                output.insert("weixin_account_id".into(), json!(account_id));
            }
        }
        _ => unreachable!("supported IM platform"),
    }

    for (key, value) in raw {
        if output.contains_key(key)
            || CONFIG_INPUT_KEYS.contains(&key.as_str())
            || matches!(
                key.as_str(),
                "platform" | "enabled" | "files" | "skip_pending_on_start" | "group_id" | "by"
            )
        {
            continue;
        }
        output.insert(key.clone(), value.clone());
    }
    Some(output)
}

/// Whether every credential required to start the selected platform is present.
/// Each credential may be a literal value or an environment-variable reference.
pub fn has_required_credentials(platform: &str, config: &Map<String, Value>) -> bool {
    let has = |value_key: &str, env_key: &str| {
        [value_key, env_key].into_iter().any(|key| {
            config
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        })
    };
    match platform.trim().to_ascii_lowercase().as_str() {
        "telegram" | "discord" => has("bot_token", "bot_token_env"),
        "slack" => has("bot_token", "bot_token_env") && has("app_token", "app_token_env"),
        "feishu" => {
            has("feishu_app_id", "feishu_app_id_env")
                && has("feishu_app_secret", "feishu_app_secret_env")
        }
        "dingtalk" => {
            has("dingtalk_app_key", "dingtalk_app_key_env")
                && has("dingtalk_app_secret", "dingtalk_app_secret_env")
        }
        "wecom" => {
            has("wecom_bot_id", "wecom_bot_id_env") && has("wecom_secret", "wecom_secret_env")
        }
        "weixin" => true,
        _ => false,
    }
}

/// Load the language-neutral IM state as the composite envelope expected by
/// Rust daemon/Web/runtime code.
///
/// Durable product state remains in the stable 0.4.35 layout:
/// `group.extra.im` plus the three purpose-specific JSON files. The historical
/// `im_bridge` object contributes only runtime diagnostics after a bounded
/// missing-state import.
pub fn load(store: &GroupStore, group_id: &str) -> io::Result<Value> {
    store.load(group_id)?;
    let state_dir = store.state_dir(group_id)?;
    with_exclusive_lock(&state_dir.join("im_state.lock"), || {
        import_missing_shadow_state(store, group_id)?;
        let group = store.load(group_id)?;
        load_from_group(store, group_id, &group)
    })
}

/// Mutate the composite IM envelope and project it back to the canonical state
/// classes while holding the shared group lock.
pub fn update<T>(
    store: &GroupStore,
    group_id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> io::Result<T> {
    store.load(group_id)?;
    let state_dir = store.state_dir(group_id)?;
    with_exclusive_lock(&state_dir.join("im_state.lock"), || {
        import_missing_shadow_state(store, group_id)?;
        store.mutate(group_id, |group| {
            let mut state = load_from_paths(group, &state_dir);
            let result = change(&mut state)?;
            persist(group, &state_dir, &state)?;
            Ok(result)
        })
    })
}

fn import_missing_shadow_state(store: &GroupStore, group_id: &str) -> io::Result<()> {
    let group = store.load(group_id)?;
    let Some(shadow) = group.extra.get(RUST_SHADOW_KEY).and_then(Value::as_object) else {
        return Ok(());
    };
    if !DURABLE_KEYS.iter().any(|key| shadow.contains_key(*key)) {
        return Ok(());
    }
    let state_dir = store.state_dir(group_id)?;

    store.mutate(group_id, |group| {
        let shadow = group
            .extra
            .get(RUST_SHADOW_KEY)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if !group.extra.get(CONFIG_KEY).is_some_and(Value::is_object)
            && let Some(mut config) = shadow.get("config").and_then(Value::as_object).cloned()
        {
            if let Some(enabled) = shadow.get("enabled").and_then(Value::as_bool) {
                config.insert("enabled".into(), Value::Bool(enabled));
            }
            let platform = config
                .get("platform")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(config) = canonicalize_config(platform, &config) {
                group.extra.insert(CONFIG_KEY.into(), Value::Object(config));
            }
        }
        for (key, kind) in [
            ("authorized", ItemKind::Authorized),
            ("pending", ItemKind::Pending),
            ("subscribers", ItemKind::Subscriber),
        ] {
            let path = state_path(&state_dir, key);
            if path.exists() {
                continue;
            }
            let Some(value) = shadow.get(key) else {
                continue;
            };
            if !value.is_array() && !value.is_object() {
                continue;
            }
            let items = normalize_items(Some(value), kind, epoch_seconds());
            write_items(&path, &items, kind)?;
        }
        let remove_shadow = if let Some(runtime) = group
            .extra
            .get_mut(RUST_SHADOW_KEY)
            .and_then(Value::as_object_mut)
        {
            for key in DURABLE_KEYS {
                runtime.remove(*key);
            }
            runtime.is_empty()
        } else {
            false
        };
        if remove_shadow {
            group.extra.remove(RUST_SHADOW_KEY);
        }
        Ok(())
    })
}

fn load_from_group(store: &GroupStore, group_id: &str, group: &GroupDoc) -> io::Result<Value> {
    let state_dir = store.state_dir(group_id)?;
    Ok(load_from_paths(group, &state_dir))
}

fn load_from_paths(group: &GroupDoc, state_dir: &Path) -> Value {
    let mut state = group
        .extra
        .get(RUST_SHADOW_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in DURABLE_KEYS {
        state.remove(*key);
    }

    if let Some(config) = group
        .extra
        .get(CONFIG_KEY)
        .and_then(Value::as_object)
        .cloned()
    {
        let platform = config
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(mut config) = canonicalize_config(platform, &config) {
            let enabled = config
                .remove("enabled")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            state.insert("config".into(), Value::Object(config));
            state.insert("enabled".into(), Value::Bool(enabled));
        }
    }

    let now = epoch_seconds();
    state.insert(
        "authorized".into(),
        Value::Array(read_items(
            &state_path(state_dir, "authorized"),
            ItemKind::Authorized,
            now,
        )),
    );
    state.insert(
        "pending".into(),
        Value::Array(read_items(
            &state_path(state_dir, "pending"),
            ItemKind::Pending,
            now,
        )),
    );
    state.insert(
        "subscribers".into(),
        Value::Array(read_items(
            &state_path(state_dir, "subscribers"),
            ItemKind::Subscriber,
            now,
        )),
    );
    Value::Object(state)
}

fn persist(group: &mut GroupDoc, state_dir: &Path, state: &Value) -> io::Result<()> {
    let object = state.as_object().cloned().unwrap_or_default();
    if let Some(mut config) = object.get("config").and_then(Value::as_object).cloned() {
        config.insert(
            "enabled".into(),
            Value::Bool(
                object
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        );
        let platform = config
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(config) = canonicalize_config(platform, &config) {
            group.extra.insert(CONFIG_KEY.into(), Value::Object(config));
        } else {
            group.extra.remove(CONFIG_KEY);
        }
    } else {
        group.extra.remove(CONFIG_KEY);
    }

    for (key, kind) in [
        ("authorized", ItemKind::Authorized),
        ("pending", ItemKind::Pending),
        ("subscribers", ItemKind::Subscriber),
    ] {
        let items = normalize_items(object.get(key), kind, epoch_seconds());
        write_items(&state_path(state_dir, key), &items, kind)?;
    }

    let mut runtime = object;
    for key in DURABLE_KEYS {
        runtime.remove(*key);
    }
    if runtime.is_empty() {
        group.extra.remove(RUST_SHADOW_KEY);
    } else {
        group
            .extra
            .insert(RUST_SHADOW_KEY.into(), Value::Object(runtime));
    }
    Ok(())
}

fn read_items(path: &Path, kind: ItemKind, now: f64) -> Vec<Value> {
    let value = read_json::<Value>(path).unwrap_or_else(|_| json!({}));
    normalize_items(Some(&value), kind, now)
}

fn normalize_items(value: Option<&Value>, kind: ItemKind, now: f64) -> Vec<Value> {
    let mut items = if let Some(array) = value.and_then(Value::as_array) {
        array.clone()
    } else {
        value
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(key, item)| normalize_object_item(key, item, kind))
            .collect()
    };
    items.retain_mut(|item| normalize_item(item, kind, now));
    if matches!(kind, ItemKind::Pending) {
        items.sort_by(|left, right| {
            right["created_at"]
                .as_f64()
                .partial_cmp(&left["created_at"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    items
}

fn normalize_object_item(key: &str, item: &Value, kind: ItemKind) -> Option<Value> {
    let mut item = item.as_object().cloned()?;
    match kind {
        ItemKind::Pending => {
            item.entry("key").or_insert_with(|| json!(key));
        }
        ItemKind::Authorized | ItemKind::Subscriber => {
            let (chat_id, thread_id) = target_from_key(key);
            item.entry("chat_id").or_insert_with(|| json!(chat_id));
            item.entry("thread_id").or_insert_with(|| json!(thread_id));
        }
    }
    Some(Value::Object(item))
}

fn normalize_item(item: &mut Value, kind: ItemKind, now: f64) -> bool {
    let Some(object) = item.as_object_mut() else {
        return false;
    };
    match kind {
        ItemKind::Pending => {
            let key = object
                .get("key")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if key.is_empty() {
                return false;
            }
            let created_at = object
                .get("created_at")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let expires_at = object
                .get("expires_at")
                .and_then(Value::as_f64)
                .unwrap_or(created_at + PENDING_TTL_SECONDS);
            if expires_at <= now {
                return false;
            }
            object.insert("expires_at".into(), json!(expires_at));
            object.insert(
                "expires_in_seconds".into(),
                json!((expires_at - now).max(0.0) as i64),
            );
        }
        ItemKind::Authorized | ItemKind::Subscriber => {
            if object
                .get("chat_id")
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
            {
                return false;
            }
            object
                .entry("thread_id")
                .or_insert_with(|| Value::Number(0.into()));
        }
    }
    true
}

fn write_items(path: &Path, items: &[Value], kind: ItemKind) -> io::Result<()> {
    let mut output = Map::new();
    for item in items {
        let Some(mut value) = item.as_object().cloned() else {
            continue;
        };
        let key = match kind {
            ItemKind::Pending => value
                .remove("key")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
            ItemKind::Authorized | ItemKind::Subscriber => target_key(&value),
        };
        if key.is_empty() {
            continue;
        }
        if matches!(kind, ItemKind::Pending) {
            value.remove("expires_at");
            value.remove("expires_in_seconds");
        }
        if matches!(kind, ItemKind::Subscriber) {
            value.remove("chat_id");
        }
        output.insert(key, Value::Object(value));
    }
    let output = Value::Object(output);
    if read_json::<Value>(path).ok().as_ref() == Some(&output) {
        return Ok(());
    }
    write_json_committed(path, &output)
}

fn target_key(item: &Map<String, Value>) -> String {
    let chat_id = item
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if chat_id.is_empty() {
        return String::new();
    }
    let thread_id = normalized_thread_id(item.get("thread_id"));
    if !thread_id.is_empty() {
        format!("{chat_id}:{thread_id}")
    } else {
        chat_id.to_owned()
    }
}

fn target_from_key(key: &str) -> (String, Value) {
    key.rsplit_once(':')
        .filter(|(chat_id, thread_id)| !chat_id.is_empty() && !thread_id.is_empty())
        .map(|(chat_id, thread_id)| {
            let thread_id = thread_id
                .parse::<i64>()
                .map_or_else(|_| json!(thread_id), |value| json!(value));
            (chat_id.to_owned(), thread_id)
        })
        .unwrap_or_else(|| (key.to_owned(), json!(0)))
}

fn normalized_thread_id(value: Option<&Value>) -> String {
    let value = match value {
        Some(Value::String(value)) => value.trim().to_owned(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    };
    if value == "0" { String::new() } else { value }
}

fn state_path(state_dir: &Path, key: &str) -> PathBuf {
    let file = match key {
        "authorized" => "im_authorized_chats.json",
        "pending" => "im_pending_keys.json",
        "subscribers" => "im_subscribers.json",
        _ => unreachable!("known IM state class"),
    };
    state_dir.join(file)
}

fn epoch_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

fn first_string(raw: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        raw.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn set_secret_ref(
    output: &mut Map<String, Value>,
    env_key: &str,
    value_key: &str,
    value: Option<String>,
) {
    let Some(value) = value else {
        return;
    };
    if is_env_var_name(&value) {
        output.insert(env_key.into(), json!(value));
    } else {
        output.insert(value_key.into(), json!(value));
    }
}

fn is_env_var_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_uppercase())
        && chars.all(|value| value == '_' || value.is_ascii_uppercase() || value.is_ascii_digit())
}

fn normalize_feishu_domain(value: String) -> String {
    let mut value = value.trim().to_ascii_lowercase();
    while value.ends_with('/') {
        value.pop();
    }
    if value.ends_with("/open-apis") {
        value.truncate(value.len() - "/open-apis".len());
        while value.ends_with('/') {
            value.pop();
        }
    }
    if matches!(
        value.as_str(),
        "lark"
            | "global"
            | "intl"
            | "international"
            | "open.larkoffice.com"
            | "https://open.larkoffice.com"
            | "open.larksuite.com"
            | "https://open.larksuite.com"
    ) {
        "https://open.larkoffice.com".into()
    } else {
        "https://open.feishu.cn".into()
    }
}

fn coerce_bool(value: &Value, default: bool) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => {
            let value = value.trim().to_ascii_lowercase();
            if matches!(value.as_str(), "1" | "true" | "yes" | "y" | "on") {
                true
            } else if matches!(value.as_str(), "0" | "false" | "no" | "n" | "off") {
                false
            } else {
                value.parse::<i64>().map_or(default, |value| value != 0)
            }
        }
        Value::Null => default,
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HomeLayout, integration_state};
    use tempfile::TempDir;

    fn fixture() -> (TempDir, GroupStore, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("IM state", "").expect("group");
        (temp, store, group.group_id)
    }

    #[test]
    fn config_canonicalization_matches_stable_credential_shape() {
        let raw = json!({
            "platform":"feishu",
            "app_key_env":"FEISHU_APP_ID",
            "app_secret_env":"raw-secret",
            "domain":"lark",
            "enabled":"yes",
            "files":{"enabled":false,"max_mb":7},
            "skip_pending_on_start":false,
            "extension":{"future":true},
            "token_env":"must-not-leak"
        });
        let config = canonicalize_config("feishu", raw.as_object().expect("configuration object"))
            .expect("supported platform");

        assert_eq!(config["feishu_app_id_env"], "FEISHU_APP_ID");
        assert_eq!(config["feishu_app_secret"], "raw-secret");
        assert_eq!(config["feishu_domain"], "https://open.larkoffice.com");
        assert_eq!(config["enabled"], true);
        assert_eq!(config["extension"]["future"], true);
        assert!(!config.contains_key("skip_pending_on_start"));
        assert!(!config.contains_key("app_key_env"));
        assert!(!config.contains_key("app_secret_env"));
        assert!(!config.contains_key("token_env"));
        assert!(has_required_credentials("feishu", &config));

        let token = canonicalize_config(
            "telegram",
            json!({"token":"raw-token"})
                .as_object()
                .expect("configuration object"),
        )
        .expect("supported platform");
        assert_eq!(token["bot_token"], "raw-token");
        assert!(!token.contains_key("bot_token_env"));
        assert!(has_required_credentials("telegram", &token));
    }

    #[test]
    fn composite_round_trip_uses_stable_state_classes() {
        let (_temp, store, group_id) = fixture();
        update(&store, &group_id, |state| {
            *state = json!({
                "config":{"platform":"telegram","bot_token_env":"TOKEN","files":{"enabled":true,"max_mb":7}},
                "enabled":true,
                "authorized":[{"chat_id":"chat","thread_id":2,"platform":"telegram"}],
                "subscribers":[{"chat_id":"chat","thread_id":2,"platform":"telegram","subscribed":true,"verbose":true}],
                "pending":[{"key":"pending","chat_id":"next","thread_id":0,"platform":"telegram","created_at":epoch_seconds()}],
                "running":true
            });
            Ok(())
        })
        .expect("update");

        let group = store.load(&group_id).expect("group");
        assert_eq!(group.extra[CONFIG_KEY]["enabled"], true);
        assert_eq!(group.extra[CONFIG_KEY]["files"]["max_mb"], 7);
        assert!(group.extra[RUST_SHADOW_KEY].get("config").is_none());
        assert_eq!(group.extra[RUST_SHADOW_KEY]["running"], true);
        let state = load(&store, &group_id).expect("load");
        assert_eq!(state["config"]["platform"], "telegram");
        assert_eq!(state["authorized"][0]["chat_id"], "chat");
        assert_eq!(state["subscribers"][0]["verbose"], true);
        assert_eq!(state["pending"][0]["key"], "pending");
    }

    #[test]
    fn string_thread_ids_remain_distinct_across_round_trip() {
        let (_temp, store, group_id) = fixture();
        update(&store, &group_id, |state| {
            state["subscribers"] = json!([
                {"chat_id":"channel","thread_id":"1710000000.100","platform":"slack"},
                {"chat_id":"channel","thread_id":"1710000000.200","platform":"slack"}
            ]);
            Ok(())
        })
        .expect("update");

        let state = load(&store, &group_id).expect("load");
        let subscribers = state["subscribers"].as_array().expect("subscribers");
        assert_eq!(subscribers.len(), 2);
        assert!(
            subscribers
                .iter()
                .any(|item| item["thread_id"] == "1710000000.100")
        );
        assert!(
            subscribers
                .iter()
                .any(|item| item["thread_id"] == "1710000000.200")
        );
    }

    #[test]
    fn canonical_classes_win_and_shadow_only_seeds_missing_classes() {
        let (_temp, store, group_id) = fixture();
        let state_dir = store.state_dir(&group_id).expect("state dir");
        integration_state::group_update(&store, &group_id, RUST_SHADOW_KEY, |value| {
            *value = json!({
                "config":{"platform":"discord","bot_token_env":"SHADOW"},
                "enabled":true,
                "authorized":[{"chat_id":"shadow-auth","thread_id":0}],
                "pending":[{"key":"shadow-pending","chat_id":"pending","created_at":epoch_seconds()}],
                "subscribers":[]
            });
            Ok(())
        })
        .expect("shadow");
        store
            .mutate(&group_id, |group| {
                group.extra.insert(
                    CONFIG_KEY.into(),
                    json!({"platform":"telegram","bot_token_env":"CANONICAL","enabled":false}),
                );
                Ok(())
            })
            .expect("canonical config");
        write_json_committed(
            &state_path(&state_dir, "authorized"),
            &json!({"canonical-auth":{"chat_id":"canonical-auth","thread_id":0}}),
        )
        .expect("canonical auth");

        let state = load(&store, &group_id).expect("load");
        assert_eq!(state["config"]["platform"], "telegram");
        assert_eq!(state["authorized"][0]["chat_id"], "canonical-auth");
        assert_eq!(state["pending"][0]["key"], "shadow-pending");
        assert!(state_path(&state_dir, "subscribers").exists());
    }

    #[test]
    fn imported_shadow_is_consumed_and_cannot_resurrect_deleted_state() {
        let (_temp, store, group_id) = fixture();
        let state_dir = store.state_dir(&group_id).expect("state dir");
        integration_state::group_update(&store, &group_id, RUST_SHADOW_KEY, |value| {
            *value = json!({
                "config":{"platform":"telegram","bot_token_env":"TOKEN"},
                "enabled":true,
                "authorized":[{"chat_id":"shadow-auth","thread_id":0}],
                "running":true
            });
            Ok(())
        })
        .expect("shadow");

        let imported = load(&store, &group_id).expect("import");
        assert_eq!(imported["authorized"][0]["chat_id"], "shadow-auth");
        let group = store.load(&group_id).expect("group after import");
        assert!(group.extra[RUST_SHADOW_KEY].get("authorized").is_none());
        assert!(group.extra[RUST_SHADOW_KEY].get("config").is_none());
        assert_eq!(group.extra[RUST_SHADOW_KEY]["running"], true);

        store
            .mutate(&group_id, |group| {
                group.extra.remove(CONFIG_KEY);
                Ok(())
            })
            .expect("simulate Python config removal");
        std::fs::remove_file(state_path(&state_dir, "authorized"))
            .expect("simulate Python authorization removal");

        let reloaded = load(&store, &group_id).expect("reload");
        assert!(reloaded.get("config").is_none());
        assert!(
            reloaded["authorized"]
                .as_array()
                .expect("authorized")
                .is_empty()
        );
    }

    #[test]
    fn clearing_composite_prevents_shadow_resurrection() {
        let (_temp, store, group_id) = fixture();
        integration_state::group_update(&store, &group_id, RUST_SHADOW_KEY, |value| {
            *value = json!({
                "config":{"platform":"telegram","bot_token_env":"TOKEN"},
                "enabled":true,
                "authorized":[{"chat_id":"chat"}],
                "running":true
            });
            Ok(())
        })
        .expect("shadow");
        let _ = load(&store, &group_id).expect("import");
        update(&store, &group_id, |state| {
            *state = json!({});
            Ok(())
        })
        .expect("clear");

        let state = load(&store, &group_id).expect("reload");
        assert!(state.get("config").is_none());
        assert!(
            state["authorized"]
                .as_array()
                .expect("authorized")
                .is_empty()
        );
        let group = store.load(&group_id).expect("group");
        assert!(group.extra.get(CONFIG_KEY).is_none());
        assert!(group.extra.get(RUST_SHADOW_KEY).is_none());
    }
}
