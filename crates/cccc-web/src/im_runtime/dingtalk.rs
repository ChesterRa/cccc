use super::dingtalk_inbound::{DingTalkAttachmentDownloader, has_attachments, inbound_text};
use super::dingtalk_outbound::{DingTalkAttachmentSender, DingTalkTarget};
use super::processing_reactions::DingTalkReactions;
use super::{
    InboundDecision, InboundMetadata, dispatch_inbound_with, inbound_decision, outbound_text,
    resolve_credential, spawn_outbound, string,
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
    let media = DingTalkStreamClient::builder(credential.clone()).build();
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
    let outbound = spawn_outbound(
        home.clone(),
        group_id.to_owned(),
        ledger_events,
        OutboundSender {
            attachments: DingTalkAttachmentSender::new(home, group_id, media, robot_code),
            sessions,
            reactions,
        },
        |sender, authorized, event| async move {
            send_outbound(&sender, authorized, event).await;
        },
    );
    Ok(vec![connection, outbound])
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
            InboundDecision::Ignore => {
                return (AckMessage::STATUS_OK, "ignored unauthorized chat".into());
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
        match dispatch_inbound_with(
            &self.daemon,
            &self.group_id,
            PLATFORM,
            &chat_id,
            &sender,
            &text,
            InboundMetadata {
                message_id: message_id.clone(),
                attachments,
            },
        )
        .await
        {
            Ok(()) => {
                self.reactions.start(&chat_id, &message_id).await;
                (AckMessage::STATUS_OK, "OK".into())
            }
            Err(error) => (AckMessage::STATUS_SYSTEM_EXCEPTION, error),
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
    attachments: DingTalkAttachmentSender,
    sessions: Arc<Mutex<HashMap<String, SessionWebhook>>>,
    reactions: DingTalkReactions,
}

async fn send_outbound(sender: &OutboundSender, authorized: Vec<String>, event: Event) {
    let body = outbound_text(&event, true);
    let attachments = event
        .data
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if body.is_none() && attachments.is_empty() {
        return;
    }
    let authorized: HashSet<String> = authorized.into_iter().collect();
    let attachment_targets = known_authorized_chats(&sender.sessions, &authorized);
    sender
        .attachments
        .send(&attachment_targets, &attachments)
        .await;
    let targets = live_webhooks(&sender.sessions, &authorized);
    let http = reqwest::Client::new();

    if let Some(body) = body {
        let payload = json!({
            "msgtype":"markdown",
            "markdown":{"title":"CCCC","text":body}
        });
        for url in targets {
            if let Err(error) = post_webhook(&http, &url, &payload).await {
                tracing::warn!(%error, "failed to send DingTalk IM message");
            }
        }
    }
    for chat_id in authorized {
        sender.reactions.complete(&chat_id).await;
    }
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
) -> Vec<String> {
    let now = chrono::Utc::now().timestamp();
    let mut sessions = sessions.lock().expect("DingTalk session registry poisoned");
    sessions.retain(|_, session| session.expires_at > now);
    sessions
        .iter()
        .filter(|(chat_id, _)| authorized.contains(*chat_id))
        .map(|(_, session)| session.url.clone())
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
    fn outbound_webhooks_are_limited_to_authorized_live_sessions() {
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
        assert_eq!(urls, vec!["https://example.test/allowed"]);
        assert_eq!(
            known_authorized_chats(
                &sessions,
                &HashSet::from(["allowed".to_owned(), "weixin-chat".to_owned()])
            ),
            vec![DingTalkTarget {
                chat_id: "allowed".into(),
                robot_code: "allowed-robot".into(),
                conversation_type: "2".into(),
                user_id: String::new(),
            }]
        );
    }
}
