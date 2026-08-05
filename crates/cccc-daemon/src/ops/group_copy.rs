use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::group_copy;
use serde_json::json;
use std::fs;
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_copy_export" => export(home, request, false),
        "group_copy_export_file" => export(home, request, true),
        "group_copy_preview_import" => preview(home, request),
        "group_copy_import" => import(home, request),
        _ => return None,
    })
}

fn export(home: &HomeLayout, request: &DaemonRequest, to_file: bool) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let (bytes, manifest, filename) =
        group_copy::export(&store(home)?, &group_id).map_err(OpError::io)?;
    if !to_file {
        return object(json!({
            "package_b64":STANDARD.encode(bytes),
            "filename":filename,
            "manifest":manifest
        }));
    }
    let directory = home.root().join("tmp/group-copy-export");
    fs::create_dir_all(&directory).map_err(OpError::io)?;
    let path = directory.join(format!("{}.zip", Uuid::new_v4().simple()));
    cccc_core::fs::atomic_write(&path, &bytes).map_err(OpError::io)?;
    object(json!({
        "package_path":path,
        "package_size_bytes":bytes.len(),
        "filename":filename,
        "manifest":manifest
    }))
}

fn preview(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let bytes = package_bytes(home, request)?;
    let preview = group_copy::preview(&store(home)?, &bytes).map_err(OpError::invalid)?;
    object(json!({"preview":preview}))
}

fn import(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let bytes = package_bytes(home, request)?;
    let result = group_copy::import(
        &store(home)?,
        &bytes,
        &string_arg(request, "workspace_root").unwrap_or_default(),
        &string_arg(request, "title").unwrap_or_default(),
    )
    .map_err(OpError::invalid)?;
    object(result)
}

fn package_bytes(home: &HomeLayout, request: &DaemonRequest) -> Result<Vec<u8>, OpError> {
    let encoded = string_arg(request, "package_b64").unwrap_or_default();
    let package_path = string_arg(request, "package_path").unwrap_or_default();
    if encoded.is_empty() == package_path.is_empty() {
        return Err(OpError::new(
            "invalid_args",
            "exactly one of package_b64 or package_path is required",
        ));
    }
    if !encoded.is_empty() {
        return STANDARD.decode(encoded).map_err(OpError::invalid);
    }
    let path = std::path::Path::new(&package_path)
        .canonicalize()
        .map_err(OpError::io)?;
    let tmp = home
        .root()
        .join("tmp")
        .canonicalize()
        .map_err(OpError::io)?;
    if !path.starts_with(tmp) || !path.is_file() {
        return Err(OpError::new(
            "invalid_args",
            "package_path must be a staged file under CCCC_HOME/tmp",
        ));
    }
    if fs::metadata(&path).map_err(OpError::io)?.len() > group_copy::MAX_PACKAGE_BYTES as u64 {
        return Err(OpError::new(
            "invalid_args",
            "group copy package exceeds 1 GiB",
        ));
    }
    fs::read(path).map_err(OpError::io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn staged_package_is_size_checked_before_reading() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let path = home.root().join("tmp/oversized.zip");
        fs::create_dir_all(path.parent().expect("tmp directory")).expect("create tmp");
        let file = File::create(&path).expect("file");
        file.set_len(group_copy::MAX_PACKAGE_BYTES as u64 + 1)
            .expect("sparse file");
        let request = DaemonRequest {
            v: 1,
            op: "group_copy_import".into(),
            args: serde_json::from_value(json!({"package_path": path})).expect("args"),
        };

        let error = package_bytes(&home, &request).expect_err("oversized package");
        assert_eq!(error.code, "invalid_args");
        assert!(error.message.contains("1 GiB"));
    }
}
