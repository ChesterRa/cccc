use super::inbound_attachments::MAX_ATTACHMENT_BYTES;
use super::outbound_text;
use cccc_contracts::Event;
use cccc_core::HomeLayout;
use serde_json::Value;
use serenity::all::{ChannelId, CreateAttachment, CreateMessage};
use serenity::http::Http;
use std::path::Path;
use std::sync::Arc;

const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;

pub(super) struct DiscordOutbound {
    home: HomeLayout,
    group_id: String,
    http: Arc<Http>,
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
        }
    }

    pub(super) async fn send_target(
        &self,
        channel_id: ChannelId,
        event: &Event,
    ) -> Result<(), String> {
        let body = outbound_text(event, true).unwrap_or_default();
        let values = event
            .data
            .get("attachments")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if values.is_empty() {
            if !body.is_empty() {
                channel_id
                    .say(self.http.as_ref(), body)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            return Ok(());
        }

        let mut body_pending = !body.is_empty();
        let mut sent = false;
        for values in values.chunks(MAX_ATTACHMENTS_PER_MESSAGE) {
            let attachments = self.prepare_available(values).await;
            if attachments.is_empty() {
                continue;
            }
            let files = attachments
                .iter()
                .map(|item| CreateAttachment::bytes(item.raw.clone(), item.title.clone()));
            let message = if body_pending {
                CreateMessage::new().content(body.clone())
            } else {
                CreateMessage::new()
            };
            channel_id
                .send_files(self.http.as_ref(), files, message)
                .await
                .map_err(|error| error.to_string())?;
            body_pending = false;
            sent = true;
        }
        if body_pending {
            channel_id
                .say(self.http.as_ref(), body)
                .await
                .map_err(|error| error.to_string())?;
            sent = true;
        }
        if !sent {
            return Err("Discord event has no valid text or attachments".into());
        }
        Ok(())
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

fn safe_filename(value: &str) -> Option<&str> {
    let value = value.trim();
    let path = Path::new(value);
    (!value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\'])
        && !value.chars().any(char::is_control)
        && path.file_name().and_then(|name| name.to_str()) == Some(value))
    .then_some(value)
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
