use super::dingtalk_inbound::{DingTalkAttachmentDownloader, has_attachments, inbound_text};
use super::dingtalk_outbound::{DingTalkOutboundSender, DingTalkTarget};
use super::dingtalk_streaming::DingTalkCardStreamer;
use super::outbound_chunks::fits_message;
use super::processing_reactions::DingTalkReactions;
use super::{
    AuthorizedChat, InboundDecision, InboundMetadata, completes_processing, dispatch_inbound_with,
    inbound_decision, is_outbound_or_stream, outbound_text, processing_reply_to,
    resolve_credential, spawn_outbound_matching, string,
};
use async_trait::async_trait;
use cccc_client::DaemonClient;
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use dingtalk_stream::{
    AckMessage, CallbackHandler, ChatbotMessage, Credential, DingTalkStreamClient,
};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

const PLATFORM: &str = "dingtalk";

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<Vec<JoinHandle<()>>, String> {
    let app_key = resolve_credential(&string(config, "dingtalk_app_key"))?;
    let app_secret = resolve_credential(&string(config, "dingtalk_app_secret"))?;
    let robot_code = match string(config, "dingtalk_robot_code") {
        value if value.trim().is_empty() => app_key.clone(),
        value => resolve_credential(&value)?,
    };
    let sessions = Arc::new(Mutex::new(load_sessions(&home, group_id)));
    let credential = Credential::new(app_key, app_secret);
    let inbound_media = Arc::new(DingTalkStreamClient::builder(credential.clone()).build());
    let reactions = DingTalkReactions::new(Arc::clone(&inbound_media), robot_code.clone());
    let handler = Handler {
        daemon,
        home: home.clone(),
        group_id: group_id.to_owned(),
        sessions: Arc::clone(&sessions),
        attachments: DingTalkAttachmentDownloader::new(
            Arc::clone(&inbound_media),
            robot_code.clone(),
        ),
        reactions: reactions.clone(),
    };
    let mut stream = DingTalkStreamClient::builder(credential)
        .register_callback_handler(ChatbotMessage::TOPIC, handler)
        .build();
    stream
        .get_access_token()
        .await
        .map_err(|error| format!("DingTalk credential verification failed: {error}"))?;

    let connection = tokio::spawn(async move {
        if let Err(error) = stream.start().await {
            tracing::error!(%error, "DingTalk IM stream stopped");
        }
    });
    let reaction_cleanup = reactions.cleanup_task();
    let outbound = spawn_outbound_matching(
        home.clone(),
        group_id.to_owned(),
        PLATFORM,
        ledger_events,
        OutboundSender {
            outbound: DingTalkOutboundSender::new(
                home,
                group_id,
                Arc::clone(&inbound_media),
                robot_code.clone(),
            ),
            cards: DingTalkCardStreamer::new(Arc::clone(&inbound_media), robot_code),
            sessions,
            reactions,
        },
        is_outbound_or_stream,
        |sender, authorized, event| async move {
            send_outbound(&sender, authorized, event).await;
        },
    );
    Ok(vec![connection, outbound, reaction_cleanup])
}

#[derive(Clone)]
struct Handler {
    daemon: DaemonClient,
    home: HomeLayout,
    group_id: String,
    sessions: Arc<Mutex<HashMap<String, SessionWebhook>>>,
    attachments: DingTalkAttachmentDownloader,
    reactions: DingTalkReactions,
}

#[async_trait]
impl CallbackHandler for Handler {
    async fn process(
        &self,
        callback: &dingtalk_stream::messages::frames::MessageBody,
    ) -> (u16, String) {
        let raw: Value = match serde_json::from_str(&callback.data) {
            Ok(value) => value,
            Err(error) => return (AckMessage::STATUS_BAD_REQUEST, error.to_string()),
        };
        let message = match ChatbotMessage::from_value(&raw) {
            Ok(message) => message,
            Err(error) => return (AckMessage::STATUS_BAD_REQUEST, error.to_string()),
        };
        let chat_id = message.conversation_id.clone().unwrap_or_default();
        let text = inbound_text(&message);
        if chat_id.is_empty() || (text.is_empty() && !has_attachments(&message)) {
            return (AckMessage::STATUS_OK, "ignored empty message".into());
        }
        if let Some(url) = message
            .session_webhook
            .clone()
            .filter(|url| !url.is_empty())
        {
            let expires_at = message
                .session_webhook_expired_time
                .map_or(i64::MAX, normalize_epoch_seconds);
            let robot_code = message.robot_code.clone().unwrap_or_default();
            let conversation_type = message.conversation_type.clone().unwrap_or_default();
            let user_id = message
                .sender_staff_id
                .clone()
                .or_else(|| message.sender_id.clone())
                .unwrap_or_default();
            let session = SessionWebhook {
                url,
                expires_at,
                robot_code,
                conversation_type,
                user_id,
            };
            if let Err(error) = save_session(&self.home, &self.group_id, &chat_id, &session) {
                tracing::warn!(%error, %chat_id, "failed to persist DingTalk session webhook");
            }
            self.sessions
                .lock()
                .expect("DingTalk session registry poisoned")
                .insert(chat_id.clone(), session);
        }
        match inbound_decision(&self.home, &self.group_id, PLATFORM, &chat_id, &text).await {
            InboundDecision::Forward => {}
            InboundDecision::Reply(body) => {
                return match self.send_command_reply(&chat_id, &body).await {
                    Ok(()) => (AckMessage::STATUS_OK, "command reply sent".into()),
                    Err(error) => {
                        tracing::warn!(%error, %chat_id, "failed to send DingTalk command reply");
                        (AckMessage::STATUS_SYSTEM_EXCEPTION, error)
                    }
                };
            }
        }
        let message_id = message.message_id.clone().unwrap_or_default();
        let attachments = self
            .attachments
            .materialize(&self.home, &self.group_id, &message)
            .await;
        if text.is_empty() && attachments.is_empty() {
            return (AckMessage::STATUS_OK, "attachment download failed".into());
        }
        let sender = message
            .sender_staff_id
            .or(message.sender_id)
            .unwrap_or_else(|| "user".into());
        // Register processing feedback before dispatching the message. The daemon can produce an
        // outbound reply before `dispatch_inbound_with` returns; starting the reaction afterwards
        // lets the completion event win the race and leaves a stale Thinking reaction behind.
        self.reactions.start(&chat_id, &chat_id, &message_id).await;
        match dispatch_inbound_with(
            &self.daemon,
            &self.group_id,
            PLATFORM,
            &chat_id,
            &sender,
            &text,
            InboundMetadata {
                message_id: message_id.clone(),
                thread_id: String::new(),
                attachments,
            },
        )
        .await
        {
            Ok(source_event_id) => {
                self.reactions
                    .bind_message(&chat_id, &message_id, source_event_id);
                (AckMessage::STATUS_OK, "OK".into())
            }
            Err(error) => {
                self.reactions.fail_message(&chat_id, &message_id).await;
                (AckMessage::STATUS_SYSTEM_EXCEPTION, error)
            }
        }
    }
}

impl Handler {
    async fn send_command_reply(&self, chat_id: &str, body: &str) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        let url = self
            .sessions
            .lock()
            .expect("DingTalk session registry poisoned")
            .get(chat_id)
            .filter(|session| session.expires_at > now)
            .map(|session| session.url.clone())
            .ok_or_else(|| "DingTalk session webhook is unavailable or expired".to_owned())?;
        let payload = json!({
            "msgtype":"markdown",
            "markdown":{"title":"CCCC","text":body}
        });
        post_webhook(&reqwest::Client::new(), &url, &payload).await
    }
}

#[derive(Clone)]
struct SessionWebhook {
    url: String,
    expires_at: i64,
    robot_code: String,
    conversation_type: String,
    user_id: String,
}

struct OutboundSender {
    outbound: DingTalkOutboundSender,
    cards: DingTalkCardStreamer,
    sessions: Arc<Mutex<HashMap<String, SessionWebhook>>>,
    reactions: DingTalkReactions,
}

async fn send_outbound(sender: &OutboundSender, authorized: Vec<AuthorizedChat>, event: Event) {
    let completes_processing = completes_processing(&event);
    let reply_to = processing_reply_to(&event).map(str::to_owned);
    let authorized: HashSet<String> = authorized
        .into_iter()
        .map(|target| target.chat_id)
        .collect();
    if event.kind == "chat.stream" {
        let targets = known_authorized_chats(&sender.sessions, &authorized);
        sender.cards.send(&targets, &event).await;
        return;
    }

    let body = outbound_text(&event, true);
    let attachments = event
        .data
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let stream_id = event
        .data
        .get("stream_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let streamed_targets = sender.cards.take_completed_targets(stream_id);
    if body.is_none() && attachments.is_empty() {
        return;
    }
    let targets = known_authorized_chats(&sender.sessions, &authorized);
    let attachment_report = sender
        .outbound
        .send_attachments(&targets, &attachments)
        .await;
    let mut text_delivered_targets = streamed_targets;
    let has_body = body.is_some();

    if let Some(body) = body {
        let payload = json!({
            "msgtype":"markdown",
            "markdown":{"title":"CCCC","text":body}
        });
        let http = reqwest::Client::new();
        for (chat_id, url) in live_webhooks(&sender.sessions, &authorized) {
            if text_delivered_targets.contains(&chat_id) {
                continue;
            }
            if !fits_message(&body, 4_096, Some(64)) {
                continue;
            }
            match post_webhook(&http, &url, &payload).await {
                Ok(()) => {
                    text_delivered_targets.insert(chat_id);
                }
                Err(error) => {
                    tracing::warn!(%error, %chat_id, "failed to send DingTalk IM webhook; falling back to OpenAPI");
                }
            }
        }
        let fallback_targets = pending_text_targets(targets, &text_delivered_targets);
        text_delivered_targets.extend(sender.outbound.send_text(&fallback_targets, &body).await);
    }
    if completes_processing {
        for chat_id in authorized {
            if delivery_succeeded(
                &chat_id,
                has_body,
                &text_delivered_targets,
                !attachments.is_empty(),
                &attachment_report,
            ) {
                sender
                    .reactions
                    .complete(&chat_id, reply_to.as_deref())
                    .await;
            } else {
                sender.reactions.fail(&chat_id, reply_to.as_deref()).await;
            }
        }
    }
}

fn delivery_succeeded(
    chat_id: &str,
    has_body: bool,
    text_delivered: &HashSet<String>,
    has_attachments: bool,
    attachments: &super::dingtalk_outbound_report::AttachmentDeliveryReport,
) -> bool {
    (!has_body || text_delivered.contains(chat_id))
        && (!has_attachments
            || (attachments.delivered_chat_ids.contains(chat_id)
                && !attachments.failed_chat_ids.contains(chat_id)))
}

async fn post_webhook(http: &reqwest::Client, url: &str, payload: &Value) -> Result<(), String> {
    let response = http
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if status.is_success() && value.get("errcode").and_then(Value::as_i64).unwrap_or(0) == 0 {
        Ok(())
    } else {
        Err(value
            .get("errmsg")
            .and_then(Value::as_str)
            .unwrap_or("DingTalk webhook rejected message")
            .to_owned())
    }
}

fn live_webhooks(
    sessions: &Mutex<HashMap<String, SessionWebhook>>,
    authorized: &HashSet<String>,
) -> Vec<(String, String)> {
    let now = chrono::Utc::now().timestamp();
    let sessions = sessions.lock().expect("DingTalk session registry poisoned");
    sessions
        .iter()
        .filter(|(chat_id, session)| authorized.contains(*chat_id) && session.expires_at > now)
        .map(|(chat_id, session)| (chat_id.clone(), session.url.clone()))
        .collect()
}

fn known_authorized_chats(
    sessions: &Mutex<HashMap<String, SessionWebhook>>,
    authorized: &HashSet<String>,
) -> Vec<DingTalkTarget> {
    sessions
        .lock()
        .expect("DingTalk session registry poisoned")
        .iter()
        .filter(|(chat_id, _)| authorized.contains(*chat_id))
        .map(|(chat_id, session)| DingTalkTarget {
            chat_id: chat_id.clone(),
            robot_code: session.robot_code.clone(),
            conversation_type: session.conversation_type.clone(),
            user_id: session.user_id.clone(),
        })
        .collect()
}

fn pending_text_targets(
    targets: Vec<DingTalkTarget>,
    delivered: &HashSet<String>,
) -> Vec<DingTalkTarget> {
    targets
        .into_iter()
        .filter(|target| !delivered.contains(&target.chat_id))
        .collect()
}

fn load_sessions(home: &HomeLayout, group_id: &str) -> HashMap<String, SessionWebhook> {
    let path = home
        .groups_dir()
        .join(group_id)
        .join("state/im_dingtalk_sessions.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return HashMap::new();
    };
    value
        .get("conversations")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(chat_id, entry)| {
            Some((
                chat_id.clone(),
                SessionWebhook {
                    url: entry.get("session_webhook")?.as_str()?.to_owned(),
                    expires_at: entry.get("session_webhook_expires_at")?.as_i64().or_else(
                        || {
                            entry
                                .get("session_webhook_expires_at")?
                                .as_f64()
                                .map(|value| value as i64)
                        },
                    )?,
                    robot_code: entry
                        .get("robot_code")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    conversation_type: entry
                        .get("conversation_type")
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .map(str::to_owned)
                        .or_else(|| match entry.get("chat_type").and_then(Value::as_str) {
                            Some("p2p") => Some("1".to_owned()),
                            Some("group") => Some("2".to_owned()),
                            _ => None,
                        })
                        .unwrap_or_default(),
                    user_id: entry
                        .get("user_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                },
            ))
        })
        .collect()
}

fn save_session(
    home: &HomeLayout,
    group_id: &str,
    chat_id: &str,
    session: &SessionWebhook,
) -> Result<(), String> {
    let path = home
        .groups_dir()
        .join(group_id)
        .join("state/im_dingtalk_sessions.json");
    let mut value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .unwrap_or_else(|| json!({"conversations":{}}));
    if !value["conversations"].is_object() {
        value["conversations"] = json!({});
    }
    let mut entry = json!({
        "session_webhook":session.url,
        "session_webhook_expires_at":session.expires_at,
    });
    if !session.robot_code.is_empty() {
        entry["robot_code"] = json!(session.robot_code);
    }
    if !session.conversation_type.is_empty() {
        entry["conversation_type"] = json!(session.conversation_type);
    }
    if !session.user_id.is_empty() {
        entry["user_id"] = json!(session.user_id);
    }
    value["conversations"][chat_id] = entry;
    cccc_core::fs::write_json(&path, &value).map_err(|error| error.to_string())
}

fn normalize_epoch_seconds(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value / 1000
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_milliseconds_are_normalized() {
        assert_eq!(normalize_epoch_seconds(1_800_000_000_000), 1_800_000_000);
        assert_eq!(normalize_epoch_seconds(1_800_000_000), 1_800_000_000);
    }

    #[test]
    fn mixed_delivery_requires_both_text_and_all_attachments() {
        let chat_id = "chat-1";
        let text_delivered = HashSet::from([chat_id.to_owned()]);
        let mut attachments =
            super::super::dingtalk_outbound_report::AttachmentDeliveryReport::default();
        attachments.delivered_chat_ids.insert(chat_id.to_owned());
        assert!(delivery_succeeded(
            chat_id,
            true,
            &text_delivered,
            true,
            &attachments
        ));

        attachments.failed_chat_ids.insert(chat_id.to_owned());
        assert!(!delivery_succeeded(
            chat_id,
            true,
            &text_delivered,
            true,
            &attachments
        ));
        assert!(!delivery_succeeded(
            chat_id,
            true,
            &HashSet::new(),
            true,
            &attachments
        ));
    }

    #[test]
    fn session_webhook_is_persisted_before_authorization() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let group = store.create("dingtalk", "").expect("group");
        let session = SessionWebhook {
            url: "https://example.test/hook".into(),
            expires_at: 1_800_000_000,
            robot_code: "callback-robot".into(),
            conversation_type: "1".into(),
            user_id: "staff-1".into(),
        };
        save_session(&home, &group.group_id, "chat-1", &session).expect("save");
        let sessions = load_sessions(&home, &group.group_id);
        assert_eq!(sessions["chat-1"].url, "https://example.test/hook");
        assert_eq!(sessions["chat-1"].robot_code, "callback-robot");
        assert_eq!(sessions["chat-1"].conversation_type, "1");
        assert_eq!(sessions["chat-1"].user_id, "staff-1");
    }

    #[test]
    fn outbound_delivery_falls_back_for_expired_authorized_sessions() {
        let future = chrono::Utc::now().timestamp() + 60;
        let past = chrono::Utc::now().timestamp() - 60;
        let sessions = Mutex::new(HashMap::from([
            (
                "allowed".to_owned(),
                SessionWebhook {
                    url: "https://example.test/allowed".into(),
                    expires_at: future,
                    robot_code: "allowed-robot".into(),
                    conversation_type: "2".into(),
                    user_id: String::new(),
                },
            ),
            (
                "unauthorized".to_owned(),
                SessionWebhook {
                    url: "https://example.test/unauthorized".into(),
                    expires_at: future,
                    robot_code: "unauthorized-robot".into(),
                    conversation_type: "2".into(),
                    user_id: String::new(),
                },
            ),
            (
                "expired".to_owned(),
                SessionWebhook {
                    url: "https://example.test/expired".into(),
                    expires_at: past,
                    robot_code: "expired-robot".into(),
                    conversation_type: "2".into(),
                    user_id: String::new(),
                },
            ),
        ]));
        let urls = live_webhooks(
            &sessions,
            &HashSet::from(["allowed".to_owned(), "expired".to_owned()]),
        );
        assert_eq!(
            urls,
            vec![(
                "allowed".to_owned(),
                "https://example.test/allowed".to_owned()
            )]
        );
        let mut targets = known_authorized_chats(
            &sessions,
            &HashSet::from(["allowed".to_owned(), "expired".to_owned()]),
        );
        targets.sort_by(|left, right| left.chat_id.cmp(&right.chat_id));
        assert_eq!(
            targets,
            vec![
                DingTalkTarget {
                    chat_id: "allowed".into(),
                    robot_code: "allowed-robot".into(),
                    conversation_type: "2".into(),
                    user_id: String::new(),
                },
                DingTalkTarget {
                    chat_id: "expired".into(),
                    robot_code: "expired-robot".into(),
                    conversation_type: "2".into(),
                    user_id: String::new(),
                },
            ]
        );
        let webhook_delivered = urls
            .into_iter()
            .map(|(chat_id, _)| chat_id)
            .collect::<HashSet<_>>();
        let mut card_or_webhook_delivered = webhook_delivered.clone();
        card_or_webhook_delivered.insert("expired".into());
        assert!(pending_text_targets(targets.clone(), &card_or_webhook_delivered).is_empty());
        assert_eq!(
            pending_text_targets(targets, &webhook_delivered),
            vec![DingTalkTarget {
                chat_id: "expired".into(),
                robot_code: "expired-robot".into(),
                conversation_type: "2".into(),
                user_id: String::new(),
            }]
        );
    }
}
