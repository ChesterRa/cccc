use super::inbound_attachments::{AttachmentSpec, download_response};
use cccc_core::HomeLayout;
use dingtalk_stream::{ChatbotMessage, DingTalkStreamClient};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct DingTalkAttachmentDownloader {
    token_client: Arc<DingTalkStreamClient>,
    http: reqwest::Client,
    robot_code: String,
    openapi_endpoint: String,
}

impl DingTalkAttachmentDownloader {
    pub(super) fn new(token_client: Arc<DingTalkStreamClient>, robot_code: String) -> Self {
        Self {
            token_client,
            http: reqwest::Client::new(),
            robot_code,
            openapi_endpoint: std::env::var("DINGTALK_OPENAPI_ENDPOINT")
                .unwrap_or_else(|_| "https://api.dingtalk.com".into()),
        }
    }

    pub(super) async fn materialize(
        &self,
        home: &HomeLayout,
        group_id: &str,
        message: &ChatbotMessage,
    ) -> Vec<Value> {
        let mut result = Vec::new();
        for attachment in remote_attachments(message) {
            match self.materialize_one(home, group_id, attachment).await {
                Ok(value) => result.push(value),
                Err(error) => tracing::warn!(%error, "failed to download DingTalk attachment"),
            }
        }
        result
    }

    async fn materialize_one(
        &self,
        home: &HomeLayout,
        group_id: &str,
        attachment: RemoteAttachment,
    ) -> Result<Value, String> {
        let token = self
            .token_client
            .get_access_token()
            .await
            .map_err(|error| error.to_string())?;
        let response = self
            .http
            .post(format!(
                "{}/v1.0/robot/messageFiles/download",
                self.openapi_endpoint.trim_end_matches('/')
            ))
            .header("x-acs-dingtalk-access-token", token)
            .json(&json!({
                "robotCode": self.robot_code,
                "downloadCode": attachment.download_code,
            }))
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        let payload: Value = response.json().await.map_err(|error| error.to_string())?;
        let download_url = payload
            .get("downloadUrl")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "DingTalk download response has no downloadUrl".to_owned())?;
        let response = self
            .http
            .get(download_url)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        download_response(home, group_id, response, None, attachment.spec).await
    }
}

pub(super) fn inbound_text(message: &ChatbotMessage) -> String {
    message
        .get_text_list()
        .unwrap_or_default()
        .into_iter()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn has_attachments(message: &ChatbotMessage) -> bool {
    !message.get_all_download_codes().is_empty()
}

struct RemoteAttachment {
    download_code: String,
    spec: AttachmentSpec,
}

fn remote_attachments(message: &ChatbotMessage) -> Vec<RemoteAttachment> {
    message
        .get_all_download_codes()
        .into_iter()
        .enumerate()
        .map(|(index, (media_type, download_code))| {
            let (kind, title, mime_type) = match media_type.as_str() {
                "picture" => (
                    "image",
                    if index == 0 {
                        "image.png".into()
                    } else {
                        format!("image-{}.png", index + 1)
                    },
                    "image/png",
                ),
                "audio" => ("file", "audio.amr".into(), "audio/amr"),
                "video" => ("file", "video.mp4".into(), "video/mp4"),
                "file" => (
                    "file",
                    message
                        .file_content
                        .as_ref()
                        .and_then(|content| content.file_name.clone())
                        .unwrap_or_else(|| "file".into()),
                    "",
                ),
                _ => ("file", "file".into(), ""),
            };
            RemoteAttachment {
                spec: AttachmentSpec::new(kind, title, mime_type)
                    .with_source_id(download_code.clone()),
                download_code,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_message_and_preserves_filename() {
        let message = ChatbotMessage::from_value(&json!({
            "msgtype":"file",
            "content":{"downloadCode":"download-1","fileName":"report.pdf"}
        }))
        .expect("message");
        let attachments = remote_attachments(&message);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].spec.title, "report.pdf");
        assert_eq!(attachments[0].spec.mime_type, "application/pdf");
    }

    #[test]
    fn extracts_audio_recognition_as_message_text() {
        let message = ChatbotMessage::from_value(&json!({
            "msgtype":"audio",
            "content":{"downloadCode":"download-1","recognition":"hello"}
        }))
        .expect("message");
        assert_eq!(inbound_text(&message), "hello");
        assert!(has_attachments(&message));
    }
}
