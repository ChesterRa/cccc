use super::inbound_attachments::MAX_ATTACHMENT_BYTES;
use super::outbound_attachment::safe_filename;
use super::outbound_chunks::split_message;
use super::outbound_text;
use async_trait::async_trait;
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use serde_json::Value;
use std::path::{Path, PathBuf};
use weixin_agent::WeixinClient;

pub(super) struct WeixinOutbound {
    home: HomeLayout,
    group_id: String,
    sdk: Option<std::sync::Arc<WeixinClient>>,
}

impl WeixinOutbound {
    pub(super) fn new(home: HomeLayout, group_id: &str, sdk: std::sync::Arc<WeixinClient>) -> Self {
        Self {
            home,
            group_id: group_id.into(),
            sdk: Some(sdk),
        }
    }

    pub(super) async fn send(&self, targets: &[String], event: &Event) {
        let Some(sdk) = self.sdk.as_deref() else {
            return;
        };
        self.send_with(sdk, targets, event).await;
    }

    async fn send_with<S: WeixinSender + ?Sized>(
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

        let mut prepared = Vec::new();
        for attachment in attachments {
            match self.prepare(attachment).await {
                Ok(item) => prepared.push(item),
                Err(error) => tracing::warn!(%error, "failed to prepare Weixin attachment"),
            }
        }
        for user_id in targets {
            let context_token = sender.context_token(user_id);
            if let Some(body) = body.as_deref() {
                for chunk in split_message(body, 4_000, None) {
                    if let Err(error) = sender
                        .send_text(user_id, &chunk, context_token.as_deref())
                        .await
                    {
                        tracing::warn!(%error, %user_id, "failed to send Weixin IM message");
                    }
                }
            }
            for attachment in &prepared {
                if let Err(error) = sender
                    .send_media(user_id, &attachment.path, context_token.as_deref())
                    .await
                {
                    tracing::warn!(
                        %error,
                        %user_id,
                        file = %attachment.title,
                        "failed to send Weixin attachment"
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
        let source = cccc_core::blobs::resolve(&self.home, &self.group_id, relative)
            .map_err(|error| error.to_string())?;
        let size = source.metadata().map_err(|error| error.to_string())?.len();
        if size > MAX_ATTACHMENT_BYTES {
            return Err("attachment exceeds 10 MiB".into());
        }
        let title = value
            .get("title")
            .and_then(Value::as_str)
            .and_then(safe_filename)
            .unwrap_or("file")
            .to_owned();
        let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
        let path = temp.path().join(&title);
        tokio::fs::copy(source, &path)
            .await
            .map_err(|error| error.to_string())?;
        Ok(PreparedAttachment {
            _temp: temp,
            path,
            title,
        })
    }
}

struct PreparedAttachment {
    _temp: tempfile::TempDir,
    path: PathBuf,
    title: String,
}

#[async_trait]
trait WeixinSender: Send + Sync {
    fn context_token(&self, user_id: &str) -> Option<String>;

    async fn send_text(
        &self,
        user_id: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<(), String>;

    async fn send_media(
        &self,
        user_id: &str,
        path: &Path,
        context_token: Option<&str>,
    ) -> Result<(), String>;
}

#[async_trait]
impl WeixinSender for WeixinClient {
    fn context_token(&self, user_id: &str) -> Option<String> {
        self.context_tokens().get(user_id)
    }

    async fn send_text(
        &self,
        user_id: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<(), String> {
        WeixinClient::send_text(self, user_id, text, context_token)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn send_media(
        &self,
        user_id: &str,
        path: &Path,
        context_token: Option<&str>,
    ) -> Result<(), String> {
        WeixinClient::send_media(self, user_id, path, context_token)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeSender {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl WeixinSender for FakeSender {
        fn context_token(&self, user_id: &str) -> Option<String> {
            Some(format!("token:{user_id}"))
        }

        async fn send_text(
            &self,
            user_id: &str,
            text: &str,
            context_token: Option<&str>,
        ) -> Result<(), String> {
            self.calls.lock().expect("calls").push(format!(
                "text:{user_id}:{text}:{}",
                context_token.unwrap_or_default()
            ));
            Ok(())
        }

        async fn send_media(
            &self,
            user_id: &str,
            path: &Path,
            context_token: Option<&str>,
        ) -> Result<(), String> {
            assert!(path.exists());
            self.calls.lock().expect("calls").push(format!(
                "media:{user_id}:{}:{}",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(""),
                context_token.unwrap_or_default()
            ));
            Ok(())
        }
    }

    fn setup() -> (tempfile::TempDir, WeixinOutbound, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("weixin", "")
            .expect("group");
        let attachment = super::super::inbound_attachments::store_bytes(
            &home,
            &group.group_id,
            b"image",
            super::super::inbound_attachments::AttachmentSpec::new(
                "image",
                "source.png",
                "image/png",
            ),
        )
        .expect("blob");
        (
            temp,
            WeixinOutbound {
                home,
                group_id: group.group_id,
                sdk: None,
            },
            attachment["path"].as_str().expect("path").to_owned(),
        )
    }

    #[tokio::test]
    async fn sends_sender_title_then_attachment_with_original_filename() {
        let (_temp, outbound, path) = setup();
        let sender = FakeSender::default();
        let event: Event = serde_json::from_value(serde_json::json!({
            "v":1,"id":"event","ts":"now","kind":"chat.message",
            "group_id":"group","scope_key":"","by":"assistant",
            "data":{"text":"result","sender_title":"Helpful Assistant","attachments":[{
                "path":path,"title":"photo.png","mime_type":"image/png","bytes":5
            }]}
        }))
        .expect("event");

        outbound
            .send_with(&sender, &["wx-user".into()], &event)
            .await;

        assert_eq!(
            *sender.calls.lock().expect("calls"),
            vec![
                "text:wx-user:Helpful Assistant\n\nresult:token:wx-user",
                "media:wx-user:photo.png:token:wx-user"
            ]
        );
    }

    #[tokio::test]
    async fn rejects_attachment_title_with_path_components() {
        let (_temp, outbound, path) = setup();
        let prepared = outbound
            .prepare(&serde_json::json!({"path":path,"title":"../photo.png"}))
            .await
            .expect("prepared");

        assert_eq!(prepared.title, "file");
        assert_eq!(
            prepared.path.file_name().and_then(|name| name.to_str()),
            Some("file")
        );
    }

    #[tokio::test]
    async fn long_unicode_reply_is_sent_in_lossless_chunks() {
        let (_temp, outbound, _path) = setup();
        let sender = FakeSender::default();
        let text = "你".repeat(5_000);
        let event: Event = serde_json::from_value(serde_json::json!({
            "v":1,"id":"event","ts":"now","kind":"chat.message",
            "group_id":"group","scope_key":"","by":"assistant",
            "data":{"text":text,"sender_title":"Helpful Assistant","to":["user"]}
        }))
        .expect("event");

        outbound
            .send_with(&sender, &["wx-user".into()], &event)
            .await;

        let calls = sender.calls.lock().expect("calls");
        let chunks = calls
            .iter()
            .map(|call| {
                call.strip_prefix("text:wx-user:")
                    .and_then(|call| call.strip_suffix(":token:wx-user"))
                    .expect("text call")
            })
            .collect::<Vec<_>>();
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 4_000));
        assert_eq!(chunks.concat(), format!("Helpful Assistant\n\n{text}"));
    }
}
