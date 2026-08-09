use super::outbound_attachment::safe_filename;
use super::outbound_chunks::{fits_message, split_message};
use super::outbound_stream_state::trim_active;
use super::{AuthorizedChat, outbound_text};
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const API: &str = "https://slack.com/api";
const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 4_000;
const STREAM_THROTTLE: Duration = Duration::from_millis(300);

pub(super) struct SlackOutbound {
    home: HomeLayout,
    group_id: String,
    http: reqwest::Client,
    bot_token: String,
    api_base: String,
    streams: Mutex<HashMap<(String, String), SlackStream>>,
    completed: Mutex<HashSet<(String, String)>>,
}

struct SlackStream {
    timestamp: String,
    last_update: Option<Instant>,
}

struct PreparedAttachment {
    raw: Vec<u8>,
    title: String,
    mime: String,
}

impl SlackOutbound {
    pub(super) fn new(
        home: HomeLayout,
        group_id: &str,
        http: reqwest::Client,
        bot_token: String,
    ) -> Self {
        Self {
            home,
            group_id: group_id.into(),
            http,
            bot_token,
            api_base: API.into(),
            streams: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashSet::new()),
        }
    }

    pub(super) async fn send(&self, targets: &[AuthorizedChat], event: &Event) {
        if event.kind == "chat.stream" {
            self.send_stream(targets, event).await;
            return;
        }
        let body = outbound_text(event, false).unwrap_or_default();
        let attachments = event
            .data
            .get("attachments")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut prepared = Vec::new();
        for attachment in attachments {
            let item = match self.prepare(attachment).await {
                Ok(item) => item,
                Err(error) => {
                    tracing::warn!(%error, "failed to prepare Slack attachment");
                    continue;
                }
            };
            prepared.push(item);
        }

        let stream_id = event_string(event, "stream_id");
        for target in targets {
            let streamed = !stream_id.is_empty()
                && self
                    .completed
                    .lock()
                    .expect("Slack completed stream registry poisoned")
                    .remove(&(stream_id.clone(), target.key()));
            let chunks = if streamed {
                Vec::new()
            } else {
                split_message(&body, MAX_MESSAGE_CHARS, None)
            };
            let mut next_chunk = 0;
            if !prepared.is_empty() {
                let comment = chunks.first().map(String::as_str).unwrap_or_default();
                match self.upload_many(target, comment, &prepared).await {
                    Ok(()) => next_chunk = usize::from(!comment.is_empty()),
                    Err(error) => tracing::warn!(
                        %error,
                        channel = %target.chat_id,
                        "failed to send Slack attachments"
                    ),
                }
            }
            for chunk in &chunks[next_chunk..] {
                if let Err(error) = self.post_message(target, chunk).await {
                    tracing::warn!(%error, channel = %target.chat_id, "failed to send Slack IM message");
                }
            }
        }
    }

    async fn send_stream(&self, targets: &[AuthorizedChat], event: &Event) {
        let op = event_string(event, "op");
        let stream_id = event_string(event, "stream_id");
        if stream_id.is_empty() || !matches!(op.as_str(), "start" | "update" | "end") {
            return;
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
        for target in targets {
            let key = (stream_id.clone(), target.key());
            if op == "start" {
                match self.post_message(target, &preview).await {
                    Ok(timestamp) => {
                        let mut streams =
                            self.streams.lock().expect("Slack stream registry poisoned");
                        streams.insert(
                            key,
                            SlackStream {
                                timestamp,
                                last_update: None,
                            },
                        );
                        trim_active(&mut streams);
                    }
                    Err(error) => tracing::warn!(%error, "failed to start Slack stream preview"),
                }
                continue;
            }
            let stream = {
                let mut streams = self.streams.lock().expect("Slack stream registry poisoned");
                if op == "end" {
                    streams.remove(&key)
                } else {
                    streams.get(&key).and_then(|stream| {
                        stream
                            .last_update
                            .is_none_or(|last| last.elapsed() >= STREAM_THROTTLE)
                            .then(|| SlackStream {
                                timestamp: stream.timestamp.clone(),
                                last_update: stream.last_update,
                            })
                    })
                }
            };
            let Some(stream) = stream else { continue };
            match self
                .update_message(target, &stream.timestamp, &preview)
                .await
            {
                Ok(())
                    if op == "end"
                        && !raw.is_empty()
                        && fits_message(&body, MAX_MESSAGE_CHARS, None) =>
                {
                    self.mark_completed(key);
                }
                Ok(()) => {
                    if let Some(stream) = self
                        .streams
                        .lock()
                        .expect("Slack stream registry poisoned")
                        .get_mut(&key)
                    {
                        stream.last_update = Some(Instant::now());
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to update Slack stream preview"),
            }
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
            .unwrap_or("application/octet-stream")
            .to_owned();
        Ok(PreparedAttachment { raw, title, mime })
    }

    async fn upload_many(
        &self,
        target: &AuthorizedChat,
        comment: &str,
        attachments: &[PreparedAttachment],
    ) -> Result<(), String> {
        let mut files = Vec::new();
        for attachment in attachments {
            let length = attachment.raw.len().to_string();
            let allocation = self
                .form_call(
                    "files.getUploadURLExternal",
                    &[("filename", attachment.title.as_str()), ("length", &length)],
                )
                .await?;
            let upload_url = allocation
                .get("upload_url")
                .and_then(Value::as_str)
                .ok_or_else(|| "Slack upload allocation has no upload_url".to_owned())?;
            let file_id = allocation
                .get("file_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "Slack upload allocation has no file_id".to_owned())?;
            let response = self
                .http
                .post(upload_url)
                .header(reqwest::header::CONTENT_TYPE, &attachment.mime)
                .body(attachment.raw.clone())
                .send()
                .await
                .map_err(|error| error.to_string())?;
            if !response.status().is_success() {
                return Err(format!("Slack upload returned HTTP {}", response.status()));
            }
            files.push(json!({"id":file_id,"title":attachment.title}));
        }
        let mut body = json!({
            "files":files,
            "channel_id":target.chat_id,
            "initial_comment":comment
        });
        if !target.thread_id.is_empty() {
            body["thread_ts"] = json!(target.thread_id);
        }
        self.api_call("files.completeUploadExternal", body).await?;
        Ok(())
    }

    async fn post_message(&self, target: &AuthorizedChat, text: &str) -> Result<String, String> {
        let mut body = json!({"channel":target.chat_id,"text":text});
        if !target.thread_id.is_empty() {
            body["thread_ts"] = json!(target.thread_id);
        }
        let value = self.api_call("chat.postMessage", body).await?;
        value
            .get("ts")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "Slack message response has no timestamp".into())
    }

    async fn update_message(
        &self,
        target: &AuthorizedChat,
        timestamp: &str,
        text: &str,
    ) -> Result<(), String> {
        self.api_call(
            "chat.update",
            json!({"channel":target.chat_id,"ts":timestamp,"text":text}),
        )
        .await
        .map(|_| ())
    }

    fn mark_completed(&self, key: (String, String)) {
        let mut completed = self
            .completed
            .lock()
            .expect("Slack completed registry poisoned");
        completed.insert(key);
        while completed.len() > 4_096 {
            let Some(key) = completed.iter().next().cloned() else {
                break;
            };
            completed.remove(&key);
        }
    }

    async fn api_call(&self, method: &str, body: Value) -> Result<Value, String> {
        let response = self
            .http
            .post(format!("{}/{method}", self.api_base.trim_end_matches('/')))
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        decode_api_response(response).await
    }

    async fn form_call(&self, method: &str, form: &[(&str, &str)]) -> Result<Value, String> {
        let response = self
            .http
            .post(format!("{}/{method}", self.api_base.trim_end_matches('/')))
            .bearer_auth(&self.bot_token)
            .form(form)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        decode_api_response(response).await
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

async fn decode_api_response(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if status.is_success() && value.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(value)
    } else {
        Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Slack API request failed")
            .to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Form, Router, body::Bytes, extract::State, routing::post};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn event(kind: &str, data: Value) -> Event {
        serde_json::from_value(json!({
            "v":1,"id":uuid::Uuid::new_v4().to_string(),"ts":"now","kind":kind,
            "group_id":"group","scope_key":"","by":"assistant","data":data
        }))
        .expect("event")
    }

    #[derive(Clone, Default)]
    struct Captured {
        upload_base: Arc<Mutex<String>>,
        allocation: Arc<Mutex<HashMap<String, String>>>,
        raw: Arc<Mutex<Vec<u8>>>,
        completed: Arc<Mutex<Value>>,
        complete_calls: Arc<Mutex<usize>>,
    }

    async fn allocate(
        State(state): State<Captured>,
        Form(form): Form<HashMap<String, String>>,
    ) -> axum::Json<Value> {
        *state.allocation.lock().expect("allocation") = form;
        axum::Json(json!({
            "ok":true,
            "upload_url":format!("{}/upload", state.upload_base.lock().expect("base")),
            "file_id":"F123"
        }))
    }

    async fn upload(State(state): State<Captured>, body: Bytes) -> &'static str {
        *state.raw.lock().expect("raw") = body.to_vec();
        "ok"
    }

    async fn complete(
        State(state): State<Captured>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::Json<Value> {
        *state.complete_calls.lock().expect("complete calls") += 1;
        *state.completed.lock().expect("completed") = body;
        axum::Json(json!({"ok":true}))
    }

    #[tokio::test]
    async fn uploads_blob_and_completes_it_in_the_target_channel() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let captured = Captured::default();
        *captured.upload_base.lock().expect("base") = base.clone();
        let app = Router::new()
            .route("/files.getUploadURLExternal", post(allocate))
            .route("/files.completeUploadExternal", post(complete))
            .route("/upload", post(upload))
            .with_state(captured.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let group = store.create("slack", "").expect("group");
        let blob = cccc_core::blobs::store(&home, &group.group_id, b"png-bytes").expect("blob");
        let mut sender = SlackOutbound::new(
            home,
            &group.group_id,
            reqwest::Client::new(),
            "token".into(),
        );
        sender.api_base = base;
        let attachment = json!({
            "path":blob.path,
            "title":"logo.png",
            "mime_type":"image/png"
        });
        let prepared = sender.prepare(&attachment).await.expect("prepare");
        let target = AuthorizedChat {
            chat_id: "D123".into(),
            thread_id: String::new(),
            verbose: false,
        };
        let prepared_again = sender.prepare(&attachment).await.expect("prepare again");
        sender
            .upload_many(&target, "caption", &[prepared, prepared_again])
            .await
            .expect("upload");

        assert_eq!(*captured.raw.lock().expect("raw"), b"png-bytes");
        assert_eq!(
            *captured.allocation.lock().expect("allocation"),
            HashMap::from([
                ("filename".to_owned(), "logo.png".to_owned()),
                ("length".to_owned(), "9".to_owned()),
            ])
        );
        let completed = captured.completed.lock().expect("completed");
        assert_eq!(completed["channel_id"], "D123");
        assert_eq!(completed["initial_comment"], "caption");
        assert_eq!(completed["files"][0]["id"], "F123");
        assert_eq!(completed["files"][0]["title"], "logo.png");
        assert_eq!(completed["files"].as_array().expect("files").len(), 2);
        assert_eq!(*captured.complete_calls.lock().expect("complete calls"), 1);
        server.abort();
    }

    #[test]
    fn rejects_unsafe_attachment_titles() {
        assert_eq!(safe_filename("report.md"), Some("report.md"));
        assert_eq!(safe_filename("../report.md"), None);
        assert_eq!(safe_filename("folder/report.md"), None);
    }

    #[tokio::test]
    async fn stream_completion_suppresses_only_a_complete_single_message() {
        #[derive(Clone, Default)]
        struct Messages(Arc<Mutex<Vec<(String, Value)>>>);

        async fn capture(
            State(messages): State<Messages>,
            uri: axum::http::Uri,
            axum::Json(body): axum::Json<Value>,
        ) -> axum::Json<Value> {
            messages
                .0
                .lock()
                .expect("messages")
                .push((uri.path().to_owned(), body));
            axum::Json(json!({"ok":true,"ts":"1710000000.100"}))
        }

        let messages = Messages::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let app = Router::new()
            .route("/chat.postMessage", post(capture))
            .route("/chat.update", post(capture))
            .with_state(messages.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("slack", "")
            .expect("group");
        let mut outbound = SlackOutbound::new(
            home,
            &group.group_id,
            reqwest::Client::new(),
            "token".into(),
        );
        outbound.api_base = base;
        let target = AuthorizedChat {
            chat_id: "C123".into(),
            thread_id: "1700000000.500".into(),
            verbose: false,
        };

        outbound
            .send(
                &[target.clone()],
                &event(
                    "chat.stream",
                    json!({"op":"start","stream_id":"short","text":"hel"}),
                ),
            )
            .await;
        outbound
            .send(
                &[target.clone()],
                &event(
                    "chat.stream",
                    json!({"op":"end","stream_id":"short","text":"hello"}),
                ),
            )
            .await;
        let before_final = messages.0.lock().expect("messages").len();
        assert!(
            messages
                .0
                .lock()
                .expect("messages")
                .iter()
                .any(|(path, body)| {
                    path == "/chat.update" && body["text"] == "assistant\n\nhello"
                })
        );
        outbound
            .send(
                &[target.clone()],
                &event(
                    "chat.message",
                    json!({"stream_id":"short","text":"hello","to":["user"]}),
                ),
            )
            .await;
        assert_eq!(messages.0.lock().expect("messages").len(), before_final);

        let long = "你".repeat(MAX_MESSAGE_CHARS + 1);
        outbound
            .send(
                &[target.clone()],
                &event(
                    "chat.stream",
                    json!({"op":"start","stream_id":"long","text":"start"}),
                ),
            )
            .await;
        outbound
            .send(
                &[target.clone()],
                &event(
                    "chat.stream",
                    json!({"op":"end","stream_id":"long","text":long}),
                ),
            )
            .await;
        let before_long_final = messages.0.lock().expect("messages").len();
        outbound
            .send(
                &[target],
                &event(
                    "chat.message",
                    json!({"stream_id":"long","text":long,"to":["user"]}),
                ),
            )
            .await;
        let calls = messages.0.lock().expect("messages");
        assert!(calls.len() >= before_long_final + 2);
        assert!(
            calls
                .iter()
                .filter(|(path, _)| path == "/chat.postMessage")
                .all(|(_, body)| body["thread_ts"] == "1700000000.500")
        );
        server.abort();
    }
}
