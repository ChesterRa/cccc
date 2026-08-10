use std::path::PathBuf;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use cccc_core::GroupStore;
use cccc_core::group_prompts::{BUILTIN_HELP_MARKDOWN, DEFAULT_PREAMBLE_BODY, PREAMBLE_FILENAME};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, call, object, success};

const HELP_FILENAME: &str = "CCCC_HELP.md";

#[derive(Deserialize)]
struct PromptDeleteQuery {
    #[serde(default)]
    confirm: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/project_md",
            get(project_get).put(project_put),
        )
        .route("/api/v1/groups/{group_id}/prompts", get(prompts_get))
        .route(
            "/api/v1/groups/{group_id}/prompts/{kind}",
            axum::routing::put(prompt_put).delete(prompt_delete),
        )
}

async fn project_get(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let path = scope_path(&state, &group_id, "PROJECT.md").await?;
    let (content, exists) = match std::fs::read_to_string(&path) {
        Ok(content) => (content, true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(error) => {
            return Err(ApiError::bad_code(
                "READ_FAILED",
                error.to_string(),
                json!({"path":path}),
            ));
        }
    };
    Ok(success(
        json!({"content":content,"path":path,"exists":exists}),
    ))
}

async fn project_put(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let path = scope_path(&state, &group_id, "PROJECT.md").await?;
    let content = body.get("content").and_then(Value::as_str).ok_or_else(|| {
        ApiError::bad_code("invalid_content", "content must be a string", json!({}))
    })?;
    std::fs::write(&path, content).map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(success(
        json!({"content":content,"path":path,"exists":true}),
    ))
}

async fn prompts_get(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let root = prompts_root(&state, &group_id)?;
    Ok(success(json!({
        "preamble": prompt_info(&root, "preamble")?,
        "help": prompt_info(&root, "help")?,
    })))
}

async fn prompt_put(
    State(state): State<AppState>,
    Path((group_id, kind)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let normalized_kind = normalize_kind(&kind)?;
    let root = prompts_root(&state, &group_id)?;
    let path = root.join(prompt_filename(normalized_kind));
    let previous = prompt_info(&root, normalized_kind)?["content"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let content = body.get("content").and_then(Value::as_str).ok_or_else(|| {
        ApiError::bad_code("invalid_content", "content must be a string", json!({}))
    })?;
    if content.trim().is_empty() {
        remove_override(&path)?;
        let value = builtin_prompt_info(normalized_kind, &path);
        let (notified, failures) = super::group_prompt_notify::notify(
            &state,
            &group_id,
            normalized_kind == "help" && previous != builtin_prompt(normalized_kind),
        )
        .await?;
        return Ok(success(super::group_prompt_notify::annotate(
            value, notified, failures,
        )));
    }
    std::fs::create_dir_all(&root)
        .and_then(|()| cccc_core::fs::atomic_write(&path, content.as_bytes()))
        .map_err(|error| {
            ApiError::bad_code(
                "WRITE_FAILED",
                format!(
                    "Failed to write {}: {error}",
                    prompt_filename(normalized_kind)
                ),
                json!({}),
            )
        })?;
    let value = home_prompt_info(normalized_kind, &path, content);
    let (notified, failures) = super::group_prompt_notify::notify(
        &state,
        &group_id,
        normalized_kind == "help" && previous != content,
    )
    .await?;
    Ok(success(super::group_prompt_notify::annotate(
        value, notified, failures,
    )))
}

async fn prompt_delete(
    State(state): State<AppState>,
    Path((group_id, kind)): Path<(String, String)>,
    Query(query): Query<PromptDeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let normalized_kind = normalize_kind(&kind)?;
    if query.confirm.trim().to_ascii_lowercase() != normalized_kind {
        return Err(ApiError::bad_code(
            "confirmation_required",
            format!("confirm must equal kind: {normalized_kind}"),
            json!({}),
        ));
    }
    let path = prompts_root(&state, &group_id)?.join(prompt_filename(normalized_kind));
    let root = prompts_root(&state, &group_id)?;
    let previous = prompt_info(&root, normalized_kind)?["content"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    remove_override(&path)?;
    let value = builtin_prompt_info(normalized_kind, &path);
    let (notified, failures) = super::group_prompt_notify::notify(
        &state,
        &group_id,
        normalized_kind == "help" && previous != builtin_prompt(normalized_kind),
    )
    .await?;
    Ok(success(super::group_prompt_notify::annotate(
        value, notified, failures,
    )))
}

async fn scope_root(state: &AppState, group_id: &str) -> Result<std::path::PathBuf, ApiError> {
    let response = call(state, "group_show", object(json!({"group_id":group_id})))
        .await?
        .0;
    let group = response
        .get("result")
        .and_then(|result| result.get("group"))
        .ok_or_else(|| ApiError::not_found("group not found"))?;
    let active = group
        .get("active_scope_key")
        .and_then(Value::as_str)
        .unwrap_or("");
    group
        .get("scopes")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("scope_key").and_then(Value::as_str) == Some(active))
                .or_else(|| items.first())
        })
        .and_then(|item| item.get("url"))
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| ApiError::not_found("group has no scope"))
}

async fn scope_path(
    state: &AppState,
    group_id: &str,
    name: &str,
) -> Result<std::path::PathBuf, ApiError> {
    Ok(scope_root(state, group_id).await?.join(name))
}

fn prompts_root(state: &AppState, group_id: &str) -> Result<PathBuf, ApiError> {
    let store =
        GroupStore::new(state.home.clone()).map_err(|error| ApiError::bad(error.to_string()))?;
    store
        .load(group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))?;
    store
        .group_dir(group_id)
        .map(|path| path.join("prompts"))
        .map_err(|error| ApiError::bad(error.to_string()))
}

fn normalize_kind(kind: &str) -> Result<&'static str, ApiError> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "preamble" => Ok("preamble"),
        "help" => Ok("help"),
        _ => Err(ApiError::bad_code(
            "invalid_kind",
            format!("unknown prompt kind: {kind}"),
            json!({}),
        )),
    }
}

fn prompt_filename(kind: &str) -> &'static str {
    match kind {
        "preamble" => PREAMBLE_FILENAME,
        "help" => HELP_FILENAME,
        _ => unreachable!("kind is normalized before filename lookup"),
    }
}

fn builtin_prompt(kind: &str) -> &'static str {
    match kind {
        "preamble" => DEFAULT_PREAMBLE_BODY,
        "help" => BUILTIN_HELP_MARKDOWN.trim(),
        _ => unreachable!("kind is normalized before builtin lookup"),
    }
}

fn prompt_info(root: &std::path::Path, kind: &str) -> Result<Value, ApiError> {
    let path = root.join(prompt_filename(kind));
    match std::fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => Ok(home_prompt_info(kind, &path, &content)),
        Ok(_) => Ok(builtin_prompt_info(kind, &path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(builtin_prompt_info(kind, &path))
        }
        Err(error) => Err(ApiError::bad(error.to_string())),
    }
}

fn home_prompt_info(kind: &str, path: &std::path::Path, content: &str) -> Value {
    json!({
        "kind": kind,
        "source": "home",
        "filename": prompt_filename(kind),
        "path": path,
        "content": content,
        "notified_actor_ids": [],
    })
}

fn builtin_prompt_info(kind: &str, path: &std::path::Path) -> Value {
    json!({
        "kind": kind,
        "source": "builtin",
        "filename": prompt_filename(kind),
        "path": path,
        "content": builtin_prompt(kind),
        "notified_actor_ids": [],
    })
}

fn remove_override(path: &std::path::Path) -> Result<(), ApiError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ApiError::bad_code(
            "DELETE_FAILED",
            format!("Failed to delete {}: {error}", path.display()),
            json!({}),
        )),
    }
}
