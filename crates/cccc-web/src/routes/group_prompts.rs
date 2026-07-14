use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, call, object, success};

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
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    Ok(success(
        json!({"content":content,"path":path,"exists":path.exists()}),
    ))
}

async fn project_put(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let path = scope_path(&state, &group_id, "PROJECT.md").await?;
    let content = body.get("content").and_then(Value::as_str).unwrap_or("");
    std::fs::write(&path, content).map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(success(
        json!({"content":content,"path":path,"exists":true}),
    ))
}

async fn prompts_get(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let root = scope_root(&state, &group_id).await?;
    let prompts: Vec<_> = ["AGENTS.md", "CLAUDE.md"]
        .iter()
        .map(|name| {
            let path = root.join(name);
            json!({"kind":name,"path":path,"exists":path.exists(),"content":std::fs::read_to_string(path).unwrap_or_default()})
        })
        .collect();
    Ok(success(json!({"prompts":prompts})))
}

async fn prompt_put(
    State(state): State<AppState>,
    Path((group_id, kind)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let path = scope_path(&state, &group_id, prompt_name(&kind)?).await?;
    let content = body.get("content").and_then(Value::as_str).unwrap_or("");
    std::fs::write(&path, content).map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(success(
        json!({"kind":kind,"path":path,"content":content,"exists":true}),
    ))
}

async fn prompt_delete(
    State(state): State<AppState>,
    Path((group_id, kind)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let path = scope_path(&state, &group_id, prompt_name(&kind)?).await?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| ApiError::bad(error.to_string()))?;
    }
    Ok(success(json!({"kind":kind,"deleted":true})))
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

fn prompt_name(kind: &str) -> Result<&'static str, ApiError> {
    match kind {
        "agents" | "AGENTS.md" => Ok("AGENTS.md"),
        "claude" | "CLAUDE.md" => Ok("CLAUDE.md"),
        _ => Err(ApiError::bad("unsupported prompt kind")),
    }
}
