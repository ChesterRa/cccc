use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
#[cfg(test)]
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthorizedChat {
    pub chat_id: String,
    pub thread_id: String,
    pub verbose: bool,
}

impl AuthorizedChat {
    pub(super) fn key(&self) -> String {
        target_key(&self.chat_id, &self.thread_id)
    }
}

pub(super) fn target_key(chat_id: &str, thread_id: &str) -> String {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() || thread_id == "0" {
        chat_id.to_owned()
    } else {
        format!("{chat_id}:{thread_id}")
    }
}

pub(super) fn normalized_thread_id(value: Option<&Value>) -> String {
    let value = match value {
        Some(Value::String(value)) => value.trim().to_owned(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    };
    if value == "0" { String::new() } else { value }
}

pub(super) fn thread_id_value(thread_id: &str) -> Value {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() || thread_id == "0" {
        json!(0)
    } else {
        json!(thread_id)
    }
}

pub(super) fn authorized_chats(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
) -> Vec<AuthorizedChat> {
    GroupStore::new(home.clone())
        .map(|store| authorized_chats_from_store(&store, group_id, platform))
        .unwrap_or_default()
}

pub(super) fn authorized_chats_from_store(
    store: &GroupStore,
    group_id: &str,
    platform: &str,
) -> Vec<AuthorizedChat> {
    let Ok(value) = cccc_core::im_state::load(store, group_id) else {
        return Vec::new();
    };
    let mut authorized = HashMap::new();
    collect_active_chats(value.get("authorized"), platform, &mut authorized);
    let mut subscribers = HashMap::new();
    collect_active_chats(value.get("subscribers"), platform, &mut subscribers);
    subscribers.retain(|key, verbose| {
        let Some(authorized_verbose) = authorized.get(key) else {
            return false;
        };
        *verbose |= *authorized_verbose;
        true
    });
    into_authorized_chats(subscribers)
}

fn collect_active_chats(
    value: Option<&Value>,
    platform: &str,
    chats: &mut HashMap<(String, String), bool>,
) {
    let items: Vec<&Value> = match value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::Object(items)) => items.values().collect(),
        _ => Vec::new(),
    };
    for item in items {
        if !item["subscribed"].as_bool().unwrap_or(true)
            || item["paused"].as_bool().unwrap_or(false)
            || !platform_matches(item, platform)
        {
            continue;
        }
        if let Some(chat_id) = item
            .get("chat_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|chat_id| !chat_id.is_empty())
        {
            let verbose = item["verbose"].as_bool().unwrap_or(false);
            chats
                .entry((
                    chat_id.to_owned(),
                    normalized_thread_id(item.get("thread_id")),
                ))
                .and_modify(|current| *current |= verbose)
                .or_insert(verbose);
        }
    }
}

fn platform_matches(item: &Value, platform: &str) -> bool {
    item.get("platform")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_none_or(|value| value.is_empty() || value.eq_ignore_ascii_case(platform))
}

fn into_authorized_chats(chats: HashMap<(String, String), bool>) -> Vec<AuthorizedChat> {
    chats
        .into_iter()
        .map(|((chat_id, thread_id), verbose)| AuthorizedChat {
            chat_id,
            thread_id,
            verbose,
        })
        .collect()
}

#[cfg(test)]
pub(super) fn collect_chat_ids(value: Option<&Value>, chat_ids: &mut HashSet<String>) {
    let items: Vec<&Value> = match value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::Object(items)) => items.values().collect(),
        _ => Vec::new(),
    };
    for item in items {
        if let Some(chat_id) = item.get("chat_id").and_then(Value::as_str) {
            chat_ids.insert(chat_id.to_owned());
        }
    }
}

pub(super) fn resolve_config_credential(
    config: &Map<String, Value>,
    value_key: &str,
    env_key: &str,
) -> Result<String, String> {
    let value = string(config, value_key);
    if !value.is_empty() {
        return Ok(value);
    }
    let env_name = string(config, env_key);
    if env_name.is_empty() {
        return Err(format!(
            "IM credential is missing: {value_key} or {env_key}"
        ));
    }
    std::env::var(&env_name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("IM credential environment variable is not set: {env_name}"))
}

pub(super) fn string(config: &Map<String, Value>, key: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

#[derive(Default)]
pub(super) struct InboundMetadata {
    pub message_id: String,
    pub thread_id: String,
    pub attachments: Vec<Value>,
}

pub(super) async fn dispatch_inbound_with(
    client: &DaemonClient,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    sender: &str,
    text: &str,
    metadata: InboundMetadata,
) -> Result<String, String> {
    let args = inbound_args(group_id, platform, chat_id, sender, text, metadata)
        .ok_or_else(|| "IM command has no message payload".to_owned())?;
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: "send".into(),
            args,
        })
        .await
        .map_err(|error| error.to_string())?;
    if response.ok {
        Ok(response
            .result
            .get("event")
            .and_then(Value::as_object)
            .and_then(|event| event.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned())
    } else {
        Err(response.error.map_or_else(
            || "daemon rejected IM message".into(),
            |error| error.message,
        ))
    }
}

fn inbound_args(
    group_id: &str,
    platform: &str,
    chat_id: &str,
    sender: &str,
    text: &str,
    metadata: InboundMetadata,
) -> Option<Map<String, Value>> {
    let (text, to) = send_payload(text).or_else(|| {
        (!metadata.attachments.is_empty())
            .then(|| ("[attachment]".to_owned(), vec!["@foreman".into()]))
    })?;
    let mut args = Map::new();
    args.insert("group_id".into(), json!(group_id));
    args.insert("by".into(), json!("user"));
    args.insert("text".into(), json!(text));
    args.insert("to".into(), json!(to));
    args.insert("message_mode".into(), json!("send"));
    args.insert("transport".into(), json!("im"));
    args.insert("im_platform".into(), json!(platform));
    args.insert("im_chat_id".into(), json!(chat_id));
    let thread_id = metadata.thread_id.trim();
    if !thread_id.is_empty() && thread_id != "0" {
        args.insert("im_thread_id".into(), json!(thread_id));
    }
    args.insert("source_platform".into(), json!(platform));
    args.insert("source_user_id".into(), json!(sender));
    let message_id = metadata.message_id.trim();
    if !message_id.is_empty() {
        args.insert("source_message_id".into(), json!(message_id));
        args.insert(
            "client_id".into(),
            json!(format!(
                "im:{platform}:{}:{message_id}",
                target_key(chat_id, thread_id)
            )),
        );
    }
    if !metadata.attachments.is_empty() {
        args.insert("attachments".into(), Value::Array(metadata.attachments));
    }
    Some(args)
}

fn send_payload(text: &str) -> Option<(String, Vec<String>)> {
    let text = text.trim();
    let command = text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split('@')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if command != "/send" {
        return (!text.is_empty()).then(|| (text.to_owned(), vec!["@foreman".into()]));
    }
    let payload = text.split_once(char::is_whitespace)?.1.trim();
    if let Some((target, message)) = payload.split_once(char::is_whitespace)
        && target.starts_with('@')
        && !message.trim().is_empty()
    {
        return Some((message.trim().to_owned(), vec![target.to_owned()]));
    }
    (!payload.is_empty()).then(|| (payload.to_owned(), vec!["@foreman".into()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_credentials_distinguish_values_from_environment_references() {
        let raw = json!({"bot_token":"literal-token"});
        assert_eq!(
            resolve_config_credential(
                raw.as_object().expect("configuration"),
                "bot_token",
                "bot_token_env",
            )
            .expect("literal credential"),
            "literal-token"
        );

        let reference = json!({"bot_token_env":"CCCC_IM_TEST_ENV_MUST_NOT_EXIST_6D6617"});
        let error = resolve_config_credential(
            reference.as_object().expect("configuration"),
            "bot_token",
            "bot_token_env",
        )
        .expect_err("missing environment variable");
        assert!(error.contains("CCCC_IM_TEST_ENV_MUST_NOT_EXIST_6D6617"));
    }

    #[test]
    fn canonical_empty_state_prevents_legacy_subscriber_resurrection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("IM", "").expect("group");
        let state_dir = store.state_dir(&group.group_id).expect("state dir");
        std::fs::write(
            state_dir.join("im_subscribers.json"),
            r#"{"legacy":{"chat_id":"legacy","subscribed":true}}"#,
        )
        .expect("legacy");
        cccc_core::im_state::update(&store, &group.group_id, |state| {
            *state = json!({"authorized":[],"subscribers":[]});
            Ok(())
        })
        .expect("canonical");

        assert!(authorized_chats_from_store(&store, &group.group_id, "telegram").is_empty());
    }

    #[test]
    fn outbound_targets_match_platform_and_preserve_verbose() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("IM", "").expect("group");
        cccc_core::im_state::update(&store, &group.group_id, |state| {
            *state = json!({
                "authorized":[
                    {"chat_id":"telegram-chat","platform":"telegram","verbose":false},
                    {"chat_id":"discord-chat","platform":"discord","verbose":true},
                    {"chat_id":"paused","platform":"telegram","paused":true},
                    {"chat_id":"legacy","platform":""}
                ],
                "subscribers":[
                    {"chat_id":"telegram-chat","platform":"telegram","verbose":true},
                    {"chat_id":"telegram-chat","thread_id":42,"platform":"telegram","verbose":false},
                    {"chat_id":"disabled","platform":"telegram","subscribed":false}
                ]
            });
            Ok(())
        })
        .expect("state");

        let mut targets = authorized_chats_from_store(&store, &group.group_id, "telegram");
        targets.sort_by(|left, right| {
            (&left.chat_id, &left.thread_id).cmp(&(&right.chat_id, &right.thread_id))
        });
        assert_eq!(
            targets,
            vec![AuthorizedChat {
                chat_id: "telegram-chat".into(),
                thread_id: String::new(),
                verbose: true,
            }]
        );
    }

    #[test]
    fn outbound_targets_require_both_authorization_and_subscription() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("IM trust boundary", "").expect("group");
        cccc_core::im_state::update(&store, &group.group_id, |state| {
            *state = json!({
                "authorized":[
                    {"chat_id":"both","platform":"telegram"},
                    {"chat_id":"authorization-only","platform":"telegram"}
                ],
                "subscribers":[
                    {"chat_id":"both","platform":"telegram","subscribed":true},
                    {"chat_id":"subscription-only","platform":"telegram","subscribed":true}
                ]
            });
            Ok(())
        })
        .expect("state");

        let targets = authorized_chats_from_store(&store, &group.group_id, "telegram");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].chat_id, "both");
    }

    #[test]
    fn im_inbound_is_a_user_message_with_source_metadata() {
        let args = inbound_args(
            "g_test",
            "dingtalk",
            "chat-1",
            "staff-1",
            "hello",
            InboundMetadata::default(),
        )
        .expect("args");
        assert_eq!(args["by"], "user");
        assert_eq!(args["to"], json!(["@foreman"]));
        assert_eq!(args["message_mode"], "send");
        assert_eq!(args["transport"], "im");
        assert_eq!(args["source_platform"], "dingtalk");
        assert_eq!(args["source_user_id"], "staff-1");
        assert_eq!(args["im_chat_id"], "chat-1");
    }

    #[test]
    fn send_command_extracts_target_and_message() {
        let args = inbound_args(
            "g_test",
            "telegram",
            "chat-1",
            "user-1",
            "/send @all hello peers",
            InboundMetadata::default(),
        )
        .expect("args");
        assert_eq!(args["text"], "hello peers");
        assert_eq!(args["to"], json!(["@all"]));
        assert!(
            inbound_args(
                "g_test",
                "telegram",
                "chat-1",
                "user-1",
                "/send",
                InboundMetadata::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn inbound_metadata_adds_stable_idempotency_and_attachments() {
        let args = inbound_args(
            "g_test",
            "wecom",
            "chat-1",
            "staff-1",
            "[image]",
            InboundMetadata {
                message_id: "msg-1".into(),
                thread_id: "thread-1".into(),
                attachments: vec![json!({"kind":"image","path":"state/blobs/hash"})],
            },
        )
        .expect("args");
        assert_eq!(args["source_message_id"], "msg-1");
        assert_eq!(args["im_thread_id"], "thread-1");
        assert_eq!(args["client_id"], "im:wecom:chat-1:thread-1:msg-1");
        assert_eq!(args["attachments"][0]["kind"], "image");
    }

    #[test]
    fn attachment_only_inbound_uses_a_visible_placeholder() {
        let args = inbound_args(
            "g_test",
            "slack",
            "chat-1",
            "staff-1",
            "",
            InboundMetadata {
                message_id: "msg-1".into(),
                thread_id: String::new(),
                attachments: vec![json!({"kind":"image","path":"state/blobs/hash"})],
            },
        )
        .expect("args");
        assert_eq!(args["text"], "[attachment]");
        assert_eq!(args["to"], json!(["@foreman"]));
    }
}
