use super::inbound_attachments::{AttachmentSpec, ensure_size, store_stream};
use cccc_core::HomeLayout;
use serde_json::Value;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{FileId, FileMeta};

#[derive(Debug, Clone)]
struct RemoteAttachment {
    file: FileMeta,
    kind: &'static str,
    title: String,
    mime_type: String,
}

pub(super) fn has_attachments(message: &Message) -> bool {
    remote_attachments(message).next().is_some()
}

pub(super) async fn materialize_attachments(
    home: &HomeLayout,
    group_id: &str,
    bot: &Bot,
    message: &Message,
) -> Vec<Value> {
    let mut result = Vec::new();
    for attachment in remote_attachments(message) {
        match materialize_attachment(home, group_id, bot, attachment).await {
            Ok(value) => result.push(value),
            Err(error) => tracing::warn!(%error, "failed to download Telegram attachment"),
        }
    }
    result
}

async fn materialize_attachment(
    home: &HomeLayout,
    group_id: &str,
    bot: &Bot,
    attachment: RemoteAttachment,
) -> Result<Value, String> {
    ensure_size(advertised_size(&attachment.file))?;
    let file = bot
        .get_file(FileId(attachment.file.id.0.clone()))
        .await
        .map_err(|error| error.to_string())?;
    let spec = AttachmentSpec::new(attachment.kind, attachment.title, attachment.mime_type)
        .with_source_id(attachment.file.unique_id.0);
    store_stream(home, group_id, bot.download_file_stream(&file.path), spec).await
}

fn advertised_size(file: &FileMeta) -> Option<u64> {
    (file.size != u32::MAX).then_some(file.size.into())
}

fn remote_attachments(message: &Message) -> impl Iterator<Item = RemoteAttachment> + '_ {
    let mut attachments = Vec::new();
    if let Some(document) = message.document() {
        attachments.push(RemoteAttachment {
            file: document.file.clone(),
            kind: if document
                .mime_type
                .as_ref()
                .is_some_and(|mime| mime.to_string().starts_with("image/"))
            {
                "image"
            } else {
                "file"
            },
            title: document.file_name.clone().unwrap_or_else(|| "file".into()),
            mime_type: document
                .mime_type
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        });
    } else if let Some(photo) = message.photo().and_then(|photos| photos.last()) {
        attachments.push(RemoteAttachment {
            file: photo.file.clone(),
            kind: "image",
            title: format!("photo_{}.jpg", photo.file.unique_id.0),
            mime_type: "image/jpeg".into(),
        });
    } else if let Some(video) = message.video() {
        attachments.push(RemoteAttachment {
            file: video.file.clone(),
            kind: "file",
            title: video
                .file_name
                .clone()
                .unwrap_or_else(|| "video.mp4".into()),
            mime_type: video
                .mime_type
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "video/mp4".into()),
        });
    } else if let Some(audio) = message.audio() {
        attachments.push(RemoteAttachment {
            file: audio.file.clone(),
            kind: "file",
            title: audio
                .file_name
                .clone()
                .unwrap_or_else(|| "audio.mp3".into()),
            mime_type: audio
                .mime_type
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "audio/mpeg".into()),
        });
    } else if let Some(voice) = message.voice() {
        attachments.push(RemoteAttachment {
            file: voice.file.clone(),
            kind: "file",
            title: "voice.ogg".into(),
            mime_type: voice
                .mime_type
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "audio/ogg".into()),
        });
    } else if let Some(video_note) = message.video_note() {
        attachments.push(RemoteAttachment {
            file: video_note.file.clone(),
            kind: "file",
            title: "video-note.mp4".into(),
            mime_type: "video/mp4".into(),
        });
    }
    attachments.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_photo_only_message() {
        let message: Message = serde_json::from_value(serde_json::json!({
            "message_id": 7,
            "date": 1,
            "chat": {"id": 42, "type": "private", "first_name": "User"},
            "photo": [{
                "file_id": "photo-id",
                "file_unique_id": "photo-unique",
                "file_size": 3,
                "width": 10,
                "height": 10
            }]
        }))
        .expect("message");

        let attachments = remote_attachments(&message).collect::<Vec<_>>();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].kind, "image");
        assert_eq!(attachments[0].title, "photo_photo-unique.jpg");
    }

    #[test]
    fn missing_telegram_size_is_checked_during_streaming_instead() {
        let file: FileMeta = serde_json::from_value(serde_json::json!({
            "file_id":"file-id",
            "file_unique_id":"file-unique"
        }))
        .expect("file");
        assert_eq!(file.size, u32::MAX);
        assert_eq!(advertised_size(&file), None);
    }
}
