//! Shared source-scope file admission for local and Connect messages.
use crate::dispatch::OpError;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(crate) struct PreparedFiles {
    root: PathBuf,
    sources: Vec<(PathBuf, Vec<u8>)>,
}

pub(crate) fn read(
    group: &GroupDoc,
    paths: &[Value],
    maximum_bytes: u64,
) -> Result<PreparedFiles, OpError> {
    let scope = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key && !scope.url.trim().is_empty())
        .ok_or_else(|| OpError::new("missing_scope", "group has no active scope"))?;
    let root = fs::canonicalize(Path::new(&scope.url))
        .map_err(|error| OpError::new("missing_scope", error.to_string()))?;

    let mut sources: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(paths.len());
    let mut total = 0_u64;
    for raw_path in paths {
        let raw = raw_path
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OpError::new("invalid_path", "file path must be a non-empty string"))?;
        let candidate = Path::new(raw);
        let candidate = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        };
        let source = fs::canonicalize(&candidate).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                OpError::new(
                    "not_found",
                    format!("file not found: {}", candidate.display()),
                )
            } else {
                OpError::new("read_failed", error.to_string())
            }
        })?;
        if !source.starts_with(&root) {
            return Err(OpError::new(
                "invalid_path",
                "file path must be under the group's active scope root",
            ));
        }
        if !source.is_file() {
            return Err(OpError::new(
                "not_found",
                format!("file not found: {}", source.display()),
            ));
        }
        let mut data = Vec::new();
        fs::File::open(&source)
            .and_then(|file| {
                file.take(maximum_bytes.saturating_sub(total).saturating_add(1))
                    .read_to_end(&mut data)
            })
            .map_err(|error| OpError::new("read_failed", error.to_string()))?;
        total = total.saturating_add(data.len() as u64);
        if total > maximum_bytes {
            return Err(OpError::new(
                "attachment_too_large",
                "combined files exceed the message attachment limit",
            ));
        }
        sources.push((source, data));
    }

    Ok(PreparedFiles { root, sources })
}

impl PreparedFiles {
    pub(crate) fn apply(
        self,
        home: &HomeLayout,
        group: &GroupDoc,
        data: &mut Map<String, Value>,
    ) -> Result<(), OpError> {
        let Self { root, sources } = self;
        let mut attachments = Vec::with_capacity(sources.len());
        let mut titles = Vec::with_capacity(sources.len());
        for (source, data) in sources {
            let title = source
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("file")
                .to_owned();
            let mime_type = mime_guess::from_path(&source)
                .first_or_octet_stream()
                .essence_str()
                .to_owned();
            let kind = if mime_type.starts_with("image/") {
                "image"
            } else {
                "file"
            };
            let blob =
                cccc_core::blobs::store(home, &group.group_id, &data).map_err(OpError::io)?;
            attachments.push(json!({
                "kind":kind,
                "path":blob.path,
                "title":title,
                "mime_type":mime_type,
                "bytes":blob.bytes,
                "sha256":blob.sha256,
            }));
            titles.push(title);
        }

        data.remove("paths");
        data.insert("attachments".into(), Value::Array(attachments));
        data.insert(
            "path".into(),
            Value::String(root.to_string_lossy().into_owned()),
        );
        if data
            .get("text")
            .and_then(Value::as_str)
            .is_none_or(|text| text.trim().is_empty())
        {
            data.insert(
                "text".into(),
                Value::String(format!("[files] {}", titles.join(", "))),
            );
        }
        Ok(())
    }
}
