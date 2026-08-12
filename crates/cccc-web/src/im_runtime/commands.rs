use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};
use std::io;

const SUBSCRIPTION_TTL_SECONDS: f64 = 600.0;
const RECOGNIZED_COMMANDS: &[&str] = &[
    "/subscribe",
    "/sub",
    "/unsubscribe",
    "/unsub",
    "/pause",
    "/resume",
    "/verbose",
    "/status",
    "/help",
    "/send",
];

pub(super) enum InboundDecision {
    Forward,
    Reply(String),
}

#[derive(Clone, Copy, Default)]
struct ChatAuthorization {
    authorized: bool,
    paused: bool,
    verbose: bool,
}

pub(super) async fn inbound_decision(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    text: &str,
) -> InboundDecision {
    inbound_decision_for_thread(home, group_id, platform, chat_id, "", text).await
}

pub(super) async fn inbound_decision_for_thread(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    thread_id: &str,
    text: &str,
) -> InboundDecision {
    let home = home.clone();
    let group_id = group_id.to_owned();
    let platform = platform.to_owned();
    let chat_id = chat_id.to_owned();
    let thread_id = thread_id.to_owned();
    let text = text.to_owned();
    match tokio::task::spawn_blocking(move || {
        inbound_decision_blocking_for_thread(
            &home, &group_id, &platform, &chat_id, &thread_id, &text,
        )
    })
    .await
    {
        Ok(decision) => decision,
        Err(error) => {
            tracing::error!(%error, "IM command worker failed");
            InboundDecision::Reply("Could not process the message. Try again later.".into())
        }
    }
}

#[cfg(test)]
fn inbound_decision_blocking(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    text: &str,
) -> InboundDecision {
    inbound_decision_blocking_for_thread(home, group_id, platform, chat_id, "", text)
}

fn inbound_decision_blocking_for_thread(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    thread_id: &str,
    text: &str,
) -> InboundDecision {
    let authorization = chat_authorization(home, group_id, platform, chat_id, thread_id);
    let command = command_name(text);
    let update_authorized = |update, success| {
        if !authorization.authorized {
            return InboundDecision::Reply(authorization_required(platform).into());
        }
        update_authorization(
            home, group_id, platform, chat_id, thread_id, update, success,
        )
    };
    match command.as_str() {
        "/subscribe" | "/sub" => {
            subscribe(home, group_id, platform, chat_id, thread_id, authorization)
        }
        "/unsubscribe" | "/unsub" => update_authorization(
            home,
            group_id,
            platform,
            chat_id,
            thread_id,
            AuthorizedUpdate::Remove,
            "Subscription removed.",
        ),
        "/pause" => update_authorized(AuthorizedUpdate::Paused(true), "Subscription paused."),
        "/resume" => update_authorized(AuthorizedUpdate::Paused(false), "Subscription resumed."),
        "/verbose" => match verbose_value(text) {
            Ok(verbose) => update_authorized(
                AuthorizedUpdate::Verbose(verbose),
                if verbose {
                    "Verbose delivery enabled."
                } else {
                    "Verbose delivery disabled."
                },
            ),
            Err(()) => InboundDecision::Reply("Usage: /verbose [on|off]".into()),
        },
        "/status" => InboundDecision::Reply(status_text(home, group_id, authorization)),
        "/help" => InboundDecision::Reply(help_text(platform).into()),
        "/send" if !authorization.authorized => {
            InboundDecision::Reply(authorization_required(platform).into())
        }
        "/send" if authorization.paused => {
            InboundDecision::Reply("This subscription is paused. Send /resume first.".into())
        }
        "/send" if send_has_payload(text) => InboundDecision::Forward,
        "/send" => InboundDecision::Reply("Usage: /send [@actor] <message>".into()),
        command if command.starts_with('/') => InboundDecision::Reply(format!(
            "Unknown command: {command}\n{}",
            help_text(platform)
        )),
        _ if authorization.authorized && authorization.paused => {
            InboundDecision::Reply("This subscription is paused. Send /resume to continue.".into())
        }
        _ if authorization.authorized => InboundDecision::Forward,
        _ => InboundDecision::Reply(unauthorized_plain_text(home, group_id, platform)),
    }
}

fn subscribe(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    thread_id: &str,
    authorization: ChatAuthorization,
) -> InboundDecision {
    if authorization.authorized {
        let group_name = group_display_name(home, group_id);
        return InboundDecision::Reply(if authorization.paused {
            format!(
                "This chat is already authorized for CCCC group \"{group_name}\" but paused. Send /resume to continue."
            )
        } else {
            format!(
                "This chat is already authorized for CCCC group \"{group_name}\". In direct chat, send plain text to reach @foreman; /send is only for explicit recipients."
            )
        });
    }
    if platform.eq_ignore_ascii_case("weixin") {
        return InboundDecision::Reply(authorization_required(platform).into());
    }
    match create_pending_subscription(home, group_id, platform, chat_id, thread_id) {
        Ok(key) => InboundDecision::Reply(format!(
            "CCCC pairing key: {key}\nRequest target: CCCC group \"{}\"\nThis key expires in 10 minutes. Approve it in Pending Requests or run: cccc im bind --key {key} --group {group_id}\nAfter approval, direct messages work as plain text; /send is only for explicit recipients.",
            group_display_name(home, group_id)
        )),
        Err(error) => {
            tracing::warn!(%error, %group_id, %platform, %chat_id, "failed to create IM subscription request");
            InboundDecision::Reply(
                "Could not create a pairing request. Check the CCCC server logs and try again."
                    .into(),
            )
        }
    }
}

fn group_display_name(home: &HomeLayout, group_id: &str) -> String {
    GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .ok()
        .map(|group| group.title.trim().to_owned())
        .filter(|title| !title.is_empty() && title != group_id)
        .unwrap_or_else(|| group_id.to_owned())
}

fn status_text(home: &HomeLayout, group_id: &str, authorization: ChatAuthorization) -> String {
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .ok();
    let group_status = group.map_or_else(
        || format!("CCCC group status: id={group_id}, unavailable"),
        |group| {
            let state = match group.state {
                cccc_contracts::GroupState::Active => "active",
                cccc_contracts::GroupState::Idle => "idle",
                cccc_contracts::GroupState::Paused => "paused",
                cccc_contracts::GroupState::Stopped => "stopped",
            };
            format!(
                "CCCC group status: title=\"{}\", state={state}, running={}, actors={}",
                group.title,
                group.running,
                group.actors.len()
            )
        },
    );
    format!(
        "{group_status}\nSubscription: authorized={}, paused={}, verbose={}.",
        authorization.authorized, authorization.paused, authorization.verbose
    )
}

fn update_authorization(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    thread_id: &str,
    update: AuthorizedUpdate,
    success: &str,
) -> InboundDecision {
    match persist_authorization_update(home, group_id, platform, chat_id, thread_id, update) {
        Ok(true) => InboundDecision::Reply(success.into()),
        Ok(false) => InboundDecision::Reply("This chat is not authorized.".into()),
        Err(error) => {
            tracing::warn!(%error, %group_id, %platform, %chat_id, "failed to update IM authorization");
            InboundDecision::Reply("Could not update the subscription. Try again later.".into())
        }
    }
}

fn create_pending_subscription(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    thread_id: &str,
) -> Result<String, String> {
    let store = GroupStore::new(home.clone()).map_err(|error| error.to_string())?;
    let now = chrono::Utc::now().timestamp() as f64;
    cccc_core::im_state::update(&store, group_id, |value| {
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
        if let Some(key) = items.iter().find_map(|item| {
            (item["chat_id"].as_str() == Some(chat_id)
                && item["platform"].as_str() == Some(platform)
                && super::normalized_thread_id(item.get("thread_id")) == thread_id.trim())
            .then(|| item["key"].as_str().map(str::to_owned))
            .flatten()
        }) {
            return Ok(key);
        }
        let key: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(12)
            .collect();
        items.push(json!({
            "key":key,"chat_id":chat_id,
            "thread_id":super::thread_id_value(thread_id),"platform":platform,
            "created_at":now,"expires_at":now+SUBSCRIPTION_TTL_SECONDS,
            "expires_in_seconds":SUBSCRIPTION_TTL_SECONDS as i64
        }));
        Ok(key)
    })
    .map_err(|error| error.to_string())
}

fn chat_authorization(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    thread_id: &str,
) -> ChatAuthorization {
    let Ok(store) = GroupStore::new(home.clone()) else {
        return ChatAuthorization::default();
    };
    let Ok(state) = cccc_core::im_state::load(&store, group_id) else {
        return ChatAuthorization::default();
    };
    let Some(authorized) = state
        .get("authorized")
        .into_iter()
        .flat_map(items)
        .find(|item| matches_chat(item, platform, chat_id, thread_id))
    else {
        return ChatAuthorization::default();
    };
    let subscriber = state
        .get("subscribers")
        .into_iter()
        .flat_map(items)
        .find(|item| matches_chat(item, platform, chat_id, thread_id));
    ChatAuthorization {
        authorized: true,
        paused: subscriber
            .and_then(|item| item["paused"].as_bool())
            .unwrap_or_else(|| authorized["paused"].as_bool().unwrap_or(false)),
        verbose: subscriber
            .and_then(|item| item["verbose"].as_bool())
            .unwrap_or_else(|| authorized["verbose"].as_bool().unwrap_or(false)),
    }
}

fn items(value: &Value) -> Box<dyn Iterator<Item = &Value> + '_> {
    match value {
        Value::Array(items) => Box::new(items.iter()),
        Value::Object(items) => Box::new(items.values()),
        _ => Box::new(std::iter::empty()),
    }
}

fn matches_chat(item: &Value, platform: &str, chat_id: &str, thread_id: &str) -> bool {
    item["chat_id"].as_str() == Some(chat_id)
        && super::normalized_thread_id(item.get("thread_id")) == thread_id.trim()
        && item["subscribed"].as_bool().unwrap_or(true)
        && item["platform"]
            .as_str()
            .map(str::trim)
            .is_none_or(|value| value.is_empty() || value.eq_ignore_ascii_case(platform))
}

#[derive(Clone, Copy)]
enum AuthorizedUpdate {
    Remove,
    Paused(bool),
    Verbose(bool),
}

fn persist_authorization_update(
    home: &HomeLayout,
    group_id: &str,
    platform: &str,
    chat_id: &str,
    thread_id: &str,
    update: AuthorizedUpdate,
) -> Result<bool, String> {
    let store = GroupStore::new(home.clone()).map_err(|error| error.to_string())?;
    cccc_core::im_state::update(&store, group_id, |value| {
        let Some(state) = value.as_object_mut() else {
            return Ok(false);
        };
        let mut changed = false;
        for key in ["authorized", "subscribers"] {
            let Some(items) = state.get_mut(key) else {
                continue;
            };
            changed |= update_items(items, platform, chat_id, thread_id, update);
        }
        Ok(changed)
    })
    .map_err(|error: io::Error| error.to_string())
}

fn update_items(
    items: &mut Value,
    platform: &str,
    chat_id: &str,
    thread_id: &str,
    update: AuthorizedUpdate,
) -> bool {
    match items {
        Value::Array(items) => {
            if matches!(update, AuthorizedUpdate::Remove) {
                if platform.eq_ignore_ascii_case("weixin")
                    && let Some(item) = items
                        .iter_mut()
                        .find(|item| matches_chat(item, platform, chat_id, thread_id))
                {
                    item["subscribed"] = Value::Bool(false);
                    return true;
                }
                let before = items.len();
                items.retain(|item| !matches_chat(item, platform, chat_id, thread_id));
                return before != items.len();
            }
            items
                .iter_mut()
                .find(|item| matches_chat(item, platform, chat_id, thread_id))
                .is_some_and(|item| apply_item_update(item, update))
        }
        Value::Object(items) => {
            if matches!(update, AuthorizedUpdate::Remove) {
                if platform.eq_ignore_ascii_case("weixin")
                    && let Some(item) = items
                        .values_mut()
                        .find(|item| matches_chat(item, platform, chat_id, thread_id))
                {
                    item["subscribed"] = Value::Bool(false);
                    return true;
                }
                let before = items.len();
                items.retain(|_, item| !matches_chat(item, platform, chat_id, thread_id));
                return before != items.len();
            }
            items
                .values_mut()
                .find(|item| matches_chat(item, platform, chat_id, thread_id))
                .is_some_and(|item| apply_item_update(item, update))
        }
        _ => false,
    }
}

fn apply_item_update(item: &mut Value, update: AuthorizedUpdate) -> bool {
    match update {
        AuthorizedUpdate::Paused(paused) => item["paused"] = json!(paused),
        AuthorizedUpdate::Verbose(verbose) => item["verbose"] = json!(verbose),
        AuthorizedUpdate::Remove => unreachable!(),
    }
    true
}

fn command_name(text: &str) -> String {
    text.split_whitespace()
        .next()
        .unwrap_or("")
        .split('@')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub(super) fn is_recognized_command(text: &str) -> bool {
    RECOGNIZED_COMMANDS.contains(&command_name(text).as_str())
}

fn send_has_payload(text: &str) -> bool {
    text.trim()
        .split_once(char::is_whitespace)
        .is_some_and(|(_, payload)| !payload.trim().is_empty())
}

fn verbose_value(text: &str) -> Result<bool, ()> {
    match text.split_whitespace().nth(1).map(str::to_ascii_lowercase) {
        None => Ok(true),
        Some(value) if matches!(value.as_str(), "on" | "true" | "1") => Ok(true),
        Some(value) if matches!(value.as_str(), "off" | "false" | "0") => Ok(false),
        Some(_) => Err(()),
    }
}

fn authorization_required(platform: &str) -> &'static str {
    if platform.eq_ignore_ascii_case("weixin") {
        "This Weixin account is not authorized. Scan and confirm the QR code in CCCC Settings; the scanning account is authorized automatically."
    } else {
        "This chat is not authorized. Send /subscribe first."
    }
}

fn unauthorized_plain_text(home: &HomeLayout, group_id: &str, platform: &str) -> String {
    let group_name = group_display_name(home, group_id);
    if platform.eq_ignore_ascii_case("weixin") {
        return format!(
            "This Weixin account is not the QR-login account authorized for CCCC group \"{group_name}\". Scan and confirm the QR code in CCCC Settings; the scanning account can then send plain text directly."
        );
    }
    format!(
        "This chat is not authorized for CCCC group \"{group_name}\". Send /subscribe to request access. After approval, direct messages work as plain text; /send is only for explicit recipients."
    )
}

fn help_text(platform: &str) -> &'static str {
    if platform.eq_ignore_ascii_case("weixin") {
        "Commands: /unsubscribe, /send <message>, /pause, /resume, /verbose [on|off], /status, /help"
    } else {
        "Commands: /subscribe, /unsubscribe, /send <message>, /pause, /resume, /verbose [on|off], /status, /help"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, HomeLayout, GroupStore, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("commands", "").expect("group");
        (temp, home, store, group.group_id)
    }

    fn reply(decision: InboundDecision) -> String {
        match decision {
            InboundDecision::Reply(body) => body,
            _ => panic!("expected reply"),
        }
    }

    #[test]
    fn only_native_cccc_commands_are_recognized_for_group_routing() {
        for command in [
            "/subscribe",
            "/sub",
            "/unsubscribe@cccc_bot",
            "/send @all hello",
            "/status",
            "/help",
        ] {
            assert!(is_recognized_command(command), "command={command}");
        }
        for command in ["/deploy", "/weather tomorrow", "hello", ""] {
            assert!(!is_recognized_command(command), "command={command}");
        }
    }

    fn authorize(store: &GroupStore, group_id: &str, paused: bool, verbose: bool) {
        cccc_core::im_state::update(store, group_id, |state| {
            *state = json!({"authorized":[{
                "chat_id":"chat-1","platform":"telegram","thread_id":0,
                "paused":paused,"verbose":verbose
            }]});
            Ok(())
        })
        .expect("authorize");
    }

    #[test]
    fn subscribe_replies_with_persisted_key_and_reuses_it() {
        let (_temp, home, store, group_id) = setup();
        let first = reply(inbound_decision_blocking(
            &home,
            &group_id,
            "telegram",
            "chat-1",
            "/subscribe",
        ));
        let state = cccc_core::im_state::load(&store, &group_id).expect("state");
        let key = state["pending"][0]["key"].as_str().expect("key");
        assert!(first.contains(key));
        assert!(first.contains("CCCC group \"commands\""));
        let second = reply(inbound_decision_blocking(
            &home, &group_id, "telegram", "chat-1", "/sub",
        ));
        assert!(second.contains(key));
        let state = cccc_core::im_state::load(&store, &group_id).expect("state");
        assert_eq!(state["pending"].as_array().expect("pending").len(), 1);
    }

    #[test]
    fn subscriptions_and_authorization_are_scoped_to_the_native_thread() {
        let (_temp, home, store, group_id) = setup();
        let _ = inbound_decision_blocking_for_thread(
            &home,
            &group_id,
            "slack",
            "channel-1",
            "1710000000.100",
            "/subscribe",
        );
        let _ = inbound_decision_blocking_for_thread(
            &home,
            &group_id,
            "slack",
            "channel-1",
            "1710000000.200",
            "/subscribe",
        );
        let state = cccc_core::im_state::load(&store, &group_id).expect("state");
        let pending = state["pending"].as_array().expect("pending");
        assert_eq!(pending.len(), 2);
        let mut thread_ids = pending
            .iter()
            .filter_map(|item| item["thread_id"].as_str())
            .collect::<Vec<_>>();
        thread_ids.sort_unstable();
        assert_eq!(thread_ids, ["1710000000.100", "1710000000.200"]);

        cccc_core::im_state::update(&store, &group_id, |state| {
            state["authorized"] = json!([{
                "chat_id":"channel-1","platform":"slack","thread_id":"1710000000.100"
            }]);
            Ok(())
        })
        .expect("authorize thread");
        assert!(matches!(
            inbound_decision_blocking_for_thread(
                &home,
                &group_id,
                "slack",
                "channel-1",
                "1710000000.100",
                "hello",
            ),
            InboundDecision::Forward
        ));
        assert!(matches!(
            inbound_decision_blocking_for_thread(
                &home,
                &group_id,
                "slack",
                "channel-1",
                "1710000000.200",
                "hello",
            ),
            InboundDecision::Reply(_)
        ));
    }

    #[test]
    fn expired_subscription_key_is_replaced() {
        let (_temp, home, store, group_id) = setup();
        let _ = inbound_decision_blocking(&home, &group_id, "telegram", "chat-1", "/subscribe");
        let old = cccc_core::im_state::load(&store, &group_id).expect("state")["pending"][0]["key"]
            .as_str()
            .expect("key")
            .to_owned();
        cccc_core::im_state::update(&store, &group_id, |state| {
            state["pending"][0]["expires_at"] = json!(0.0);
            Ok(())
        })
        .expect("expire");
        let body = reply(inbound_decision_blocking(
            &home,
            &group_id,
            "telegram",
            "chat-1",
            "/subscribe",
        ));
        assert!(!body.contains(&old));
    }

    #[test]
    fn persistence_failure_is_reported_to_chat() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("missing")).expect("home");
        let body = reply(inbound_decision_blocking(
            &home,
            "missing-group",
            "telegram",
            "chat-1",
            "/subscribe",
        ));
        assert!(body.contains("Could not create"));
    }

    #[test]
    fn unsubscribe_removes_authorization() {
        let (_temp, home, store, group_id) = setup();
        authorize(&store, &group_id, false, false);
        let body = reply(inbound_decision_blocking(
            &home,
            &group_id,
            "telegram",
            "chat-1",
            "/unsubscribe",
        ));
        assert!(body.contains("removed"));
        assert!(
            super::super::authorized_chats(&home, &group_id, "telegram")
                .iter()
                .all(|chat| chat.chat_id != "chat-1")
        );
    }

    #[test]
    fn weixin_unsubscribe_survives_automatic_authorization_recovery() {
        let (_temp, home, store, group_id) = setup();
        cccc_core::im_state::update(&store, &group_id, |state| {
            *state = json!({"authorized":[{
                "chat_id":"wx-user","platform":"weixin","thread_id":0,
                "authorization_source":"weixin_login"
            }]});
            Ok(())
        })
        .expect("authorize");

        let body = reply(inbound_decision_blocking(
            &home,
            &group_id,
            "weixin",
            "wx-user",
            "/unsubscribe",
        ));

        assert!(body.contains("removed"));
        let state = cccc_core::im_state::load(&store, &group_id).expect("state");
        assert_eq!(state["authorized"][0]["subscribed"], false);
        assert!(super::super::authorized_chats(&home, &group_id, "weixin").is_empty());
    }

    #[test]
    fn paused_chat_can_resume_and_status_reflects_flags() {
        let (_temp, home, store, group_id) = setup();
        authorize(&store, &group_id, true, true);
        let status = reply(inbound_decision_blocking(
            &home, &group_id, "telegram", "chat-1", "/status",
        ));
        assert!(status.contains("authorized=true"));
        assert!(status.contains("paused=true"));
        assert!(status.contains("verbose=true"));
        let _ = inbound_decision_blocking(&home, &group_id, "telegram", "chat-1", "/resume");
        assert!(matches!(
            inbound_decision_blocking(&home, &group_id, "telegram", "chat-1", "hello"),
            InboundDecision::Forward
        ));
    }

    #[test]
    fn status_identifies_the_group_and_its_lifecycle() {
        let (_temp, home, store, group_id) = setup();
        authorize(&store, &group_id, false, false);

        let status = reply(inbound_decision_blocking(
            &home, &group_id, "telegram", "chat-1", "/status",
        ));

        assert!(status.contains("commands"));
        assert!(status.contains("state=active"));
        assert!(status.contains("running=false"));
    }

    #[test]
    fn ordinary_and_send_messages_obey_authorization() {
        let (_temp, home, store, group_id) = setup();
        let body = reply(inbound_decision_blocking(
            &home, &group_id, "telegram", "chat-1", "hello",
        ));
        assert!(body.contains("not authorized"));
        assert!(body.contains("CCCC group \"commands\""));
        assert!(!body.contains(&group_id));
        assert!(body.contains("direct messages work as plain text"));
        authorize(&store, &group_id, false, false);
        assert!(matches!(
            inbound_decision_blocking(&home, &group_id, "telegram", "chat-1", "hello"),
            InboundDecision::Forward
        ));
        assert!(matches!(
            inbound_decision_blocking(&home, &group_id, "telegram", "chat-1", "/send"),
            InboundDecision::Reply(_)
        ));
    }

    #[test]
    fn stale_subscription_does_not_grant_inbound_authorization() {
        let (_temp, home, store, group_id) = setup();
        cccc_core::im_state::update(&store, &group_id, |state| {
            *state = json!({"subscribers":[{
                "chat_id":"chat-1","platform":"telegram","thread_id":0,
                "subscribed":true
            }]});
            Ok(())
        })
        .expect("stale subscriber");

        let body = reply(inbound_decision_blocking(
            &home, &group_id, "telegram", "chat-1", "hello",
        ));
        assert!(body.contains("not authorized"));
    }

    #[test]
    fn paused_plain_text_reports_the_paused_state() {
        let (_temp, home, store, group_id) = setup();
        authorize(&store, &group_id, true, false);

        let body = reply(inbound_decision_blocking(
            &home, &group_id, "telegram", "chat-1", "hello",
        ));

        assert!(body.contains("paused"));
        assert!(body.contains("/resume"));
        assert!(!body.contains("not authorized"));
        assert!(!body.contains("/subscribe"));
    }

    #[test]
    fn unauthorized_plain_text_has_consistent_feedback_on_subscription_platforms() {
        let (_temp, home, _store, group_id) = setup();
        for platform in [
            "telegram", "slack", "discord", "feishu", "dingtalk", "wecom",
        ] {
            let body = reply(inbound_decision_blocking(
                &home,
                &group_id,
                platform,
                &format!("{platform}-chat"),
                "hello",
            ));
            assert!(body.contains("not authorized"), "platform={platform}");
            assert!(body.contains("/subscribe"), "platform={platform}");
            assert!(
                body.contains("CCCC group \"commands\""),
                "platform={platform}"
            );
            assert!(body.contains("plain text"), "platform={platform}");
        }
    }

    #[test]
    fn unauthorized_weixin_account_is_directed_to_qr_login() {
        let (_temp, home, store, group_id) = setup();
        for text in ["hello", "/subscribe", "/send hello", "/resume", "/help"] {
            let body = reply(inbound_decision_blocking(
                &home, &group_id, "weixin", "wx-user", text,
            ));
            assert!(!body.contains("/subscribe"), "text={text}: {body}");
        }
        let body = reply(inbound_decision_blocking(
            &home, &group_id, "weixin", "wx-user", "hello",
        ));
        assert!(body.contains("QR"));
        assert!(body.contains("automatically") || body.contains("send plain text directly"));
        let state = cccc_core::im_state::load(&store, &group_id).expect("state");
        assert!(state["pending"].as_array().is_some_and(Vec::is_empty));
    }

    #[test]
    fn verbose_command_is_idempotent_and_supports_explicit_disable() {
        let (_temp, home, store, group_id) = setup();
        authorize(&store, &group_id, false, false);

        for _ in 0..2 {
            let body = reply(inbound_decision_blocking(
                &home, &group_id, "telegram", "chat-1", "/verbose",
            ));
            assert!(body.contains("enabled"));
        }
        let state = cccc_core::im_state::load(&store, &group_id).expect("state");
        assert_eq!(state["authorized"][0]["verbose"], true);

        let body = reply(inbound_decision_blocking(
            &home,
            &group_id,
            "telegram",
            "chat-1",
            "/verbose off",
        ));
        assert!(body.contains("disabled"));
        let state = cccc_core::im_state::load(&store, &group_id).expect("state");
        assert_eq!(state["authorized"][0]["verbose"], false);
    }

    #[test]
    fn object_shaped_legacy_authorization_can_be_updated_and_removed() {
        let (_temp, home, store, group_id) = setup();
        cccc_core::im_state::update(&store, &group_id, |state| {
            *state = json!({"authorized":{"chat-1":{
                "chat_id":"chat-1","platform":"telegram","paused":true
            }}});
            Ok(())
        })
        .expect("legacy authorization");

        let body = reply(inbound_decision_blocking(
            &home, &group_id, "telegram", "chat-1", "/resume",
        ));
        assert!(body.contains("resumed"));
        assert!(matches!(
            inbound_decision_blocking(&home, &group_id, "telegram", "chat-1", "hello"),
            InboundDecision::Forward
        ));

        let body = reply(inbound_decision_blocking(
            &home,
            &group_id,
            "telegram",
            "chat-1",
            "/unsubscribe",
        ));
        assert!(body.contains("removed"));
        let state = cccc_core::im_state::load(&store, &group_id).expect("state");
        assert!(state["authorized"].as_array().expect("array").is_empty());
    }
}
