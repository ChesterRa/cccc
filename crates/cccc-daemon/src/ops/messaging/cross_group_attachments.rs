//! Attachments on a local cross-group message. Blobs live per Group, so the
//! sender's files are verified in the source Group and placed into the
//! destination before either ledger entry is written.
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::path::PathBuf;

use crate::dispatch::OpError;

/// Total attachment size accepted on one cross-group message.
pub(crate) const MAX_BYTES: u64 = 100 * 1024 * 1024;

pub(super) struct Verified {
    files: Vec<(String, PathBuf, String)>,
}

/// Check every attachment against the bytes actually stored in the source
/// Group and rewrite each record from those bytes. Nothing is written.
pub(super) fn verify(
    home: &HomeLayout,
    source_group_id: &str,
    data: &mut Map<String, Value>,
) -> Result<Verified, OpError> {
    let Some(raw) = data.get("attachments") else {
        return Ok(Verified { files: Vec::new() });
    };
    let items = raw
        .as_array()
        .ok_or_else(|| invalid("attachments must be an array"))?;
    let mut records = Vec::with_capacity(items.len());
    let mut files = Vec::with_capacity(items.len());
    let mut total = 0_u64;
    for item in items {
        let object = item
            .as_object()
            .ok_or_else(|| invalid("each attachment must be an object"))?;
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid("attachment path is required"))?;
        let name = path.strip_prefix("state/blobs/").unwrap_or(path).to_owned();
        let source = cccc_core::blobs::resolve(home, source_group_id, &name)
            .map_err(|error| invalid(&error.to_string()))?;
        if std::fs::metadata(&source).map_err(OpError::io)?.len() > MAX_BYTES.saturating_sub(total)
        {
            return Err(OpError::new(
                "attachment_too_large",
                "attachments exceed 100 MiB in total",
            ));
        }
        let (sha256, bytes) = cccc_core::blobs::digest_file(&source).map_err(OpError::io)?;
        if !name[..64].eq_ignore_ascii_case(&sha256) {
            return Err(invalid("stored attachment contents do not match its name"));
        }
        if object
            .get("sha256")
            .and_then(Value::as_str)
            .is_some_and(|declared| !declared.eq_ignore_ascii_case(&sha256))
            || object
                .get("bytes")
                .and_then(Value::as_u64)
                .is_some_and(|declared| declared != bytes)
        {
            return Err(invalid("attachment does not match the stored file"));
        }
        total = total.saturating_add(bytes);
        if total > MAX_BYTES {
            return Err(OpError::new(
                "attachment_too_large",
                "attachments exceed 100 MiB in total",
            ));
        }
        let mime_type = object
            .get("mime_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("application/octet-stream")
            .to_ascii_lowercase();
        let title = object
            .get("title")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&name)
            .to_owned();
        records.push(json!({
            "kind": if mime_type.starts_with("image/") { "image" } else { "file" },
            "path": format!("state/blobs/{name}"),
            "title": title,
            "mime_type": mime_type,
            "bytes": bytes,
            "sha256": sha256,
        }));
        files.push((name, source, sha256));
    }
    data.insert("attachments".into(), Value::Array(records));
    Ok(Verified { files })
}

impl Verified {
    /// Make the verified files readable in the destination Group. Placing is
    /// idempotent, so a retried relay can repeat it safely.
    pub(super) fn place(
        &self,
        home: &HomeLayout,
        destination_group_id: &str,
    ) -> Result<(), OpError> {
        for (name, source, sha256) in &self.files {
            cccc_core::blobs::place(home, destination_group_id, name, source, sha256)
                .map_err(OpError::io)?;
        }
        Ok(())
    }
}

fn invalid(message: &str) -> OpError {
    OpError::new("invalid_attachments", message)
}
