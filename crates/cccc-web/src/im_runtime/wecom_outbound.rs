use super::outbound_attachment::safe_filename;
use super::outbound_chunks::split_message;
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
        } else if matches!(event.kind.as_str(), "chat.message" | "system.notify") {
            self.send_message(targets, &event).await;
        }
    }

    async fn send_stream(&self, targets: Vec<String>, event: &Event) {
        let op = string_data(event, "op");
        let stream_id = string_data(event, "stream_id");
        if stream_id.is_empty() || !matches!(op.as_str(), "start" | "update" | "end") {
            return;
        }
        let raw_text = raw_string_data(event, "text");
        let body = outbound_text(event, true).unwrap_or_default();
        let stream_is_complete = !raw_text.is_empty() && body.len() <= 20_480;
        let preview_body = if raw_text.is_empty() {
            format!("{body}…")
        } else {
            body
        };
        let text = truncate_utf8(&preview_body, 20_480);
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
            } else if finish && stream_is_complete && !text.is_empty() {
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
        let bodies = ordinary_message_payloads(event);
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
                .and_then(safe_filename)
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

        if bodies.is_empty() {
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
        }
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
            for body in &bodies {
                self.throttle(&chat_id).await;
                if let Err(error) = self.send_body(&chat_id, body.clone()).await {
                    tracing::warn!(%error, %chat_id, "failed to send WeCom message");
                }
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

fn ordinary_message_payloads(event: &Event) -> Vec<Value> {
    outbound_text(event, true)
        .map(|text| {
            split_message(&text, 2_048, Some(64))
                .into_iter()
                .map(|content| json!({"msgtype":"markdown","markdown":{"content":content}}))
                .collect()
        })
        .unwrap_or_default()
}

fn string_data(event: &Event, key: &str) -> String {
    raw_string_data(event, key).trim().to_owned()
}

fn raw_string_data(event: &Event, key: &str) -> String {
    event
        .data
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
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
        let payloads = ordinary_message_payloads(&chat_message(Some(" Review Bot "), "result"));
        let payload = &payloads[0];

        assert_eq!(payload["markdown"]["content"], "**Review Bot**\n\nresult");
    }

    #[test]
    fn ordinary_message_payload_falls_back_to_actor_id() {
        for sender_title in [None, Some(" \t\n ")] {
            let payloads = ordinary_message_payloads(&chat_message(sender_title, "result"));
            let payload = &payloads[0];

            assert_eq!(payload["markdown"]["content"], "**actor-id**\n\nresult");
        }
    }

    #[test]
    fn ordinary_message_payload_splits_without_losing_long_unicode_text() {
        let text = "你".repeat(3_000);
        let payloads = ordinary_message_payloads(&chat_message(Some("Review Bot"), &text));
        let content = payloads
            .iter()
            .map(|payload| payload["markdown"]["content"].as_str().expect("content"))
            .collect::<String>();

        assert!(payloads.len() > 1);
        assert_eq!(content, format!("**Review Bot**\n\n{text}"));
    }

    #[test]
    fn stream_text_preserves_leading_and_trailing_whitespace() {
        let mut event = Event::new("chat.stream", "group");
        event.data.insert("text".into(), json!("  exact reply\n"));

        assert_eq!(raw_string_data(&event, "text"), "  exact reply\n");
    }

    #[test]
    fn truncation_preserves_utf8_boundaries_and_limits() {
        let text = "你".repeat(1_000);
        let truncated = truncate_utf8(&text, 2_048);
        assert!(truncated.len() <= 2_048);
        assert!(truncated.ends_with("... (truncated)"));
    }
}
