use axum::Router;
use axum::extract::Query;
use axum::routing::get;
use serde_json::json;

use crate::AppState;
use crate::api::{ApiResult, success};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/fs/scope_root", get(scope_root))
}

#[derive(serde::Deserialize)]
struct ScopeRootQuery {
    #[serde(default)]
    path: String,
}

async fn scope_root(Query(query): Query<ScopeRootQuery>) -> ApiResult {
    let raw = query.path.trim();
    if raw.is_empty() {
        return Err(crate::api::ApiError::bad_code(
            "missing_path",
            "missing path",
            json!({}),
        ));
    }
    let expanded = if raw == "~" || raw.starts_with("~/") {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| crate::api::ApiError::bad("HOME is unavailable"))?;
        if raw == "~" {
            home
        } else {
            home.join(&raw[2..])
        }
    } else {
        std::path::PathBuf::from(raw)
    };
    if !expanded.is_dir() {
        return Err(crate::api::ApiError::bad_code(
            "invalid_path",
            format!("path does not exist: {}", expanded.display()),
            json!({}),
        ));
    }
    let path = expanded
        .canonicalize()
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let scope = cccc_core::scope::detect(&path).map_err(|error| {
        crate::api::ApiError::bad_code("resolve_failed", error.to_string(), json!({}))
    })?;
    Ok(success(json!({
        "path": path,
        "scope_root": scope.url,
        "scope_key": scope.scope_key,
        "git_remote": scope.git_remote,
    })))
}
