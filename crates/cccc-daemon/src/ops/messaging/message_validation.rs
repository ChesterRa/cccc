use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

use crate::dispatch::OpError;

pub(super) fn normalize(
    home: &HomeLayout,
    group: &GroupDoc,
    data: &mut Map<String, Value>,
) -> Result<(), OpError> {
    normalize_scope(group, data)?;
    normalize_attachments(home, group, data)?;
    super::install_command::prepare(data);
    Ok(())
}

fn normalize_scope(group: &GroupDoc, data: &mut Map<String, Value>) -> Result<(), OpError> {
    let Some(raw) = data
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let requested = absolute(Path::new(raw)).map_err(|_| {
        OpError::new(
            "scope_not_attached",
            "path does not belong to an attached group scope",
        )
    })?;
    let scope = group.scopes.iter().find(|scope| {
        absolute(Path::new(&scope.url))
            .is_ok_and(|root| requested == root || requested.starts_with(root))
    });
    let Some(scope) = scope else {
        return Err(OpError::new(
            "scope_not_attached",
            "path does not belong to an attached group scope",
        ));
    };
    data.insert("scope_key".into(), Value::String(scope.scope_key.clone()));
    Ok(())
}

fn normalize_attachments(
    home: &HomeLayout,
    group: &GroupDoc,
    data: &mut Map<String, Value>,
) -> Result<(), OpError> {
    let Some(raw) = data.get("attachments") else {
        return Ok(());
    };
    let items = raw
        .as_array()
        .ok_or_else(|| OpError::new("invalid_attachments", "attachments must be an array"))?;
    for item in items {
        let object = item.as_object().ok_or_else(|| {
            OpError::new("invalid_attachments", "each attachment must be an object")
        })?;
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OpError::new("invalid_attachments", "attachment path is required"))?;
        cccc_core::blobs::resolve(home, &group.group_id, path)
            .map_err(|error| OpError::new("invalid_attachments", error.to_string()))?;
    }
    Ok(())
}

fn absolute(path: &Path) -> std::io::Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    path.canonicalize()
}
