use super::inbound_attachments::MAX_ATTACHMENT_BYTES;
use super::outbound_attachment::safe_filename;
use super::outbound_chunks::{fits_message, split_message};
use super::outbound_stream_state::trim_active;
use super::{AuthorizedChat, outbound_text};
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use serde_json::Value;
use serenity::all::{ChannelId, CreateAttachment, CreateMessage, EditMessage, MessageId};
use serenity::http::Http;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;
const MAX_MESSAGE_CHARS: usize = 2_000;
const STREAM_THROTTLE: Duration = Duration::from_millis(300);

pub(super) struct DiscordOutbound {
    home: HomeLayout,
    group_id: String,
    http: Arc<Http>,
    streams: Mutex<HashMap<(String, String), DiscordStream>>,
    completed: Mutex<HashSet<(String, String)>>,
}

#[derive(Clone, Copy)]
struct DiscordStream {
    message_id: MessageId,
    last_update: Option<Instant>,
}

#[derive(Clone)]
struct PreparedAttachment {
    raw: Vec<u8>,
    title: String,
}

impl DiscordOutbound {
    pub(super) fn new(home: HomeLayout, group_id: &str, http: Arc<Http>) -> Self {
        Self {
            home,
            group_id: group_id.to_owned(),
            http,
            streams: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashSet::new()),
        }
    }

    pub(super) async fn send_target(
        &self,
        target: &AuthorizedChat,
        event: &Event,
    ) -> Result<(), String> {
        let channel_id = target
            .chat_id
            .parse::<u64>()
            .map(ChannelId::new)
            .map_err(|_| format!("invalid Discord channel id: {}", target.chat_id))?;
        if event.kind == "chat.stream" {
            return self.send_stream(target, channel_id, event).await;
        }
        let body = outbound_text(event, true).unwrap_or_default();
        let stream_id = event_string(event, "stream_id");
        let streamed = !stream_id.is_empty()
            && self
                .completed
                .lock()
                .expect("Discord completed stream registry poisoned")
                .remove(&(stream_id, target.key()));
        let chunks = if streamed {
            Vec::new()
        } else {
            split_message(&body, MAX_MESSAGE_CHARS, None)
        };
        let values = event
            .data
            .get("attachments")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if values.is_empty() {
            for chunk in chunks {
                channel_id
                    .say(self.http.as_ref(), chunk)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            return Ok(());
        }

        let mut next_chunk = 0;
        let mut sent = false;
        for values in values.chunks(MAX_ATTACHMENTS_PER_MESSAGE) {
            let attachments = self.prepare_available(values).await;
            if attachments.is_empty() {
                continue;
            }
            let files = attachments
                .iter()
                .map(|item| CreateAttachment::bytes(item.raw.clone(), item.title.clone()));
            let message = if let Some(chunk) = chunks.get(next_chunk) {
                next_chunk += 1;
                CreateMessage::new().content(chunk)
            } else {
                CreateMessage::new()
            };
            channel_id
                .send_files(self.http.as_ref(), files, message)
                .await
                .map_err(|error| error.to_string())?;
            sent = true;
        }
        for chunk in &chunks[next_chunk..] {
            channel_id
                .say(self.http.as_ref(), chunk)
                .await
                .map_err(|error| error.to_string())?;
            sent = true;
        }
        if !sent {
            return Err("Discord event has no valid text or attachments".into());
        }
        Ok(())
    }

    async fn send_stream(
        &self,
        target: &AuthorizedChat,
        channel_id: ChannelId,
        event: &Event,
    ) -> Result<(), String> {
        let op = event_string(event, "op");
        let stream_id = event_string(event, "stream_id");
        if stream_id.is_empty() || !matches!(op.as_str(), "start" | "update" | "end") {
            return Ok(());
        }
        let raw = event
            .data
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = outbound_text(event, true).unwrap_or_default();
        let preview_body = if raw.is_empty() {
            format!("{body}…")
        } else {
            body.clone()
        };
        let preview = split_message(&preview_body, MAX_MESSAGE_CHARS, None)
            .into_iter()
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "…".into());
        let key = (stream_id, target.key());
        if op == "start" {
            let message = channel_id
                .say(self.http.as_ref(), &preview)
                .await
                .map_err(|error| error.to_string())?;
            let mut streams = self
                .streams
                .lock()
                .expect("Discord stream registry poisoned");
            streams.insert(
                key,
                DiscordStream {
                    message_id: message.id,
                    last_update: None,
                },
            );
            trim_active(&mut streams);
            return Ok(());
        }
        let stream = {
            let mut streams = self
                .streams
                .lock()
                .expect("Discord stream registry poisoned");
            if op == "end" {
                streams.remove(&key)
            } else {
                streams.get(&key).copied().filter(|stream| {
                    stream
                        .last_update
                        .is_none_or(|last| last.elapsed() >= STREAM_THROTTLE)
                })
            }
        };
        let Some(stream) = stream else { return Ok(()) };
        channel_id
            .edit_message(
                self.http.as_ref(),
                stream.message_id,
                EditMessage::new().content(&preview),
            )
            .await
            .map_err(|error| error.to_string())?;
        if op == "end" {
            if !raw.is_empty() && fits_message(&body, MAX_MESSAGE_CHARS, None) {
                self.mark_completed(key);
            }
        } else if let Some(stream) = self
            .streams
            .lock()
            .expect("Discord stream registry poisoned")
            .get_mut(&key)
        {
            stream.last_update = Some(Instant::now());
        }
        Ok(())
    }

    fn mark_completed(&self, key: (String, String)) {
        let mut completed = self
            .completed
            .lock()
            .expect("Discord completed stream registry poisoned");
        completed.insert(key);
        while completed.len() > 4_096 {
            let Some(key) = completed.iter().next().cloned() else {
                break;
            };
            completed.remove(&key);
        }
    }

    async fn prepare_available(&self, values: &[Value]) -> Vec<PreparedAttachment> {
        let mut attachments = Vec::with_capacity(values.len());
        for value in values {
            match self.prepare(value).await {
                Ok(attachment) => attachments.push(attachment),
                Err(error) => {
                    tracing::warn!(%error, "skipped invalid Discord attachment");
                }
            }
        }
        attachments
    }

    async fn prepare(&self, value: &Value) -> Result<PreparedAttachment, String> {
        let relative = value
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "attachment path is missing".to_owned())?;
        let path = cccc_core::blobs::resolve(&self.home, &self.group_id, relative)
            .map_err(|error| error.to_string())?;
        if path.metadata().map_err(|error| error.to_string())?.len() > MAX_ATTACHMENT_BYTES {
            return Err("attachment exceeds 10 MiB before read".into());
        }
        let raw = tokio::fs::read(&path)
            .await
            .map_err(|error| error.to_string())?;
        if raw.len() as u64 > MAX_ATTACHMENT_BYTES {
            return Err("attachment exceeds 10 MiB after read".into());
        }
        let title = value
            .get("title")
            .and_then(Value::as_str)
            .and_then(safe_filename)
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .unwrap_or("file")
            .to_owned();
        Ok(PreparedAttachment { raw, title })
    }
}

fn event_string(event: &Event, key: &str) -> String {
    event
        .data
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn prepares_blob_with_original_safe_filename() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("discord", "")
            .expect("group");
        let blob = cccc_core::blobs::store(&home, &group.group_id, b"file-bytes").expect("blob");
        let outbound = DiscordOutbound::new(home, &group.group_id, Arc::new(Http::new("token")));

        let attachment = outbound
            .prepare(&json!({"path":blob.path,"title":"PROJECT.md"}))
            .await
            .expect("attachment");
        assert_eq!(attachment.title, "PROJECT.md");
        assert_eq!(attachment.raw, b"file-bytes");
    }

    #[tokio::test]
    async fn rejects_unsafe_attachment_title() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("discord", "")
            .expect("group");
        let blob = cccc_core::blobs::store(&home, &group.group_id, b"file").expect("blob");
        let outbound = DiscordOutbound::new(home, &group.group_id, Arc::new(Http::new("token")));

        let attachment = outbound
            .prepare(&json!({"path":blob.path,"title":"../secret.txt"}))
            .await
            .expect("attachment");
        assert_ne!(attachment.title, "../secret.txt");

        let attachment = outbound
            .prepare(&json!({"path":blob.path,"title":"..\\secret.txt"}))
            .await
            .expect("attachment");
        assert_ne!(attachment.title, "..\\secret.txt");
    }

    #[tokio::test]
    async fn keeps_valid_attachments_when_a_sibling_is_invalid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("discord", "")
            .expect("group");
        let blob = cccc_core::blobs::store(&home, &group.group_id, b"valid").expect("blob");
        let outbound = DiscordOutbound::new(home, &group.group_id, Arc::new(Http::new("token")));

        let attachments = outbound
            .prepare_available(&[
                json!({"path":"state/blobs/missing"}),
                json!({"path":blob.path,"title":"valid.txt"}),
            ])
            .await;

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].title, "valid.txt");
        assert_eq!(attachments[0].raw, b"valid");
    }
}
