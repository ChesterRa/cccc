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
                        self.report_failure(
                            "send_text",
                            user_id,
                            context_token.is_some(),
                            None,
                            &error,
                        );
                    }
                }
            }
            for attachment in &prepared {
                if let Err(error) = sender
                    .send_media(user_id, &attachment.path, context_token.as_deref())
                    .await
                {
                    self.report_failure(
                        "send_media",
                        user_id,
                        context_token.is_some(),
                        Some(&attachment.title),
                        &error,
                    );
                }
            }
        }
    }

    /// Delivery failures are otherwise invisible outside the daemon's terminal, so record them
    /// in the group's IM log as well.
    fn report_failure(
        &self,
        operation: &str,
        user_id: &str,
        has_context_token: bool,
        file: Option<&str>,
        error: &str,
    ) {
        tracing::warn!(%error, %user_id, operation, "failed to send Weixin IM message");
        let line = serde_json::json!({
            "ts": cccc_contracts::utc_now(), "level": "WARN", "platform": "weixin",
            "group_id": self.group_id, "operation": operation, "user_id": user_id,
            "has_context_token": has_context_token, "file": file,
            "error": error.chars().take(4096).collect::<String>(),
        })
        .to_string();
        if let Err(error) = super::bridge_log::append(&self.home, &self.group_id, &line) {
            tracing::warn!(%error, "failed to write the Weixin IM log");
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
#[path = "weixin_outbound_tests.rs"]
mod tests;
