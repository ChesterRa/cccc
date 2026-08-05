use super::outbound_text;
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use serde_json::{Value, json};

const API: &str = "https://slack.com/api";
const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;

pub(super) struct SlackOutbound {
    home: HomeLayout,
    group_id: String,
    http: reqwest::Client,
    bot_token: String,
    api_base: String,
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
        }
    }

    pub(super) async fn send(&self, targets: &[String], event: &Event) {
        let body = outbound_text(event, false).unwrap_or_default();
        let attachments = event
            .data
            .get("attachments")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if attachments.is_empty() {
            if body.is_empty() {
                return;
            }
            for channel in targets {
                if let Err(error) = self.post_message(channel, &body).await {
                    tracing::warn!(%error, %channel, "failed to send Slack IM message");
                }
            }
            return;
        }

        for attachment in attachments {
            let prepared = match self.prepare(attachment).await {
                Ok(prepared) => prepared,
                Err(error) => {
                    tracing::warn!(%error, "failed to prepare Slack attachment");
                    continue;
                }
            };
            for channel in targets {
                if let Err(error) = self.upload(channel, &body, &prepared).await {
                    tracing::warn!(
                        %error,
                        %channel,
                        file = %prepared.title,
                        "failed to send Slack attachment"
                    );
                }
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

    async fn upload(
        &self,
        channel: &str,
        comment: &str,
        attachment: &PreparedAttachment,
    ) -> Result<(), String> {
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
        self.api_call(
            "files.completeUploadExternal",
            json!({
                "files":[{"id":file_id,"title":attachment.title}],
                "channel_id":channel,
                "initial_comment":comment
            }),
        )
        .await?;
        Ok(())
    }

    async fn post_message(&self, channel: &str, text: &str) -> Result<(), String> {
        self.api_call("chat.postMessage", json!({"channel":channel,"text":text}))
            .await?;
        Ok(())
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

fn safe_filename(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\'))
    .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Form, Router, body::Bytes, extract::State, routing::post};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Captured {
        upload_base: Arc<Mutex<String>>,
        allocation: Arc<Mutex<HashMap<String, String>>>,
        raw: Arc<Mutex<Vec<u8>>>,
        completed: Arc<Mutex<Value>>,
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
        sender
            .upload("D123", "caption", &prepared)
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
        server.abort();
    }

    #[test]
    fn rejects_unsafe_attachment_titles() {
        assert_eq!(safe_filename("report.md"), Some("report.md"));
        assert_eq!(safe_filename("../report.md"), None);
        assert_eq!(safe_filename("folder/report.md"), None);
    }
}
