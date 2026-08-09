use super::inbound_attachments::MAX_ATTACHMENT_BYTES;
use super::outbound_attachment::safe_filename;
use super::outbound_chunks::{fits_message, split_message};
use super::outbound_stream_state::trim_active;
use super::{AuthorizedChat, outbound_text};
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use teloxide::payloads::{SendDocumentSetters, SendMessageSetters, SendPhotoSetters};
use teloxide::prelude::*;
use teloxide::types::{InputFile, MessageId, ThreadId};

const MAX_MESSAGE_CHARS: usize = 4_096;
const STREAM_THROTTLE: Duration = Duration::from_millis(300);

pub(super) struct TelegramOutbound {
    home: HomeLayout,
    group_id: String,
    bot: Bot,
    streams: Mutex<HashMap<(String, String), TelegramStream>>,
    completed: Mutex<HashSet<(String, String)>>,
}

#[derive(Clone)]
struct TelegramStream {
    message_id: MessageId,
    last_update: Option<Instant>,
    last_text: String,
}

struct PreparedAttachment {
    raw: Vec<u8>,
    title: String,
    is_photo: bool,
}

impl TelegramOutbound {
    pub(super) fn new(home: HomeLayout, group_id: &str, bot: Bot) -> Self {
        Self {
            home,
            group_id: group_id.to_owned(),
            bot,
            streams: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashSet::new()),
        }
    }

    pub(super) async fn send_target(
        &self,
        target: &AuthorizedChat,
        event: &Event,
    ) -> Result<(), String> {
        if event.kind == "chat.stream" {
            return self.send_stream(target, event).await;
        }
        let body = outbound_text(event, false).unwrap_or_default();
        let stream_id = event_string(event, "stream_id");
        let streamed = !stream_id.is_empty()
            && self
                .completed
                .lock()
                .expect("Telegram completed stream registry poisoned")
                .remove(&(stream_id, target.key()));
        let mut first_error = None;
        if !streamed {
            for chunk in split_message(&body, MAX_MESSAGE_CHARS, None) {
                if let Err(error) = self.send_text(target, &chunk).await {
                    first_error.get_or_insert(error);
                }
            }
        }
        let attachments = event
            .data
            .get("attachments")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for value in attachments {
            match self.prepare(value).await {
                Ok(attachment) => {
                    if let Err(error) = self.send_attachment(target, attachment).await {
                        first_error.get_or_insert(error);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "skipped invalid Telegram attachment");
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn send_stream(&self, target: &AuthorizedChat, event: &Event) -> Result<(), String> {
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
        let body = outbound_text(event, false).unwrap_or_default();
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
            let message = self.send_text(target, &preview).await?;
            let mut streams = self
                .streams
                .lock()
                .expect("Telegram stream registry poisoned");
            streams.insert(
                key,
                TelegramStream {
                    message_id: message.id,
                    last_update: None,
                    last_text: preview,
                },
            );
            trim_active(&mut streams);
            return Ok(());
        }
        let stream = {
            let mut streams = self
                .streams
                .lock()
                .expect("Telegram stream registry poisoned");
            if op == "end" {
                streams.remove(&key)
            } else {
                streams.get(&key).cloned().filter(|stream| {
                    stream
                        .last_update
                        .is_none_or(|last| last.elapsed() >= STREAM_THROTTLE)
                })
            }
        };
        let Some(stream) = stream else { return Ok(()) };
        if stream.last_text != preview {
            let chat_id = parse_chat_id(target)?;
            self.bot
                .edit_message_text(chat_id, stream.message_id, &preview)
                .await
                .map_err(|error| error.to_string())?;
        }
        if op == "end" {
            if !raw.is_empty() && fits_message(&body, MAX_MESSAGE_CHARS, None) {
                self.mark_completed(key);
            }
        } else if let Some(stream) = self
            .streams
            .lock()
            .expect("Telegram stream registry poisoned")
            .get_mut(&key)
        {
            stream.last_update = Some(Instant::now());
            stream.last_text = preview;
        }
        Ok(())
    }

    async fn send_text(&self, target: &AuthorizedChat, text: &str) -> Result<Message, String> {
        let chat_id = parse_chat_id(target)?;
        let request = self.bot.send_message(chat_id, text);
        match parse_thread_id(target)? {
            Some(thread_id) => request.message_thread_id(thread_id).await,
            None => request.await,
        }
        .map_err(|error| error.to_string())
    }

    async fn send_attachment(
        &self,
        target: &AuthorizedChat,
        attachment: PreparedAttachment,
    ) -> Result<(), String> {
        let chat_id = parse_chat_id(target)?;
        let thread_id = parse_thread_id(target)?;
        let input = InputFile::memory(attachment.raw).file_name(attachment.title);
        if attachment.is_photo {
            let request = self.bot.send_photo(chat_id, input);
            match thread_id {
                Some(thread_id) => request.message_thread_id(thread_id).await,
                None => request.await,
            }
            .map(|_| ())
            .map_err(|error| error.to_string())
        } else {
            let request = self.bot.send_document(chat_id, input);
            match thread_id {
                Some(thread_id) => request.message_thread_id(thread_id).await,
                None => request.await,
            }
            .map(|_| ())
            .map_err(|error| error.to_string())
        }
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
        let mime = value
            .get("mime_type")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| mime_guess::from_path(&path).first_raw())
            .unwrap_or("application/octet-stream");
        Ok(PreparedAttachment {
            raw,
            title,
            is_photo: matches!(mime, "image/jpeg" | "image/png"),
        })
    }

    fn mark_completed(&self, key: (String, String)) {
        let mut completed = self
            .completed
            .lock()
            .expect("Telegram completed stream registry poisoned");
        completed.insert(key);
        while completed.len() > 4_096 {
            let Some(key) = completed.iter().next().cloned() else {
                break;
            };
            completed.remove(&key);
        }
    }
}

fn parse_chat_id(target: &AuthorizedChat) -> Result<ChatId, String> {
    target
        .chat_id
        .parse::<i64>()
        .map(ChatId)
        .map_err(|_| format!("invalid Telegram chat id: {}", target.chat_id))
}

fn parse_thread_id(target: &AuthorizedChat) -> Result<Option<ThreadId>, String> {
    if target.thread_id.is_empty() {
        return Ok(None);
    }
    target
        .thread_id
        .parse::<i32>()
        .map(|id| Some(ThreadId(MessageId(id))))
        .map_err(|_| format!("invalid Telegram thread id: {}", target.thread_id))
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
    use axum::{Json, Router, extract::State, http::Uri};
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct TelegramApiCalls {
        send_message: AtomicUsize,
        edit_message: AtomicUsize,
        unexpected: AtomicUsize,
    }

    async fn telegram_api(State(calls): State<Arc<TelegramApiCalls>>, uri: Uri) -> Json<Value> {
        let path = uri.path().trim_end_matches('/').to_ascii_lowercase();
        if path.ends_with("/sendmessage") {
            calls.send_message.fetch_add(1, Ordering::Relaxed);
        } else if path.ends_with("/editmessagetext") {
            calls.edit_message.fetch_add(1, Ordering::Relaxed);
            return Json(
                json!({"ok": false, "error_code": 400, "description": "message is not modified"}),
            );
        } else {
            calls.unexpected.fetch_add(1, Ordering::Relaxed);
        }
        Json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 1,
                "chat": {"id": 42, "type": "private"},
                "text": "complete"
            }
        }))
    }

    #[tokio::test]
    async fn prepares_safe_attachment_without_truncation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("telegram", "")
            .expect("group");
        let blob = cccc_core::blobs::store(&home, &group.group_id, b"png").expect("blob");
        let outbound = TelegramOutbound::new(home, &group.group_id, Bot::new("token"));
        let attachment = outbound
            .prepare(&json!({"path":blob.path,"title":"photo.png","mime_type":"image/png"}))
            .await
            .expect("attachment");
        assert_eq!(attachment.raw, b"png");
        assert_eq!(attachment.title, "photo.png");
        assert!(attachment.is_photo);
    }

    #[tokio::test]
    async fn unchanged_stream_end_is_completed_without_edit_or_duplicate_final_message() {
        let calls = Arc::new(TelegramApiCalls::default());
        let app = Router::new()
            .fallback(telegram_api)
            .with_state(Arc::clone(&calls));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        let bot = Bot::new("token").set_api_url(
            reqwest::Url::parse(&format!("http://{address}")).expect("Telegram test API URL"),
        );
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let outbound = TelegramOutbound::new(home, "group", bot);
        let target = AuthorizedChat {
            chat_id: "42".into(),
            thread_id: String::new(),
            verbose: false,
        };
        let event = |kind: &str, op: Option<&str>| {
            let mut event = Event::new(kind, "group");
            event.by = "foreman".into();
            event.data = json!({
                "op": op,
                "stream_id": "stream",
                "text": "complete",
                "to": ["user"]
            })
            .as_object()
            .cloned()
            .expect("event data");
            event
        };

        outbound
            .send_target(&target, &event("chat.stream", Some("start")))
            .await
            .expect("start stream");
        outbound
            .send_target(&target, &event("chat.stream", Some("end")))
            .await
            .expect("end unchanged stream");
        outbound
            .send_target(&target, &event("chat.message", None))
            .await
            .expect("suppress duplicate final message");

        assert_eq!(calls.send_message.load(Ordering::Relaxed), 1);
        assert_eq!(calls.edit_message.load(Ordering::Relaxed), 0);
        assert_eq!(calls.unexpected.load(Ordering::Relaxed), 0);
        server.abort();
    }
}
