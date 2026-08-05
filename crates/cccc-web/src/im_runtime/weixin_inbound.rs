use super::inbound_attachments::{AttachmentSpec, ensure_size, store_file};
use cccc_core::HomeLayout;
use serde_json::Value;
use weixin_agent::{MediaInfo, MediaType, MessageContext};

pub(super) fn has_media(context: &MessageContext) -> bool {
    context.media.is_some()
}

pub(super) async fn materialize_media(
    home: &HomeLayout,
    group_id: &str,
    context: &MessageContext,
) -> Vec<Value> {
    let Some(media) = context.media.as_ref() else {
        return Vec::new();
    };
    match materialize_one(home, group_id, context, media).await {
        Ok(attachment) => vec![attachment],
        Err(error) => {
            tracing::warn!(%error, "failed to download Weixin attachment");
            Vec::new()
        }
    }
}

async fn materialize_one(
    home: &HomeLayout,
    group_id: &str,
    context: &MessageContext,
    media: &MediaInfo,
) -> Result<Value, String> {
    ensure_size(media.file_size)?;
    let (kind, default_title, default_mime) = media_defaults(media.media_type);
    let title = media
        .file_name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_title.into());
    let guessed_mime = mime_guess::from_path(&title)
        .first()
        .map_or_else(|| default_mime.to_owned(), |mime| mime.to_string());
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    let download_path = temp.path().join("download");
    context
        .download_media(media, &download_path)
        .await
        .map_err(|error| error.to_string())?;
    store_file(
        home,
        group_id,
        &download_path,
        media.file_size,
        AttachmentSpec::new(kind, title, guessed_mime),
    )
    .await
}

fn media_defaults(media_type: MediaType) -> (&'static str, &'static str, &'static str) {
    match media_type {
        MediaType::Image => ("image", "image.jpg", "image/jpeg"),
        MediaType::Video => ("file", "video.mp4", "video/mp4"),
        MediaType::Voice => ("file", "voice.silk", "audio/silk"),
        MediaType::File => ("file", "file", "application/octet-stream"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_media_types_to_stable_attachment_metadata() {
        assert_eq!(
            media_defaults(MediaType::Image),
            ("image", "image.jpg", "image/jpeg")
        );
        assert_eq!(
            media_defaults(MediaType::Voice),
            ("file", "voice.silk", "audio/silk")
        );
        assert_eq!(media_defaults(MediaType::File).0, "file");
    }
}
