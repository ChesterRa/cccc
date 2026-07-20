use cccc_core::{HomeLayout, blobs::BlobUpload};
use futures_util::StreamExt;
use serde_json::{Value, json};

const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

pub(super) fn has_files(event: &Value) -> bool {
    event
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(|files| !files.is_empty())
}

pub(super) fn message_id(event: &Value) -> String {
    ["client_msg_id", "event_ts", "ts"]
        .into_iter()
        .find_map(|key| {
            event
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_default()
        .to_owned()
}

pub(super) async fn materialize_files(
    home: &HomeLayout,
    group_id: &str,
    http: &reqwest::Client,
    bot_token: &str,
    event: &Value,
) -> Vec<Value> {
    let Some(files) = event.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut attachments = Vec::with_capacity(files.len());
    for file in files {
        match materialize_file(home, group_id, http, bot_token, file).await {
            Ok(attachment) => attachments.push(attachment),
            Err(error) => {
                let file_id = file.get("id").and_then(Value::as_str).unwrap_or("");
                tracing::warn!(%error, %file_id, "failed to download Slack attachment");
            }
        }
    }
    attachments
}

async fn materialize_file(
    home: &HomeLayout,
    group_id: &str,
    http: &reqwest::Client,
    bot_token: &str,
    file: &Value,
) -> Result<Value, String> {
    if file
        .get("size")
        .and_then(Value::as_u64)
        .is_some_and(|size| size > MAX_ATTACHMENT_BYTES as u64)
    {
        return Err("attachment exceeds 10 MiB before download".into());
    }
    let url = file
        .get("url_private_download")
        .or_else(|| file.get("url_private"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Slack attachment has no private download URL".to_owned())?;
    let response = http
        .get(url)
        .bearer_auth(bot_token)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ATTACHMENT_BYTES as u64)
    {
        return Err("attachment exceeds 10 MiB before read".into());
    }

    let mut upload = BlobUpload::new(home, group_id).map_err(|error| error.to_string())?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        if upload.bytes().saturating_add(chunk.len()) > MAX_ATTACHMENT_BYTES {
            return Err("attachment exceeds 10 MiB while downloading".into());
        }
        upload
            .write_chunk(&chunk)
            .map_err(|error| error.to_string())?;
    }
    let blob = upload.finish().map_err(|error| error.to_string())?;
    let mime_type = file
        .get("mimetype")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("application/octet-stream");
    let title = file
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("file");
    Ok(json!({
        "kind": if mime_type.starts_with("image/") { "image" } else { "file" },
        "path": blob.path,
        "title": title,
        "mime_type": mime_type,
        "bytes": blob.bytes,
        "sha256": blob.sha256,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::HeaderMap, routing::get};

    #[tokio::test]
    async fn downloads_authenticated_slack_image_into_group_blob() {
        async fn download(headers: HeaderMap) -> ([(&'static str, &'static str); 1], Vec<u8>) {
            assert_eq!(
                headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer token")
            );
            ([("content-type", "image/png")], b"png-bytes".to_vec())
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let url = format!("http://{}/file", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/file", get(download)))
                .await
                .expect("server");
        });
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("slack", "")
            .expect("group");
        let event = json!({
            "files":[{
                "id":"F1","name":"image.png","mimetype":"image/png","size":9,
                "url_private_download":url
            }]
        });

        let attachments = materialize_files(
            &home,
            &group.group_id,
            &reqwest::Client::new(),
            "token",
            &event,
        )
        .await;

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0]["kind"], "image");
        assert_eq!(attachments[0]["title"], "image.png");
        let path = attachments[0]["path"].as_str().expect("path");
        assert_eq!(
            std::fs::read(cccc_core::blobs::resolve(&home, &group.group_id, path).expect("blob"))
                .expect("read"),
            b"png-bytes"
        );
        server.abort();
    }

    #[tokio::test]
    async fn skips_advertised_oversize_attachment_without_downloading() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("slack", "")
            .expect("group");
        let event = json!({"files":[{
            "id":"F1","name":"large.bin","size":MAX_ATTACHMENT_BYTES + 1,
            "url_private_download":"http://127.0.0.1:1/unreachable"
        }]});

        let attachments = materialize_files(
            &home,
            &group.group_id,
            &reqwest::Client::new(),
            "token",
            &event,
        )
        .await;

        assert!(attachments.is_empty());
    }

    #[test]
    fn extracts_stable_message_identity_and_file_presence() {
        let event = json!({"client_msg_id":"C1","event_ts":"E1","files":[{}]});
        assert_eq!(message_id(&event), "C1");
        assert!(has_files(&event));
    }
}
