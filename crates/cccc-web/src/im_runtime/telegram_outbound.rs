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
use teloxide::types::{InputFile, MessageId, ReplyParameters, ThreadId};

const MAX_MESSAGE_CHARS: usize = 4_096;
const STREAM_THROTTLE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SendOutcome {
    Delivered,
    Skipped,
}

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
    ) -> Result<SendOutcome, String> {
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
        // `reply_to` carries a source event id; resolve it to the Telegram
        // message id so the reply threads onto the original inbound message.
        // Unresolvable targets fall back to a plain send and are logged.
        let reply_to = if streamed {
            None
        } else {
            self.resolve_reply_target(target, event)
        };
        let mut first_error = None;
        let mut outcome = SendOutcome::Skipped;
        if !streamed {
            for (index, chunk) in split_message(&body, MAX_MESSAGE_CHARS, None)
                .into_iter()
                .enumerate()
            {
                let reply = (index == 0).then_some(reply_to).flatten();
                match self.send_text(target, &chunk, reply).await {
                    Ok(_) => outcome = SendOutcome::Delivered,
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
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
                Ok(attachment) => match self.send_attachment(target, attachment).await {
                    Ok(()) => outcome = SendOutcome::Delivered,
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                },
                Err(error) => {
                    tracing::warn!(%error, "skipped invalid Telegram attachment");
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(outcome), Err)
    }

    async fn send_stream(
        &self,
        target: &AuthorizedChat,
        event: &Event,
    ) -> Result<SendOutcome, String> {
        let op = event_string(event, "op");
        let stream_id = event_string(event, "stream_id");
        if stream_id.is_empty() || !matches!(op.as_str(), "start" | "update" | "end") {
            return Ok(SendOutcome::Skipped);
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
            let message = self.send_text(target, &preview, None).await?;
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
            return Ok(SendOutcome::Delivered);
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
        let Some(stream) = stream else {
            return Ok(SendOutcome::Skipped);
        };
        let mut outcome = SendOutcome::Skipped;
        if stream.last_text != preview {
            let chat_id = parse_chat_id(target)?;
            self.bot
                .edit_message_text(chat_id, stream.message_id, &preview)
                .await
                .map_err(|error| error.to_string())?;
            outcome = SendOutcome::Delivered;
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
        Ok(outcome)
    }

    async fn send_text(
        &self,
        target: &AuthorizedChat,
        text: &str,
        reply_to: Option<MessageId>,
    ) -> Result<Message, String> {
        let chat_id = parse_chat_id(target)?;
        let mut request = self.bot.send_message(chat_id, text);
        if let Some(message_id) = reply_to {
            request = request.reply_parameters(
                // `allow_sending_without_reply` keeps a deleted source message
                // from failing the whole send.
                ReplyParameters::new(message_id).allow_sending_without_reply(),
            );
        }
        match parse_thread_id(target)? {
            Some(thread_id) => request.message_thread_id(thread_id).await,
            None => request.await,
        }
        .map_err(|error| error.to_string())
    }

    /// Maps `data.reply_to` (a CCCC event id) to the Telegram `MessageId` of the
    /// inbound message that produced it. Inbound events stamp
    /// `source_message_id` as `{chat_id}:{message_id}`; a reply only threads
    /// when the source was posted into this same chat.
    fn resolve_reply_target(&self, target: &AuthorizedChat, event: &Event) -> Option<MessageId> {
        let reply_to = event.data.get("reply_to")?.as_str()?.trim();
        if reply_to.is_empty() {
            return None;
        }
        let resolved = (|| {
            let store = cccc_core::GroupStore::new(self.home.clone()).ok()?;
            let path = store.ledger_path(&self.group_id).ok()?;
            let source = cccc_core::ledger::find_event(&path, reply_to)
                .ok()
                .flatten()?;
            let source_id = source.data.get("source_message_id")?.as_str()?.trim();
            let (chat_id, message_id) = source_id.split_once(':')?;
            if chat_id != target.chat_id {
                return None;
            }
            message_id.trim().parse::<i32>().ok().map(MessageId)
        })();
        if resolved.is_none() {
            // The source event is missing, carries no Telegram id, or belongs
            // to another chat: send as a plain message and leave a log line.
            super::bridge_log_line(
                &self.home,
                &self.group_id,
                "telegram",
                "WARN",
                "reply target unresolved; sent as plain message",
                serde_json::Map::from_iter([("reply_to".into(), Value::from(reply_to))]),
            );
        }
        resolved
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
        send_bodies: Mutex<Vec<Value>>,
        fail_send: std::sync::atomic::AtomicBool,
    }

    async fn telegram_api(
        State(calls): State<Arc<TelegramApiCalls>>,
        uri: Uri,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let path = uri.path().trim_end_matches('/').to_ascii_lowercase();
        if path.ends_with("/sendmessage") {
            calls.send_message.fetch_add(1, Ordering::Relaxed);
            calls.send_bodies.lock().expect("bodies").push(body);
            if calls.fail_send.load(Ordering::Relaxed) {
                return Json(
                    json!({"ok":false,"error_code":403,"description":"fixture send denied"}),
                );
            }
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
    async fn skipped_stream_updates_preserve_delivery_error_until_an_actual_send() {
        let calls = Arc::new(TelegramApiCalls::default());
        calls.fail_send.store(true, Ordering::Relaxed);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let app = Router::new()
            .fallback(telegram_api)
            .with_state(calls.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });
        let bot = Bot::new("fixture-token")
            .set_api_url(reqwest::Url::parse(&format!("http://{address}")).expect("fixture API"));
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let group = store.create("telegram", "").expect("group");
        let outbound = TelegramOutbound::new(home.clone(), &group.group_id, bot);
        let target = AuthorizedChat {
            chat_id: "42".into(),
            thread_id: String::new(),
            verbose: false,
            relay: crate::im_runtime::RelayMode::Mentions,
        };
        let mut event = Event::new("chat.stream", &group.group_id);
        event.by = "foreman".into();
        event.data = json!({"op":"start","stream_id":"stream","text":"answer","to":["user"]})
            .as_object()
            .expect("data")
            .clone();
        let result = outbound.send_target(&target, &event).await;
        assert!(result.is_err());
        super::super::telegram::record_send_result(
            &home,
            &group.group_id,
            &target.key(),
            &event.id,
            result,
        );
        let read_status = || cccc_core::im_state::load(&store, &group.group_id).expect("status");
        let failed = read_status();
        assert!(
            failed["last_error"]
                .as_str()
                .expect("failure")
                .contains("fixture send denied")
        );
        for op in ["update", "end", "invalid"] {
            event.data.insert("op".into(), json!(op));
            let result = outbound.send_target(&target, &event).await;
            assert!(result.is_ok());
            super::super::telegram::record_send_result(
                &home,
                &group.group_id,
                &target.key(),
                &event.id,
                result,
            );
            assert_eq!(calls.send_message.load(Ordering::Relaxed), 1);
            assert_eq!(calls.edit_message.load(Ordering::Relaxed), 0);
            assert_eq!(
                read_status(),
                failed,
                "a skipped operation is not delivery recovery"
            );
        }
        let log_path = store
            .group_dir(&group.group_id)
            .expect("group dir")
            .join("state/im_bridge.log");
        let log = std::fs::read_to_string(&log_path).expect("failure log");
        assert!(!log.contains("outbound send delivered"));
        calls.fail_send.store(false, Ordering::Relaxed);
        event.kind = "chat.message".into();
        let result = outbound.send_target(&target, &event).await;
        assert!(result.is_ok());
        super::super::telegram::record_send_result(
            &home,
            &group.group_id,
            &target.key(),
            &event.id,
            result,
        );
        let recovered = read_status();
        assert_eq!(calls.send_message.load(Ordering::Relaxed), 2);
        assert_eq!(recovered["last_error"], Value::Null);
        assert!(recovered["last_send_ok_at"].is_string());
        assert!(
            std::fs::read_to_string(log_path)
                .expect("log")
                .contains("outbound send delivered")
        );
        server.abort();
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
            relay: crate::im_runtime::RelayMode::Mentions,
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

        assert_eq!(
            outbound
                .send_target(&target, &event("chat.stream", Some("start")))
                .await,
            Ok(SendOutcome::Delivered)
        );
        assert_eq!(
            outbound
                .send_target(&target, &event("chat.stream", Some("update")))
                .await,
            Ok(SendOutcome::Skipped)
        );
        let mut throttled = event("chat.stream", Some("update"));
        throttled
            .data
            .insert("text".into(), json!("changed but throttled"));
        assert_eq!(
            outbound.send_target(&target, &throttled).await,
            Ok(SendOutcome::Skipped)
        );
        assert_eq!(
            outbound
                .send_target(&target, &event("chat.stream", Some("end")))
                .await,
            Ok(SendOutcome::Skipped)
        );
        assert_eq!(
            outbound
                .send_target(&target, &event("chat.message", None))
                .await,
            Ok(SendOutcome::Skipped)
        );

        assert_eq!(calls.send_message.load(Ordering::Relaxed), 1);
        assert_eq!(calls.edit_message.load(Ordering::Relaxed), 0);
        assert_eq!(calls.unexpected.load(Ordering::Relaxed), 0);
        server.abort();
    }

    #[tokio::test]
    async fn reply_target_threads_onto_the_inbound_message() {
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
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let group = store.create("telegram", "").expect("group");
        // Inbound source event stamped with its Telegram id: chat 42, message 777.
        let path = store.ledger_path(&group.group_id).expect("ledger");
        let mut source = Event::new("chat.message", &group.group_id);
        source.by = "user".into();
        source.data = json!({"source_message_id":"42:777","text":"hello"})
            .as_object()
            .cloned()
            .expect("source data");
        cccc_core::ledger::append(&path, &source).expect("append");
        let outbound = TelegramOutbound::new(home, &group.group_id, bot);
        let target = AuthorizedChat {
            chat_id: "42".into(),
            thread_id: String::new(),
            verbose: false,
            relay: crate::im_runtime::RelayMode::Mentions,
        };
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "foreman".into();
        event.data = json!({"text":"reply text","reply_to":source.id,"to":["user"]})
            .as_object()
            .cloned()
            .expect("event data");

        outbound
            .send_target(&target, &event)
            .await
            .expect("reply send");

        {
            let bodies = calls.send_bodies.lock().expect("bodies");
            assert_eq!(bodies.len(), 1);
            assert_eq!(bodies[0]["reply_parameters"]["message_id"], json!(777));
        }

        // A reply_to that does not resolve (different chat) falls back to a plain send.
        let mut other_chat = Event::new("chat.message", &group.group_id);
        other_chat.by = "user".into();
        other_chat.data = json!({"source_message_id":"999:555","text":"elsewhere"})
            .as_object()
            .cloned()
            .expect("data");
        cccc_core::ledger::append(&path, &other_chat).expect("append");
        let mut fallback = Event::new("chat.message", &group.group_id);
        fallback.by = "foreman".into();
        fallback.data = json!({"text":"plain","reply_to":other_chat.id,"to":["user"]})
            .as_object()
            .cloned()
            .expect("event data");
        outbound
            .send_target(&target, &fallback)
            .await
            .expect("fallback send");
        let bodies = calls.send_bodies.lock().expect("bodies");
        assert_eq!(bodies.len(), 2);
        assert!(bodies[1].get("reply_parameters").is_none());
        server.abort();
    }
}
