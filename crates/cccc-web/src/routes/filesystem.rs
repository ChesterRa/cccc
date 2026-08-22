use axum::Router;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io;

mod create_directory;
mod path_display;

use path_display::{display_path, drive_label, filesystem_roots};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_path")]
    path: String,
    #[serde(default)]
    show_hidden: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/fs/list", get(list))
        .route("/api/v1/fs/recent", get(recent))
        .route("/api/v1/fs/directory", post(create_directory::create))
}

async fn list(State(state): State<AppState>, Query(query): Query<ListQuery>) -> ApiResult {
    require_filesystem_access(&state)?;
    let target = cccc_core::path_input::resolve_existing_directory(&query.path)
        .map_err(|error| path_error(&query.path, error))?;
    let (mut items, readable) =
        directory_items(&target).map_err(|error| path_error(&query.path, error))?;
    if !query.show_hidden {
        items.retain(|item| {
            !item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with('.'))
        });
    }
    items.sort_by(|left, right| {
        let left_key = (!left["is_dir"].as_bool().unwrap_or(false), lower_name(left));
        let right_key = (
            !right["is_dir"].as_bool().unwrap_or(false),
            lower_name(right),
        );
        left_key.cmp(&right_key)
    });
    items.truncate(100);
    Ok(success(json!({
        "path": display_path(&target),
        "parent": target.parent().filter(|parent| *parent != target).map(display_path),
        "items": items,
        "readable": readable,
    })))
}

async fn recent(State(state): State<AppState>) -> ApiResult {
    require_filesystem_access(&state)?;
    Ok(success(json!({"suggestions": recent_suggestions()})))
}

fn require_filesystem_access(state: &AppState) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden_code(
            "read_only",
            "filesystem browsing is disabled in exhibit mode",
        ));
    }
    Ok(())
}

fn recent_suggestions() -> Vec<Value> {
    let Ok(home) = cccc_core::path_input::expand_user_path("~") else {
        return Vec::new();
    };
    let mut candidates = vec![("Home".to_owned(), home.clone(), "home")];
    candidates.extend(
        filesystem_roots()
            .into_iter()
            .map(|path| (format!("Drive {}", drive_label(&path)), path, "drive")),
    );
    candidates.extend(
        [
            "dev",
            "projects",
            "code",
            "src",
            "workspace",
            "repos",
            "github",
            "work",
        ]
        .map(|name| (name.to_owned(), home.join(name), "folder")),
    );
    candidates.extend([
        ("Desktop".to_owned(), home.join("Desktop"), "desktop"),
        ("Documents".to_owned(), home.join("Documents"), "document"),
        ("Downloads".to_owned(), home.join("Downloads"), "download"),
    ]);
    if let Ok(cwd) = std::env::current_dir()
        && cwd != home
    {
        candidates.push(("Current Dir".to_owned(), cwd, "current"));
    }
    candidates
        .into_iter()
        .filter(|(_, path, _)| path.is_dir())
        .take(10)
        .map(|(name, path, icon)| {
            json!({"name": title(&name), "path": display_path(&path), "icon": icon})
        })
        .collect()
}

fn directory_item(entry: std::fs::DirEntry) -> Value {
    let path = entry.path();
    json!({
        "name": entry.file_name().to_string_lossy(),
        "path": display_path(&path),
        "is_dir": path.is_dir(),
    })
}

fn directory_items(path: &std::path::Path) -> io::Result<(Vec<Value>, bool)> {
    let result = std::fs::read_dir(path).and_then(|entries| {
        entries
            .map(|entry| entry.map(directory_item))
            .collect::<io::Result<Vec<_>>>()
    });
    readable_items(result)
}

fn readable_items(result: io::Result<Vec<Value>>) -> io::Result<(Vec<Value>, bool)> {
    match result {
        Ok(items) => Ok((items, true)),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Ok((Vec::new(), false)),
        Err(error) => Err(error),
    }
}

fn lower_name(value: &Value) -> String {
    value["name"].as_str().unwrap_or("").to_lowercase()
}

fn path_error(raw: &str, error: io::Error) -> ApiError {
    match error.kind() {
        io::ErrorKind::NotFound => {
            ApiError::not_found_code("NOT_FOUND", format!("Path not found: {raw}"))
        }
        io::ErrorKind::NotADirectory => {
            ApiError::bad_code("NOT_DIR", format!("Not a directory: {raw}"), json!({}))
        }
        io::ErrorKind::PermissionDenied => {
            ApiError::forbidden_code("PERMISSION", format!("Permission denied: {raw}"))
        }
        _ => ApiError::bad_code("filesystem_error", error.to_string(), json!({})),
    }
}

fn default_path() -> String {
    "~".into()
}

fn title(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{path_error, readable_items};
    use std::io;

    #[test]
    fn permission_errors_remain_explicit() {
        let error = path_error(
            "/private",
            io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
        );
        assert!(error.to_string().starts_with("PERMISSION:"));
        let response = axum::response::IntoResponse::into_response(error);
        assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn enumeration_permission_errors_keep_the_directory_selectable() {
        let (items, readable) = readable_items(Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "denied",
        )))
        .expect("permission error should degrade");
        assert!(items.is_empty());
        assert!(!readable);
    }
}
