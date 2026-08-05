use super::inbound_attachments::MAX_ATTACHMENT_BYTES;
use super::outbound_text;
use async_trait::async_trait;
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use lark_channel::lark_openapi::ReqwestOpenApiTransport;
use lark_channel::{MessageContent, MessageSender, Recipient};
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};
use std::path::Path;

pub(super) struct FeishuOutbound {
    home: HomeLayout,
    group_id: String,
    http: reqwest::Client,
    api_base: String,
    sender: MessageSender<ReqwestOpenApiTransport>,
}

struct PreparedAttachment {
    raw: Vec<u8>,
    title: String,
    mime: String,
    is_image: bool,
}

struct UploadedAttachment {
    title: String,
    key: String,
    is_image: bool,
}

impl FeishuOutbound {
    pub(super) fn new(
        home: HomeLayout,
        group_id: &str,
        http: reqwest::Client,
        api_base: String,
        sender: MessageSender<ReqwestOpenApiTransport>,
    ) -> Self {
        Self {
            home,
            group_id: group_id.into(),
            http,
            api_base,
            sender,
        }
    }

    pub(super) async fn send(&self, targets: &[String], event: &Event) {
        self.send_with(&self.sender, targets, event).await;
    }

    async fn send_with<S: FeishuSender + ?Sized>(
        &self,
        sender: &S,
        targets: &[String],
        event: &Event,
    ) {
        let body = outbound_text(event, false);
        let attachments = event
            .data
            .get("attachments")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if body.is_none() && attachments.is_empty() {
            return;
        }

        if let Some(body) = body.as_deref() {
            for chat_id in targets {
                if let Err(error) = sender.send_text(chat_id, body).await {
                    tracing::warn!(%error, %chat_id, "failed to send Feishu IM message");
                }
            }
        }

        for attachment in attachments {
            let prepared = match self.prepare(attachment).await {
                Ok(prepared) => prepared,
                Err(error) => {
                    tracing::warn!(%error, "failed to prepare Feishu attachment");
                    continue;
                }
            };
            let uploaded = match self.upload(sender, prepared).await {
                Ok(uploaded) => uploaded,
                Err(error) => {
                    tracing::warn!(%error, "failed to upload Feishu attachment");
                    continue;
                }
            };
            for chat_id in targets {
                if let Err(error) = sender.send_attachment(chat_id, &uploaded).await {
                    tracing::warn!(
                        %error,
                        %chat_id,
                        file = %uploaded.title,
                        "failed to send Feishu attachment"
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
        let size = path.metadata().map_err(|error| error.to_string())?.len();
        if size > MAX_ATTACHMENT_BYTES {
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
            .or_else(|| mime_guess::from_path(&title).first_raw())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let is_image = mime.starts_with("image/");
        Ok(PreparedAttachment {
            raw,
            title,
            mime,
            is_image,
        })
    }

    async fn upload<S: FeishuSender + ?Sized>(
        &self,
        sender: &S,
        attachment: PreparedAttachment,
    ) -> Result<UploadedAttachment, String> {
        let token = sender.tenant_token().await?;
        let endpoint = if attachment.is_image {
            "/open-apis/im/v1/images"
        } else {
            "/open-apis/im/v1/files"
        };
        let file = Part::bytes(attachment.raw)
            .file_name(attachment.title.clone())
            .mime_str(&attachment.mime)
            .map_err(|error| error.to_string())?;
        let form = if attachment.is_image {
            Form::new()
                .text("image_type", "message")
                .part("image", file)
        } else {
            Form::new()
                .text("file_type", "stream")
                .text("file_name", attachment.title.clone())
                .part("file", file)
        };
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.api_base.trim_end_matches('/'),
                endpoint
            ))
            .bearer_auth(token)
            .multipart(form)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let value: Value = response.json().await.map_err(|error| error.to_string())?;
        if !status.is_success() || value.get("code").and_then(Value::as_i64) != Some(0) {
            let message = value
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("Feishu upload API request failed");
            return Err(format!("HTTP {status}: {message}"));
        }
        let key_name = if attachment.is_image {
            "image_key"
        } else {
            "file_key"
        };
        let key = value
            .pointer(&format!("/data/{key_name}"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("Feishu upload response has no {key_name}"))?;
        Ok(UploadedAttachment {
            title: attachment.title,
            key: key.to_owned(),
            is_image: attachment.is_image,
        })
    }
}

fn safe_filename(value: &str) -> Option<&str> {
    let value = value.trim();
    let path = Path::new(value);
    (!value.is_empty()
        && value != "."
        && value != ".."
        && path.file_name().and_then(|name| name.to_str()) == Some(value))
    .then_some(value)
}

fn attachment_content(attachment: &UploadedAttachment) -> MessageContent {
    let (msg_type, content) = if attachment.is_image {
        ("image", json!({"image_key":attachment.key}))
    } else {
        ("file", json!({"file_key":attachment.key}))
    };
    MessageContent::Custom {
        msg_type: msg_type.into(),
        content,
    }
}

#[async_trait]
trait FeishuSender: Send + Sync {
    async fn tenant_token(&self) -> Result<String, String>;
    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String>;
    async fn send_attachment(
        &self,
        chat_id: &str,
        attachment: &UploadedAttachment,
    ) -> Result<(), String>;
}

#[async_trait]
impl FeishuSender for MessageSender<ReqwestOpenApiTransport> {
    async fn tenant_token(&self) -> Result<String, String> {
        self.client()
            .tenant_access_token()
            .await
            .map_err(|error| error.to_string())
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String> {
        self.text_message(Recipient::Chat(chat_id.into()), text)
            .send()
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn send_attachment(
        &self,
        chat_id: &str,
        attachment: &UploadedAttachment,
    ) -> Result<(), String> {
        self.message(
            Recipient::Chat(chat_id.into()),
            attachment_content(attachment),
        )
        .send()
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, Uri},
        routing::post,
    };
    use cccc_core::GroupStore;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeSender {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl FeishuSender for FakeSender {
        async fn tenant_token(&self) -> Result<String, String> {
            Ok("tenant-token".into())
        }

        async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String> {
            self.calls
                .lock()
                .expect("calls")
                .push(format!("text:{chat_id}:{text}"));
            Ok(())
        }

        async fn send_attachment(
            &self,
            chat_id: &str,
            attachment: &UploadedAttachment,
        ) -> Result<(), String> {
            self.calls.lock().expect("calls").push(format!(
                "attachment:{chat_id}:{}:{}:{}",
                attachment.title, attachment.key, attachment.is_image
            ));
            Ok(())
        }
    }

    fn setup(
        http: reqwest::Client,
        api_base: String,
    ) -> (tempfile::TempDir, FeishuOutbound, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("feishu", "")
            .expect("group");
        let blob = cccc_core::blobs::store(&home, &group.group_id, b"png-bytes").expect("blob");
        let config = lark_channel::ChannelConfig::new("app", "secret");
        let openapi =
            lark_channel::lark_openapi::OpenApiClient::new(config, ReqwestOpenApiTransport::new());
        (
            temp,
            FeishuOutbound::new(
                home,
                &group.group_id,
                http,
                api_base,
                MessageSender::new(openapi),
            ),
            blob.path,
        )
    }

    #[test]
    fn builds_image_and_file_message_content() {
        let image = attachment_content(&UploadedAttachment {
            title: "photo.png".into(),
            key: "img_1".into(),
            is_image: true,
        });
        let file = attachment_content(&UploadedAttachment {
            title: "report.pdf".into(),
            key: "file_1".into(),
            is_image: false,
        });
        assert_eq!(
            image,
            MessageContent::Custom {
                msg_type: "image".into(),
                content: json!({"image_key":"img_1"})
            }
        );
        assert_eq!(
            file,
            MessageContent::Custom {
                msg_type: "file".into(),
                content: json!({"file_key":"file_1"})
            }
        );
    }

    #[tokio::test]
    async fn uploads_image_and_file_and_sends_them_to_each_target() {
        type UploadCapture = Vec<(String, String, String, Vec<u8>)>;

        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<UploadCapture>>);

        async fn upload(
            State(capture): State<Capture>,
            uri: Uri,
            headers: HeaderMap,
            body: Bytes,
        ) -> axum::Json<Value> {
            capture.0.lock().expect("capture").push((
                uri.path().into(),
                headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .into(),
                headers
                    .get("content-type")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .into(),
                body.to_vec(),
            ));
            let data = if uri.path().ends_with("/images") {
                json!({"image_key":"img_1"})
            } else {
                json!({"file_key":"file_1"})
            };
            axum::Json(json!({"code":0,"msg":"ok","data":data}))
        }

        let capture = Capture::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let app = Router::new()
            .route("/open-apis/im/v1/images", post(upload))
            .route("/open-apis/im/v1/files", post(upload))
            .with_state(capture.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        let (_temp, outbound, path) = setup(reqwest::Client::new(), base);
        let sender = FakeSender::default();
        let event: Event = serde_json::from_value(json!({
            "v":1,"id":"event","ts":"now","kind":"chat.message",
            "group_id":"group","scope_key":"","by":"assistant",
            "data":{"text":"result","attachments":[
                {"path":path,"title":"photo.png","kind":"file","mime_type":"image/png"},
                {"path":path,"title":"report.pdf","kind":"file","mime_type":"application/pdf"}
            ]}
        }))
        .expect("event");

        outbound
            .send_with(&sender, &["chat-1".into(), "chat-2".into()], &event)
            .await;

        let uploads = capture.0.lock().expect("capture");
        assert_eq!(uploads.len(), 2);
        assert_eq!(uploads[0].0, "/open-apis/im/v1/images");
        assert_eq!(uploads[0].1, "Bearer tenant-token");
        assert!(uploads[0].2.starts_with("multipart/form-data; boundary="));
        assert!(String::from_utf8_lossy(&uploads[0].3).contains("name=\"image_type\""));
        assert!(String::from_utf8_lossy(&uploads[0].3).contains("png-bytes"));
        assert_eq!(uploads[1].0, "/open-apis/im/v1/files");
        assert_eq!(uploads[1].1, "Bearer tenant-token");
        assert!(uploads[1].2.starts_with("multipart/form-data; boundary="));
        let file_body = String::from_utf8_lossy(&uploads[1].3);
        assert!(file_body.contains("name=\"file_type\""));
        assert!(file_body.contains("stream"));
        assert!(file_body.contains("name=\"file_name\""));
        assert!(file_body.contains("report.pdf"));
        assert!(file_body.contains("png-bytes"));
        assert_eq!(
            *sender.calls.lock().expect("calls"),
            vec![
                "text:chat-1:assistant\n\nresult",
                "text:chat-2:assistant\n\nresult",
                "attachment:chat-1:photo.png:img_1:true",
                "attachment:chat-2:photo.png:img_1:true",
                "attachment:chat-1:report.pdf:file_1:false",
                "attachment:chat-2:report.pdf:file_1:false",
            ]
        );
        server.abort();
    }
}
