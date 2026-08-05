use super::{outbound_text, wecom_client::WecomClient};
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(super) struct WecomOutbound {
    home: HomeLayout,
    group_id: String,
    client: Arc<WecomClient>,
    streams: Mutex<HashMap<(String, String), String>>,
    completed: Mutex<HashSet<(String, String)>>,
    last_send: Mutex<HashMap<String, Instant>>,
}

impl WecomOutbound {
    pub(super) fn new(home: HomeLayout, group_id: String, client: Arc<WecomClient>) -> Self {
        Self {
            home,
            group_id,
            client,
            streams: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashSet::new()),
            last_send: Mutex::new(HashMap::new()),
        }
    }

    pub(super) async fn send(&self, targets: Vec<String>, event: Event) {
        if event.kind == "chat.stream" {
            self.send_stream(targets, &event).await;
        } else if event.kind == "chat.message" {
            self.send_message(targets, &event).await;
        }
    }

    async fn send_stream(&self, targets: Vec<String>, event: &Event) {
        if !user_facing(event) {
            return;
        }
        let op = string_data(event, "op");
        let stream_id = string_data(event, "stream_id");
        if stream_id.is_empty() || !matches!(op.as_str(), "start" | "update" | "end") {
            return;
        }
        let text = truncate_utf8(&string_data(event, "text"), 20_480);
        for chat_id in targets {
            let key = (stream_id.clone(), chat_id.clone());
            let req_id = if op == "start" {
                let Some(req_id) = self.client.reply_req_id(&chat_id) else {
                    continue;
                };
                self.streams
                    .lock()
                    .expect("WeCom stream registry poisoned")
                    .insert(key.clone(), req_id.clone());
                self.trim_streams();
                req_id
            } else {
                let Some(req_id) = self
                    .streams
                    .lock()
                    .expect("WeCom stream registry poisoned")
                    .get(&key)
                    .cloned()
                else {
                    continue;
                };
                req_id
            };
            if op == "start" && text.is_empty() {
                continue;
            }
            self.throttle(&chat_id).await;
            let finish = op == "end";
            let result = self
                .client
                .reply_message(
                    &req_id,
                    json!({"msgtype":"stream","stream":{
                        "id":stream_id,"finish":finish,"content":text
                    }}),
                )
                .await;
            if let Err(error) = result {
                tracing::warn!(%error, %stream_id, %chat_id, op = %op, "failed to send WeCom stream frame");
                if op == "start" {
                    self.streams
                        .lock()
                        .expect("WeCom stream registry poisoned")
                        .remove(&key);
                }
            } else if finish && !text.is_empty() {
                self.mark_completed(key.clone());
            }
            if finish {
                self.streams
                    .lock()
                    .expect("WeCom stream registry poisoned")
                    .remove(&key);
            }
        }
    }

    async fn send_message(&self, targets: Vec<String>, event: &Event) {
        if !user_facing(event) {
            return;
        }
        let body = ordinary_message_payload(event);
        let attachments = event
            .data
            .get("attachments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let stream_id = string_data(event, "stream_id");

        for attachment in attachments {
            let Some(path) = attachment.get("path").and_then(Value::as_str) else {
                continue;
            };
            let Ok(path) = cccc_core::blobs::resolve(&self.home, &self.group_id, path) else {
                tracing::warn!(attachment = %path, "ignored invalid WeCom attachment path");
                continue;
            };
            let Ok(metadata) = path.metadata() else {
                continue;
            };
            if metadata.len() > 50 * 1024 * 1024 {
                tracing::warn!(attachment = %path.display(), "ignored oversized WeCom attachment");
                continue;
            }
            let Ok(bytes) = tokio::fs::read(&path).await else {
                continue;
            };
            let title = attachment
                .get("title")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .or_else(|| path.file_name().and_then(|value| value.to_str()))
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
            let media_type = if matches!(mime.as_str(), "image/png" | "image/jpeg") {
                "image"
            } else {
                "file"
            };
            let media_id = match self.client.upload_media(&bytes, media_type, title).await {
                Ok(media_id) => media_id,
                Err(error) => {
                    tracing::warn!(%error, attachment = %title, "failed to upload WeCom attachment");
                    continue;
                }
            };
            let media_body = if media_type == "image" {
                json!({"msgtype":"image","image":{"media_id":media_id}})
            } else {
                json!({"msgtype":"file","file":{"media_id":media_id,"filename":title}})
            };
            for chat_id in &targets {
                self.throttle(chat_id).await;
                if let Err(error) = self.send_body(chat_id, media_body.clone()).await {
                    tracing::warn!(%error, attachment = %title, %chat_id, "failed to send WeCom attachment");
                }
            }
        }

        let Some(body) = body else {
            if !stream_id.is_empty() {
                let mut completed = self
                    .completed
                    .lock()
                    .expect("WeCom completed stream registry poisoned");
                for chat_id in targets {
                    completed.remove(&(stream_id.clone(), chat_id));
                }
            }
            return;
        };
        for chat_id in targets {
            let streamed = !stream_id.is_empty()
                && self
                    .completed
                    .lock()
                    .expect("WeCom completed stream registry poisoned")
                    .remove(&(stream_id.clone(), chat_id.clone()));
            if streamed {
                continue;
            }
            self.throttle(&chat_id).await;
            if let Err(error) = self.send_body(&chat_id, body.clone()).await {
                tracing::warn!(%error, %chat_id, "failed to send WeCom message");
            }
        }
    }

    async fn send_body(&self, chat_id: &str, body: Value) -> Result<(), String> {
        if let Some(req_id) = self.client.reply_req_id(chat_id) {
            let body = if body.get("msgtype").and_then(Value::as_str) == Some("markdown") {
                let content = body
                    .pointer("/markdown/content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                json!({"msgtype":"stream","stream":{
                    "id":format!("cccc-wecom-{}", uuid::Uuid::new_v4().simple()),
                    "finish":true,"content":content
                }})
            } else {
                body
            };
            self.client.reply_message(&req_id, body).await.map(|_| ())
        } else {
            self.client.send_message(chat_id, body).await
        }
    }

    async fn throttle(&self, chat_id: &str) {
        let delay = {
            let sends = self.last_send.lock().expect("WeCom rate limiter poisoned");
            sends
                .get(chat_id)
                .and_then(|last| Duration::from_millis(200).checked_sub(last.elapsed()))
        };
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        self.last_send
            .lock()
            .expect("WeCom rate limiter poisoned")
            .insert(chat_id.to_owned(), Instant::now());
    }

    fn trim_streams(&self) {
        let mut streams = self.streams.lock().expect("WeCom stream registry poisoned");
        while streams.len() > 1_024 {
            let Some(key) = streams.keys().next().cloned() else {
                break;
            };
            streams.remove(&key);
        }
    }

    fn mark_completed(&self, key: (String, String)) {
        let mut completed = self
            .completed
            .lock()
            .expect("WeCom completed stream registry poisoned");
        completed.insert(key);
        while completed.len() > 4_096 {
            let Some(key) = completed.iter().next().cloned() else {
                break;
            };
            completed.remove(&key);
        }
    }
}

fn ordinary_message_payload(event: &Event) -> Option<Value> {
    let content = outbound_text(event, true).map(|text| truncate_message(&text))?;
    Some(json!({"msgtype":"markdown","markdown":{"content":content}}))
}

fn user_facing(event: &Event) -> bool {
    event
        .data
        .get("to")
        .and_then(Value::as_array)
        .is_none_or(|targets| {
            targets.is_empty() || targets.iter().any(|target| target.as_str() == Some("user"))
        })
}

fn string_data(event: &Event, key: &str) -> String {
    event
        .data
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn truncate_message(value: &str) -> String {
    let mut lines: Vec<&str> = value.lines().take(64).collect();
    let truncated_lines = value.lines().count() > lines.len();
    if truncated_lines {
        lines.push("... (truncated)");
    }
    truncate_utf8(&lines.join("\n"), 2_048)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let suffix = "\n... (truncated)";
    let mut end = max_bytes.saturating_sub(suffix.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}{suffix}", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_message(sender_title: Option<&str>, text: &str) -> Event {
        let mut event = Event::new("chat.message", "group");
        event.by = "actor-id".into();
        event.data.insert("to".into(), json!(["user"]));
        event.data.insert("text".into(), json!(text));
        if let Some(sender_title) = sender_title {
            event
                .data
                .insert("sender_title".into(), json!(sender_title));
        }
        event
    }

    #[test]
    fn ordinary_message_payload_prefers_trimmed_sender_title() {
        let payload = ordinary_message_payload(&chat_message(Some(" Review Bot "), "result"))
            .expect("chat.message payload");

        assert_eq!(payload["markdown"]["content"], "**Review Bot**\n\nresult");
    }

    #[test]
    fn ordinary_message_payload_falls_back_to_actor_id() {
        for sender_title in [None, Some(" \t\n ")] {
            let payload = ordinary_message_payload(&chat_message(sender_title, "result"))
                .expect("chat.message payload");

            assert_eq!(payload["markdown"]["content"], "**actor-id**\n\nresult");
        }
    }

    #[test]
    fn ordinary_message_payload_truncates_after_sender_wrapping() {
        let payload =
            ordinary_message_payload(&chat_message(Some("Review Bot"), &"你".repeat(1_000)))
                .expect("chat.message payload");
        let content = payload["markdown"]["content"]
            .as_str()
            .expect("markdown content");

        assert!(content.starts_with("**Review Bot**\n\n"));
        assert!(content.ends_with("... (truncated)"));
        assert!(content.len() <= 2_048);
    }

    #[test]
    fn truncation_preserves_utf8_boundaries_and_limits() {
        let text = "你".repeat(1_000);
        let truncated = truncate_utf8(&text, 2_048);
        assert!(truncated.len() <= 2_048);
        assert!(truncated.ends_with("... (truncated)"));
    }

    #[test]
    fn only_user_facing_events_are_forwarded() {
        let mut event = Event::new("chat.stream", "g");
        event.data.insert("to".into(), json!(["peer"]));
        assert!(!user_facing(&event));
        event.data.insert("to".into(), json!(["user"]));
        assert!(user_facing(&event));
    }
}
