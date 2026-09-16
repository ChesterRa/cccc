//! Workspace file tree for the Files panel, confined to the group's active scope.
//!
//! Reads and writes go through `cccc_core::workspace`, which owns the scope boundary.
//! The whole surface is disabled in exhibit mode: it is an operator tool, not a demo view.

use axum::Router;
use axum::extract::Json as JsonBody;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use cccc_core::GroupStore;
use cccc_core::workspace::{self, ListOptions, WriteOutcome};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

mod changes;
mod content;
mod entries;

#[derive(Deserialize)]
struct ListQuery {
    scope_key: String,
    scope_url: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    show_ignored: bool,
}

#[derive(Deserialize)]
struct FileQuery {
    scope_key: String,
    scope_url: String,
    #[serde(default)]
    path: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/workspace/changes",
            get(changes::list),
        )
        .route(
            "/api/v1/groups/{group_id}/workspace/diff",
            get(changes::diff),
        )
        .route(
            "/api/v1/groups/{group_id}/workspace/entries",
            post(entries::change),
        )
        .route(
            "/api/v1/groups/{group_id}/workspace/upload",
            post(entries::upload),
        )
        .route("/api/v1/groups/{group_id}/workspace/list", get(list))
        .route(
            "/api/v1/groups/{group_id}/workspace/path",
            get(inspect_path),
        )
        .route(
            "/api/v1/groups/{group_id}/workspace/content",
            get(content::read),
        )
        .route(
            "/api/v1/groups/{group_id}/workspace/file",
            get(read).put(write),
        )
}

async fn inspect_path(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<FileQuery>,
) -> ApiResult {
    tokio::task::spawn_blocking(move || {
        let group = load_group(&state, &group_id, &query.scope_key, &query.scope_url)?;
        let (path, is_dir) = workspace::inspect_path(&group, &query.path)
            .map_err(|error| path_error(&query.path, error))?;
        Ok(success(json!({
            "scope_key": group.active_scope_key,
            "scope_url": query.scope_url,
            "path": path,
            "is_dir": is_dir,
        })))
    })
    .await
    .map_err(|error| ApiError::unavailable("workspace_unavailable", error.to_string()))?
}

async fn list(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> ApiResult {
    tokio::task::spawn_blocking(move || {
        let group = load_group(&state, &group_id, &query.scope_key, &query.scope_url)?;
        let listing = workspace::list(
            &group,
            &query.path,
            ListOptions {
                show_ignored: query.show_ignored,
            },
        )
        .map_err(|error| path_error(&query.path, error))?;
        Ok(success(json!({
            "scope_key": group.active_scope_key,
            "scope_url": query.scope_url,
            "root_path": listing.root,
            "path": listing.path,
            "parent": listing.parent,
            "items": listing.items,
        })))
    })
    .await
    .map_err(|error| ApiError::unavailable("workspace_unavailable", error.to_string()))?
}

async fn read(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<FileQuery>,
) -> ApiResult {
    tokio::task::spawn_blocking(move || {
        let group = load_group(&state, &group_id, &query.scope_key, &query.scope_url)?;
        let file = workspace::read_file(&group, &query.path)
            .map_err(|error| path_error(&query.path, error))?;
        Ok(success(serde_json::to_value(file).unwrap_or_default()))
    })
    .await
    .map_err(|error| ApiError::unavailable("workspace_unavailable", error.to_string()))?
}

async fn write(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    JsonBody(body): JsonBody<Value>,
) -> ApiResult {
    tokio::task::spawn_blocking(move || {
        let group = load_group(
            &state,
            &group_id,
            &text(&body, "scope_key"),
            &text(&body, "scope_url"),
        )?;
        let path = text(&body, "path");
        let content = body.get("content").and_then(Value::as_str).ok_or_else(|| {
            ApiError::bad_code("invalid_content", "content must be a string", json!({}))
        })?;
        let expected = text(&body, "sha256");
        match workspace::write_file(&group, &path, content, &expected)
            .map_err(|error| path_error(&path, error))?
        {
            WriteOutcome::Written { sha256, created } => Ok(success(json!({
                "path": path,
                "sha256": sha256,
                "created": created,
            }))),
            WriteOutcome::Conflict { sha256 } => Err(ApiError::conflict(
                "workspace_write_conflict",
                "the file changed on disk since it was opened",
                json!({"path": path, "sha256": sha256}),
            )),
        }
    })
    .await
    .map_err(|error| ApiError::unavailable("workspace_unavailable", error.to_string()))?
}

fn load_group(
    state: &AppState,
    group_id: &str,
    scope_key: &str,
    scope_url: &str,
) -> Result<cccc_core::GroupDoc, ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden_code(
            "read_only",
            "workspace files are unavailable in exhibit mode",
        ));
    }
    let store =
        GroupStore::new(state.home.clone()).map_err(|error| ApiError::bad(error.to_string()))?;
    if scope_key.is_empty() || scope_url.is_empty() {
        return Err(ApiError::bad_code(
            "invalid_scope",
            "scope_key and scope_url are required",
            json!({}),
        ));
    }
    let group = store
        .load(group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))?;
    if scope_key != group.active_scope_key
        || !group
            .scopes
            .iter()
            .any(|scope| scope.scope_key == scope_key && scope.url == scope_url)
    {
        return Err(ApiError::conflict(
            "workspace_scope_changed",
            "The active workspace changed. Reopen the file before saving.",
            json!({}),
        ));
    }
    // Resolve the whole operation against this checked snapshot, never a later active scope.
    Ok(group)
}

fn text(body: &Value, key: &str) -> String {
    body.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Keeps "outside the scope" distinguishable from "missing", so the panel can say which.
fn path_error(raw: &str, error: std::io::Error) -> ApiError {
    match error.kind() {
        std::io::ErrorKind::AlreadyExists => ApiError::conflict(
            "workspace_entry_exists",
            format!("An entry already exists at the destination: {raw}"),
            json!({}),
        ),
        std::io::ErrorKind::NotFound => {
            ApiError::not_found_code("NOT_FOUND", format!("Path not found: {raw}"))
        }
        std::io::ErrorKind::PermissionDenied => {
            ApiError::forbidden_code("PERMISSION", format!("Permission denied: {raw}"))
        }
        std::io::ErrorKind::IsADirectory => ApiError::bad_code(
            "not_a_file",
            format!("Path is a directory, not a file: {raw}"),
            json!({}),
        ),
        std::io::ErrorKind::NotADirectory => ApiError::bad_code(
            "not_a_directory",
            format!("Path is not a directory: {raw}"),
            json!({}),
        ),
        _ if error
            .get_ref()
            .is_some_and(|inner| inner.is::<workspace::OutsideScope>()) =>
        {
            ApiError::forbidden_code(
                "outside_scope",
                format!("Path is outside the group workspace: {raw}"),
            )
        }
        _ => ApiError::bad_code("workspace_error", error.to_string(), json!({})),
    }
}
