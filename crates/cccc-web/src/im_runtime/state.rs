use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

pub(super) fn authorized_chat_ids(home: &HomeLayout, group_id: &str) -> HashSet<String> {
    GroupStore::new(home.clone())
        .map(|store| authorized_chat_ids_from_store(&store, group_id))
        .unwrap_or_default()
}

pub(super) fn accepts_inbound(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    text: &str,
) -> bool {
    let authorized = authorized_chat_ids(home, group_id).contains(chat_id);
    let command = text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split('@')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match command.as_str() {
        "/subscribe" | "/sub" => {
            if !authorized {
                create_pending_subscription(home, group_id, platform, chat_id);
            }
            false
        }
        "/unsubscribe" | "/unsub" => {
            update_authorized_chat(home, group_id, platform, chat_id, AuthorizedUpdate::Remove);
            false
        }
        "/pause" => {
            update_authorized_chat(
                home,
                group_id,
                platform,
                chat_id,
                AuthorizedUpdate::Paused(true),
            );
            false
        }
        "/resume" => {
            update_authorized_chat(
                home,
                group_id,
                platform,
                chat_id,
                AuthorizedUpdate::Paused(false),
            );
            false
        }
        "/verbose" => {
            update_authorized_chat(
                home,
                group_id,
                platform,
                chat_id,
                AuthorizedUpdate::ToggleVerbose,
            );
            false
        }
        "/status" | "/help" => false,
        "/send" => authorized && send_payload(text).is_some(),
        command if command.starts_with('/') => false,
        _ => authorized,
    }
}

fn create_pending_subscription(home: &HomeLayout, group_id: &str, platform: &str, chat_id: &str) {
    let Ok(store) = GroupStore::new(home.clone()) else {
        return;
    };
    let now = chrono::Utc::now().timestamp() as f64;
    let key: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(12)
        .collect();
    let _ = cccc_core::integration_state::group_update(&store, group_id, "im_bridge", |value| {
        if !value.is_object() {
            *value = json!({});
        }
        let state = value.as_object_mut().expect("IM state initialized");
        let pending = state.entry("pending").or_insert_with(|| json!([]));
        if !pending.is_array() {
            *pending = json!([]);
        }
        let items = pending.as_array_mut().expect("pending initialized");
        items.retain(|item| item["expires_at"].as_f64().unwrap_or(0.0) > now);
        if !items
            .iter()
            .any(|item| item["chat_id"] == chat_id && item["platform"] == platform)
        {
            items.push(json!({
                "key":key,"chat_id":chat_id,"thread_id":0,"platform":platform,
                "created_at":now,"expires_at":now+600.0,"expires_in_seconds":600
            }));
        }
        Ok(())
    });
}

#[derive(Clone, Copy)]
enum AuthorizedUpdate {
    Remove,
    Paused(bool),
    ToggleVerbose,
}

fn update_authorized_chat(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    update: AuthorizedUpdate,
) {
    let Ok(store) = GroupStore::new(home.clone()) else {
        return;
    };
    let _ = cccc_core::integration_state::group_update(&store, group_id, "im_bridge", |value| {
        if !value.is_object() {
            return Ok(());
        }
        let Some(items) = value.get_mut("authorized").and_then(Value::as_array_mut) else {
            return Ok(());
        };
        if matches!(update, AuthorizedUpdate::Remove) {
            items.retain(|item| {
                item["chat_id"].as_str() != Some(chat_id)
                    || item["platform"]
                        .as_str()
                        .is_some_and(|value| value != platform)
            });
            return Ok(());
        }
        if let Some(item) = items.iter_mut().find(|item| {
            item["chat_id"].as_str() == Some(chat_id)
                && item["platform"]
                    .as_str()
                    .is_none_or(|value| value == platform)
        }) {
            match update {
                AuthorizedUpdate::Paused(paused) => item["paused"] = json!(paused),
                AuthorizedUpdate::ToggleVerbose => {
                    item["verbose"] = json!(!item["verbose"].as_bool().unwrap_or(false));
                }
                AuthorizedUpdate::Remove => {}
            }
        }
        Ok(())
    });
}

pub(super) fn authorized_chat_ids_from_store(
    store: &GroupStore,
    group_id: &str,
) -> HashSet<String> {
    let mut chat_ids = HashSet::new();
    if let Ok(value) = cccc_core::integration_state::group_get(store, group_id, "im_bridge") {
        let has_canonical_authorization = ["authorized", "subscribers"]
            .into_iter()
            .any(|key| value.get(key).is_some());
        for key in ["authorized", "subscribers"] {
            collect_active_chat_ids(value.get(key), &mut chat_ids);
        }
        if has_canonical_authorization {
            return chat_ids;
        }
    }
    if let Ok(state_dir) = store.state_dir(group_id) {
        for name in ["im_authorized_chats.json", "im_subscribers.json"] {
            if let Ok(raw) = std::fs::read_to_string(state_dir.join(name))
                && let Ok(value) = serde_json::from_str::<Value>(&raw)
            {
                collect_chat_ids(Some(&value), &mut chat_ids);
            }
        }
    }
    chat_ids
}

fn collect_active_chat_ids(value: Option<&Value>, chat_ids: &mut HashSet<String>) {
    let items: Vec<&Value> = match value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::Object(items)) => items.values().collect(),
        _ => Vec::new(),
    };
    for item in items {
        if !item["paused"].as_bool().unwrap_or(false)
            && let Some(chat_id) = item.get("chat_id").and_then(Value::as_str)
        {
            chat_ids.insert(chat_id.to_owned());
        }
    }
}

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

pub(super) fn resolve_credential(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("IM credential is empty".into());
    }
    Ok(std::env::var(value).unwrap_or_else(|_| value.to_owned()))
}

pub(super) fn string(config: &Map<String, Value>, key: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

pub(super) async fn dispatch_inbound(
    client: &DaemonClient,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    sender: &str,
    text: &str,
) -> Result<(), String> {
    dispatch_inbound_with(
        client,
        group_id,
        platform,
        chat_id,
        sender,
        text,
        InboundMetadata::default(),
    )
    .await
}

#[derive(Default)]
pub(super) struct InboundMetadata {
    pub message_id: String,
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
) -> Result<(), String> {
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
        Ok(())
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
    let (text, to) = send_payload(text)?;
    let mut args = Map::new();
    args.insert("group_id".into(), json!(group_id));
    args.insert("by".into(), json!("user"));
    args.insert("text".into(), json!(text));
    args.insert("to".into(), json!(to));
    args.insert("transport".into(), json!("im"));
    args.insert("im_platform".into(), json!(platform));
    args.insert("im_chat_id".into(), json!(chat_id));
    args.insert("source_platform".into(), json!(platform));
    args.insert("source_user_id".into(), json!(sender));
    let message_id = metadata.message_id.trim();
    if !message_id.is_empty() {
        args.insert("source_message_id".into(), json!(message_id));
        args.insert(
            "client_id".into(),
            json!(format!("im:{platform}:{chat_id}:{message_id}")),
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
                attachments: vec![json!({"kind":"image","path":"state/blobs/hash"})],
            },
        )
        .expect("args");
        assert_eq!(args["client_id"], "im:wecom:chat-1:msg-1");
        assert_eq!(args["source_message_id"], "msg-1");
        assert_eq!(args["attachments"][0]["kind"], "image");
    }

    #[test]
    fn unsubscribe_is_consumed_and_removes_authorization() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("commands", "").expect("group");
        cccc_core::integration_state::group_update(&store, &group.group_id, "im_bridge", |state| {
            *state = json!({"authorized":[{
                "chat_id":"chat-1","platform":"telegram","thread_id":0
            }]});
            Ok(())
        })
        .expect("state");

        assert!(!accepts_inbound(
            &home,
            &group.group_id,
            "telegram",
            "chat-1",
            "/unsubscribe"
        ));
        assert!(!authorized_chat_ids(&home, &group.group_id).contains("chat-1"));
    }
}
