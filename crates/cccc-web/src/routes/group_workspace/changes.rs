use crate::{
    AppState,
    api::{ApiError, ApiResult, success},
};
use axum::extract::{Path, Query, State};
use cccc_core::workspace_changes::{self, DiffSide};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct ChangesQuery {
    scope_key: String,
    scope_url: String,
}
#[derive(Deserialize)]
pub(super) struct DiffQuery {
    scope_key: String,
    scope_url: String,
    path: String,
    side: DiffSide,
}

pub(super) async fn list(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<ChangesQuery>,
) -> ApiResult {
    tokio::task::spawn_blocking(move || {
        let group = super::load_group(&state, &group_id, &query.scope_key, &query.scope_url)?;
        let changes = workspace_changes::changes(&group).map_err(|e| {
            ApiError::bad_code(
                "git_inspection_failed",
                e.to_string(),
                serde_json::json!({}),
            )
        })?;
        Ok(success(serde_json::to_value(changes).unwrap_or_default()))
    })
    .await
    .map_err(|e| ApiError::unavailable("workspace_unavailable", e.to_string()))?
}
pub(super) async fn diff(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<DiffQuery>,
) -> ApiResult {
    tokio::task::spawn_blocking(move || {
        let group = super::load_group(&state, &group_id, &query.scope_key, &query.scope_url)?;
        let patch = workspace_changes::diff(&group, &query.path, query.side).map_err(|e| {
            ApiError::bad_code(
                "git_inspection_failed",
                e.to_string(),
                serde_json::json!({}),
            )
        })?;
        Ok(success(serde_json::to_value(patch).unwrap_or_default()))
    })
    .await
    .map_err(|e| ApiError::unavailable("workspace_unavailable", e.to_string()))?
}
