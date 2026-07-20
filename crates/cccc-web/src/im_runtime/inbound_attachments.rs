use cccc_core::{HomeLayout, blobs::BlobUpload};
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};
use std::fmt::Display;
use std::path::Path;
use tokio::io::AsyncReadExt;

pub(super) const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;

pub(super) fn ensure_size(size: Option<u64>) -> Result<(), String> {
    validate_size(size, "before download")
}

#[derive(Debug, Clone)]
pub(super) struct AttachmentSpec {
    pub kind: String,
    pub title: String,
    pub mime_type: String,
    pub source_id: Option<String>,
}

impl AttachmentSpec {
    pub(super) fn new(
        kind: impl Into<String>,
        title: impl Into<String>,
        mime_type: impl Into<String>,
    ) -> Self {
        let title = title.into();
        let supplied_mime = mime_type.into();
        let mime_type = if supplied_mime.trim().is_empty() {
            mime_guess::from_path(&title)
                .first_or_octet_stream()
                .to_string()
        } else {
            supplied_mime
        };
        Self {
            kind: kind.into(),
            title,
            mime_type,
            source_id: None,
        }
    }

    pub(super) fn with_source_id(mut self, source_id: impl Into<String>) -> Self {
        self.source_id = Some(source_id.into());
        self
    }
}

pub(super) async fn download_response(
    home: &HomeLayout,
    group_id: &str,
    response: reqwest::Response,
    advertised_size: Option<u64>,
    spec: AttachmentSpec,
) -> Result<Value, String> {
    validate_size(advertised_size, "before download")?;
    let response = response
        .error_for_status()
        .map_err(|error| error.to_string())?;
    validate_size(response.content_length(), "before read")?;
    store_stream(home, group_id, response.bytes_stream(), spec).await
}

pub(super) async fn store_stream<S, B, E>(
    home: &HomeLayout,
    group_id: &str,
    mut stream: S,
    spec: AttachmentSpec,
) -> Result<Value, String>
where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: Display,
{
    let mut upload = BlobUpload::new(home, group_id).map_err(|error| error.to_string())?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        let chunk = chunk.as_ref();
        validate_size(
            Some(upload.bytes() as u64 + chunk.len() as u64),
            "while downloading",
        )?;
        upload
            .write_chunk(chunk)
            .map_err(|error| error.to_string())?;
    }
    finish_upload(upload, spec)
}

#[cfg(test)]
pub(super) fn store_bytes(
    home: &HomeLayout,
    group_id: &str,
    bytes: &[u8],
    spec: AttachmentSpec,
) -> Result<Value, String> {
    validate_size(Some(bytes.len() as u64), "after download")?;
    let mut upload = BlobUpload::new(home, group_id).map_err(|error| error.to_string())?;
    upload
        .write_chunk(bytes)
        .map_err(|error| error.to_string())?;
    finish_upload(upload, spec)
}

pub(super) async fn store_file(
    home: &HomeLayout,
    group_id: &str,
    path: &Path,
    advertised_size: Option<u64>,
    spec: AttachmentSpec,
) -> Result<Value, String> {
    validate_size(advertised_size, "before download")?;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| error.to_string())?;
    validate_size(
        file.metadata().await.ok().map(|metadata| metadata.len()),
        "before read",
    )?;
    let mut upload = BlobUpload::new(home, group_id).map_err(|error| error.to_string())?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        validate_size(Some(upload.bytes() as u64 + read as u64), "while reading")?;
        upload
            .write_chunk(&buffer[..read])
            .map_err(|error| error.to_string())?;
    }
    finish_upload(upload, spec)
}

fn validate_size(size: Option<u64>, stage: &str) -> Result<(), String> {
    if size.is_some_and(|size| size > MAX_ATTACHMENT_BYTES) {
        Err(format!("attachment exceeds 10 MiB {stage}"))
    } else {
        Ok(())
    }
}

fn finish_upload(upload: BlobUpload, spec: AttachmentSpec) -> Result<Value, String> {
    let blob = upload.finish().map_err(|error| error.to_string())?;
    let mut attachment = json!({
        "kind": spec.kind,
        "path": blob.path,
        "title": spec.title,
        "mime_type": spec.mime_type,
        "bytes": blob.bytes,
        "sha256": blob.sha256,
    });
    if let Some(source_id) = spec.source_id.filter(|value| !value.trim().is_empty()) {
        attachment["source_media_id"] = json!(source_id);
    }
    Ok(attachment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_standard_attachment_and_infers_mime_type() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("attachments", "")
            .expect("group");
        let attachment = store_bytes(
            &home,
            &group.group_id,
            b"png",
            AttachmentSpec::new("image", "photo.png", "").with_source_id("remote-1"),
        )
        .expect("attachment");

        assert_eq!(attachment["kind"], "image");
        assert_eq!(attachment["mime_type"], "image/png");
        assert_eq!(attachment["bytes"], 3);
        assert_eq!(attachment["source_media_id"], "remote-1");
    }

    #[test]
    fn rejects_oversize_bytes_before_blob_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("attachments", "")
            .expect("group");
        let error = store_bytes(
            &home,
            &group.group_id,
            &vec![0; MAX_ATTACHMENT_BYTES as usize + 1],
            AttachmentSpec::new("file", "large.bin", "application/octet-stream"),
        )
        .expect_err("oversize");
        assert!(error.contains("exceeds 10 MiB"));
    }
}
