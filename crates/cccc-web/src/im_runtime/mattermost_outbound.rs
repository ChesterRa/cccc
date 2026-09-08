use super::inbound_attachments::MAX_ATTACHMENT_BYTES;
use super::mattermost::{MattermostApi, field, valid_id};
use super::outbound_attachment::safe_filename;
use super::outbound_chunks::{fits_message, split_message};
use super::outbound_stream_state::trim_active;
use super::{AuthorizedChat, outbound_text};
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use reqwest::Method;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_MESSAGE_CHARS: usize = 16_383;
const MAX_FILES_PER_POST: usize = 5;
const STREAM_THROTTLE: Duration = Duration::from_millis(500);

pub(super) struct MattermostOutbound {
    home: HomeLayout,
    group_id: String,
    api: MattermostApi,
    streams: Mutex<HashMap<(String, String), MattermostStream>>,
    completed: Mutex<HashMap<(String, String), String>>,
    files_enabled: bool,
    max_file_bytes: u64,
}

#[derive(Clone)]
struct MattermostStream {
    post_id: String,
    last_update: Option<Instant>,
}

impl MattermostOutbound {
    pub(super) fn new(
        home: HomeLayout,
        group_id: &str,
        api: MattermostApi,
        config: &Map<String, Value>,
    ) -> Self {
        let files = config.get("files");
        Self {
            home,
            group_id: group_id.to_owned(),
            api,
            streams: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashMap::new()),
            files_enabled: files
                .and_then(|v| v.get("enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
            max_file_bytes: files
                .and_then(|v| v.get("max_mb"))
                .and_then(Value::as_u64)
                .unwrap_or(10)
                .saturating_mul(1024 * 1024)
                .min(MAX_ATTACHMENT_BYTES),
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
        let body = outbound_text(event, true).unwrap_or_default();
        let stream_id = event
            .data
            .get("stream_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let streamed = !stream_id.is_empty()
            && self
                .completed
                .lock()
                .expect("Mattermost streams poisoned")
                .remove(&(stream_id.to_owned(), target.key()))
                .is_some_and(|sent| sent == body);
        let chunks = if streamed {
            Vec::new()
        } else {
            split_message(&body, MAX_MESSAGE_CHARS, None)
        };
        let mut ids = Vec::new();
        let mut failed = false;
        if let Some(attachments) = event.data.get("attachments").and_then(Value::as_array) {
            for attachment in attachments {
                match self.upload(&target.chat_id, attachment).await {
                    Ok(id) => ids.push(id),
                    Err(error) => {
                        failed = true;
                        self.api
                            .log_error(&self.home, &self.group_id, "upload", &error);
                    }
                }
            }
        }
        let mut chunk_index = 0;
        for files in ids.chunks(MAX_FILES_PER_POST) {
            let text = chunks.get(chunk_index).map_or("", String::as_str);
            self.api
                .post(&target.chat_id, &target.thread_id, text, files)
                .await?;
            if chunk_index < chunks.len() {
                chunk_index += 1;
            }
        }
        for chunk in &chunks[chunk_index..] {
            self.api
                .post(&target.chat_id, &target.thread_id, chunk, &[])
                .await?;
        }
        if failed {
            self.api
                .post(
                    &target.chat_id,
                    &target.thread_id,
                    "部分附件发送失败，请在 CCCC Web 中查看原附件及连接器错误。",
                    &[],
                )
                .await?;
            return Err("Some Mattermost attachments could not be sent".into());
        }
        Ok(())
    }

    async fn upload(&self, chat_id: &str, value: &Value) -> Result<String, String> {
        if !self.files_enabled {
            return Err("Mattermost file forwarding is disabled".into());
        }
        if !valid_id(chat_id) {
            return Err("Invalid Mattermost channel id".into());
        }
        let relative = value
            .get("path")
            .and_then(Value::as_str)
            .ok_or("Attachment path is missing")?;
        let path = cccc_core::blobs::resolve(&self.home, &self.group_id, relative)
            .map_err(|e| e.to_string())?;
        if path.metadata().map_err(|e| e.to_string())?.len() > self.max_file_bytes {
            return Err("Attachment exceeds configured size limit".into());
        }
        let raw = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
        if raw.len() as u64 > self.max_file_bytes {
            return Err("Attachment exceeds configured size limit".into());
        }
        let title = value
            .get("title")
            .and_then(Value::as_str)
            .and_then(safe_filename)
            .or_else(|| {
                path.file_name()
                    .and_then(|v| v.to_str())
                    .and_then(safe_filename)
            })
            .unwrap_or("file");
        // MM 4.8+ 原生单文件 body 上传，与 multipart 等价且可保留原文件名。
        let request = self
            .api
            .request(Method::POST, "files")
            .query(&[("channel_id", chat_id), ("filename", title)])
            .header("content-type", "application/octet-stream")
            .body(raw);
        let result: Value = self
            .api
            .response(request)
            .await?
            .json()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let id = result
            .pointer("/file_infos/0/id")
            .and_then(Value::as_str)
            .filter(|id| valid_id(id))
            .ok_or("Mattermost upload response has no file id")?;
        Ok(id.to_owned())
    }

    async fn send_stream(&self, target: &AuthorizedChat, event: &Event) -> Result<(), String> {
        let data = Value::Object(event.data.clone());
        let op = field(&data, "op");
        let stream_id = field(&data, "stream_id");
        if stream_id.is_empty() || !matches!(op, "start" | "update" | "end") {
            return Ok(());
        }
        let key = (stream_id.to_owned(), target.key());
        let raw = field(&data, "text");
        let body = outbound_text(event, true).unwrap_or_default();
        let preview = split_message(
            &if raw.is_empty() {
                format!("{body}…")
            } else {
                body.clone()
            },
            MAX_MESSAGE_CHARS,
            None,
        )
        .into_iter()
        .next()
        .unwrap_or_else(|| "…".into());
        if op == "start" {
            if self
                .streams
                .lock()
                .expect("Mattermost streams poisoned")
                .contains_key(&key)
            {
                return Ok(());
            }
            let post_id = self
                .api
                .post(&target.chat_id, &target.thread_id, &preview, &[])
                .await?;
            let mut streams = self.streams.lock().expect("Mattermost streams poisoned");
            streams.insert(
                key,
                MattermostStream {
                    post_id,
                    last_update: None,
                },
            );
            trim_active(&mut streams);
            return Ok(());
        }
        let stream = {
            let mut streams = self.streams.lock().expect("Mattermost streams poisoned");
            if op == "end" {
                streams.remove(&key)
            } else {
                streams
                    .get(&key)
                    .cloned()
                    .filter(|s| s.last_update.is_none_or(|t| t.elapsed() >= STREAM_THROTTLE))
            }
        };
        let Some(stream) = stream else {
            return Ok(());
        };
        self.api.edit(&stream.post_id, &preview).await?;
        if op == "end" {
            if !raw.is_empty() && fits_message(&body, MAX_MESSAGE_CHARS, None) {
                let mut completed = self.completed.lock().expect("Mattermost streams poisoned");
                completed.insert(key, body);
                trim_active(&mut completed);
            }
        } else if let Some(stream) = self
            .streams
            .lock()
            .expect("Mattermost streams poisoned")
            .get_mut(&key)
        {
            stream.last_update = Some(Instant::now());
        }
        Ok(())
    }
}
