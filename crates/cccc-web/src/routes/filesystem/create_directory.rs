use axum::extract::{Json, State};
use serde::Deserialize;
use serde_json::json;
use std::io;

use super::path_display::display_path;
use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

#[derive(Deserialize)]
pub(super) struct CreateDirectoryInput {
    parent: String,
    name: String,
}

pub(super) async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateDirectoryInput>,
) -> ApiResult {
    super::require_filesystem_access(&state)?;
    let parent = cccc_core::path_input::resolve_existing_directory(&input.parent)
        .map_err(|error| super::path_error(&input.parent, error))?;
    let name = validate_name(&input.name)?;
    let target = parent.join(name);
    std::fs::create_dir(&target).map_err(|error| match error.kind() {
        io::ErrorKind::AlreadyExists => ApiError::conflict(
            "ALREADY_EXISTS",
            format!("Path already exists: {}", display_path(&target)),
            json!({}),
        ),
        _ => super::path_error(&display_path(&target), error),
    })?;
    Ok(success(json!({"path": display_path(&target)})))
}

fn validate_name(raw: &str) -> Result<&str, ApiError> {
    let name = raw.trim();
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', '\0']) {
        return Err(ApiError::bad_code(
            "INVALID_NAME",
            "Directory name must be a single non-empty path segment",
            json!({}),
        ));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::validate_name;

    #[test]
    fn names_must_be_single_path_segments() {
        assert_eq!(validate_name(" project ").expect("valid"), "project");
        for value in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(validate_name(value).is_err(), "{value:?}");
        }
    }
}
