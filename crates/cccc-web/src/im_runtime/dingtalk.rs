use super::{
    accepts_inbound, dispatch_inbound, outbound_text, resolve_credential, spawn_outbound, string,
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
) -> Result<Vec<JoinHandle<()>>, String> {
    let app_key = resolve_credential(&string(config, "dingtalk_app_key"))?;
    let app_secret = resolve_credential(&string(config, "dingtalk_app_secret"))?;
    let sessions = Arc::new(Mutex::new(load_sessions(&home, group_id)));
    let handler = Handler {
        daemon,
        home: home.clone(),
        group_id: group_id.to_owned(),
        sessions: Arc::clone(&sessions),
    };
    let credential = Credential::new(app_key, app_secret);
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
        OutboundSender {
            home,
            group_id: group_id.to_owned(),
            sessions,
            media,
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
        let text = message
            .text
            .as_ref()
            .and_then(|text| text.content.as_deref())
            .map(str::trim)
            .unwrap_or_default();
        if chat_id.is_empty() || text.is_empty() {
            return (AckMessage::STATUS_OK, "ignored non-text message".into());
        }
        if let Some(url) = message
            .session_webhook
            .clone()
            .filter(|url| !url.is_empty())
        {
            let expires_at = message
                .session_webhook_expired_time
                .map_or(i64::MAX, normalize_epoch_seconds);
            if let Err(error) = save_session(&self.home, &self.group_id, &chat_id, &url, expires_at)
            {
                tracing::warn!(%error, %chat_id, "failed to persist DingTalk session webhook");
            }
            self.sessions
                .lock()
                .expect("DingTalk session registry poisoned")
                .insert(chat_id.clone(), SessionWebhook { url, expires_at });
        }
        if !accepts_inbound(&self.home, &self.group_id, PLATFORM, &chat_id, text) {
            return (AckMessage::STATUS_OK, "ignored unauthorized chat".into());
        }
        let sender = message
            .sender_staff_id
            .or(message.sender_id)
            .unwrap_or_else(|| "user".into());
        match dispatch_inbound(
            &self.daemon,
            &self.group_id,
            PLATFORM,
            &chat_id,
            &sender,
            text,
        )
        .await
        {
            Ok(()) => (AckMessage::STATUS_OK, "OK".into()),
            Err(error) => (AckMessage::STATUS_SYSTEM_EXCEPTION, error),
        }
    }
}

#[derive(Clone)]
struct SessionWebhook {
    url: String,
    expires_at: i64,
}

struct OutboundSender {
    home: HomeLayout,
    group_id: String,
    sessions: Arc<Mutex<HashMap<String, SessionWebhook>>>,
    media: DingTalkStreamClient,
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
    let targets = live_webhooks(&sender.sessions, &authorized.into_iter().collect());
    let http = reqwest::Client::new();

    for attachment in attachments {
        let Some(path) = attachment.get("path").and_then(Value::as_str) else {
            continue;
        };
        let Ok(path) = cccc_core::blobs::resolve(&sender.home, &sender.group_id, path) else {
            tracing::warn!(attachment = %path, "ignored invalid DingTalk attachment path");
            continue;
        };
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        if metadata.len() > 10 * 1024 * 1024 {
            tracing::warn!(attachment = %path.display(), "ignored oversized DingTalk attachment");
            continue;
        }
        let Ok(raw) = tokio::fs::read(&path).await else {
            continue;
        };
        let title = attachment
            .get("title")
            .and_then(Value::as_str)
            .and_then(safe_filename)
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .unwrap_or("file");
        let mime = attachment
            .get("mime_type")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .to_string()
            });
        let is_image = mime.starts_with("image/");
        let file_type = if is_image { "image" } else { "file" };
        let media_id = match sender
            .media
            .upload_to_dingtalk(&raw, file_type, title, &mime)
            .await
        {
            Ok(media_id) => media_id,
            Err(error) => {
                tracing::warn!(%error, attachment = %title, "failed to upload DingTalk attachment");
                continue;
            }
        };
        let payload = attachment_payload(&media_id, title, is_image);
        for url in &targets {
            if let Err(error) = post_webhook(&http, url, &payload).await {
                tracing::warn!(%error, attachment = %title, "failed to send DingTalk attachment");
            }
        }
    }

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

fn attachment_payload(media_id: &str, filename: &str, is_image: bool) -> Value {
    if is_image {
        json!({"msgtype":"image","image":{"picURL":media_id}})
    } else {
        let file_type = std::path::Path::new(filename)
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("file");
        json!({
            "msgtype":"file",
            "file":{"mediaId":media_id,"fileType":file_type}
        })
    }
}

fn safe_filename(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty() && !value.contains(['/', '\\'])).then_some(value)
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
                },
            ))
        })
        .collect()
}

fn save_session(
    home: &HomeLayout,
    group_id: &str,
    chat_id: &str,
    url: &str,
    expires_at: i64,
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
    value["conversations"][chat_id] = json!({
        "session_webhook":url,
        "session_webhook_expires_at":expires_at,
    });
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
        save_session(
            &home,
            &group.group_id,
            "chat-1",
            "https://example.test/hook",
            1_800_000_000,
        )
        .expect("save");
        let sessions = load_sessions(&home, &group.group_id);
        assert_eq!(sessions["chat-1"].url, "https://example.test/hook");
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
                },
            ),
            (
                "unauthorized".to_owned(),
                SessionWebhook {
                    url: "https://example.test/unauthorized".into(),
                    expires_at: future,
                },
            ),
            (
                "expired".to_owned(),
                SessionWebhook {
                    url: "https://example.test/expired".into(),
                    expires_at: past,
                },
            ),
        ]));
        let urls = live_webhooks(
            &sessions,
            &HashSet::from(["allowed".to_owned(), "expired".to_owned()]),
        );
        assert_eq!(urls, vec!["https://example.test/allowed"]);
    }

    #[test]
    fn attachment_payloads_match_dingtalk_webhook_contract() {
        assert_eq!(
            attachment_payload("@media", "logo.png", true),
            json!({"msgtype":"image","image":{"picURL":"@media"}})
        );
        assert_eq!(
            attachment_payload("@file", "README.md", false),
            json!({"msgtype":"file","file":{"mediaId":"@file","fileType":"md"}})
        );
    }
}
