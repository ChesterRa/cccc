use super::inbound_attachments::{AttachmentSpec, download_response, ensure_size};
use cccc_core::HomeLayout;
use serde_json::Value;
use serenity::all::Attachment;

pub(super) async fn materialize_attachments(
    home: &HomeLayout,
    group_id: &str,
    http: &reqwest::Client,
    attachments: &[Attachment],
) -> Vec<Value> {
    let mut result = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        match materialize_attachment(home, group_id, http, attachment).await {
            Ok(value) => result.push(value),
            Err(error) => {
                tracing::warn!(
                    %error,
                    attachment_id = %attachment.id,
                    "failed to download Discord attachment"
                );
            }
        }
    }
    result
}

async fn materialize_attachment(
    home: &HomeLayout,
    group_id: &str,
    http: &reqwest::Client,
    attachment: &Attachment,
) -> Result<Value, String> {
    ensure_size(Some(attachment.size.into()))?;
    let mime_type = attachment
        .content_type
        .as_deref()
        .unwrap_or_default()
        .to_owned();
    let inferred_mime = if mime_type.is_empty() {
        mime_guess::from_path(&attachment.filename)
            .first_or_octet_stream()
            .to_string()
    } else {
        mime_type.clone()
    };
    let spec = AttachmentSpec::new(
        if inferred_mime.starts_with("image/") {
            "image"
        } else {
            "file"
        },
        attachment.filename.clone(),
        mime_type,
    )
    .with_source_id(attachment.id.to_string());
    let response = http
        .get(&attachment.url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    download_response(home, group_id, response, Some(attachment.size.into()), spec).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};

    #[tokio::test]
    async fn downloads_discord_image_into_group_blob() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let url = format!("http://{}/image", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/image",
                    get(|| async { ([("content-type", "image/png")], b"png".to_vec()) }),
                ),
            )
            .await
            .expect("server");
        });
        let attachment: Attachment = serde_json::from_value(serde_json::json!({
            "id":"1",
            "filename":"image.png",
            "size":3,
            "url":url,
            "proxy_url":url,
            "content_type":"image/png"
        }))
        .expect("attachment");
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("discord", "")
            .expect("group");

        let value =
            materialize_attachment(&home, &group.group_id, &reqwest::Client::new(), &attachment)
                .await
                .expect("download");
        assert_eq!(value["kind"], "image");
        assert_eq!(value["title"], "image.png");
        assert_eq!(value["bytes"], 3);
        let path = value["path"].as_str().expect("path");
        assert_eq!(
            std::fs::read(cccc_core::blobs::resolve(&home, &group.group_id, path).expect("blob"))
                .expect("bytes"),
            b"png"
        );
        server.abort();
    }

    #[tokio::test]
    async fn rejects_advertised_oversize_before_request() {
        let attachment: Attachment = serde_json::from_value(serde_json::json!({
            "id":"1",
            "filename":"large.bin",
            "size":super::super::inbound_attachments::MAX_ATTACHMENT_BYTES + 1,
            "url":"http://127.0.0.1:1/unreachable",
            "proxy_url":"http://127.0.0.1:1/unreachable"
        }))
        .expect("attachment");
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("discord", "")
            .expect("group");

        let error =
            materialize_attachment(&home, &group.group_id, &reqwest::Client::new(), &attachment)
                .await
                .expect_err("oversize");
        assert!(error.contains("exceeds 10 MiB"));
    }
}
