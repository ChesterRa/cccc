use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiResult, call, object, success};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ping", get(ping))
        .route("/api/v1/health", get(health))
        .route("/api/v1/ready", get(health))
        .route("/api/v1/runtimes", get(runtimes))
        .route(
            "/api/v1/observability",
            get(observability_get).put(observability_update),
        )
        .route("/api/v1/branding", get(branding_get).put(branding_update))
        .route(
            "/api/v1/branding/assets/{asset_kind}",
            get(branding_asset_get)
                .post(branding_asset_upload)
                .delete(branding_asset_delete),
        )
        .route("/api/v1/registry/reconcile", get(reconcile).post(reconcile))
        .route("/api/v1/fs/list", get(fs_list))
        .route("/api/v1/fs/recent", get(fs_recent))
        .route("/api/v1/fs/scope_root", get(fs_scope_root))
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024))
}

async fn ping(State(state): State<AppState>) -> ApiResult {
    call(&state, "ping", Default::default()).await
}
async fn health(State(state): State<AppState>) -> ApiResult {
    let mut response = call(&state, "ping", Default::default()).await?;
    response
        .0
        .get_mut("result")
        .and_then(Value::as_object_mut)
        .map(|value| value.insert("status".into(), Value::String("ok".into())));
    Ok(response)
}
async fn runtimes() -> Json<Value> {
    let runtimes = cccc_runtime::detect_runtimes();
    let available = runtimes
        .iter()
        .filter(|runtime| runtime.available)
        .map(|runtime| runtime.name.clone())
        .collect::<Vec<_>>();
    success(json!({"available":available,"runtimes":runtimes}))
}
async fn observability_get(State(state): State<AppState>) -> ApiResult {
    call(&state, "observability_get", Default::default()).await
}
async fn observability_update(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    call(
        &state,
        "observability_update",
        object(json!({"patch":body})),
    )
    .await
}
async fn branding_get(State(state): State<AppState>) -> ApiResult {
    let response = call(&state, "branding_get", Default::default()).await?;
    let raw = response.0["result"]["branding"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    Ok(success(
        json!({"branding":cccc_core::branding::payload(&raw)}),
    ))
}
async fn branding_update(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let before = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let mut patch = serde_json::Map::new();
    if let Some(value) = body.get("product_name") {
        patch.insert("product_name".into(), value.clone());
    }
    for (flag, key) in [
        ("clear_logo_icon", "logo_icon_asset_path"),
        ("clear_favicon", "favicon_asset_path"),
    ] {
        if body.get(flag).and_then(Value::as_bool).unwrap_or(false) {
            if let Some(relative) = before.branding.get(key).and_then(Value::as_str) {
                let _ = cccc_core::branding::delete(&state.home, relative);
            }
            patch.insert(key.into(), Value::String(String::new()));
        }
    }
    let response = call(
        &state,
        "branding_update",
        object(json!({"by":"user","patch":patch})),
    )
    .await?;
    let raw = response.0["result"]["branding"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    Ok(success(
        json!({"branding":cccc_core::branding::payload(&raw)}),
    ))
}

async fn branding_asset_get(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> Result<Response<axum::body::Body>, crate::api::ApiError> {
    let global = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let relative = cccc_core::branding::asset_relative(&global.branding, &kind)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    if relative.is_empty() {
        return Err(crate::api::ApiError::not_found(
            "custom branding asset not found",
        ));
    }
    let path = cccc_core::branding::resolve(&state.home, &relative)
        .map_err(|error| crate::api::ApiError::not_found(error.to_string()))?;
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    super::file_response::stream(&path, &mime, Some("no-cache"), None)
        .await
        .map_err(|error| crate::api::ApiError::not_found(error.to_string()))
}

async fn branding_asset_upload(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    mut multipart: Multipart,
) -> ApiResult {
    let mut bytes = None;
    let mut mime = String::new();
    let mut filename = String::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?
    {
        if field.name() == Some("file") {
            mime = field.content_type().unwrap_or("").into();
            filename = field.file_name().unwrap_or("asset").into();
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|error| crate::api::ApiError::bad(error.to_string()))?
                    .to_vec(),
            );
        }
    }
    let stored = cccc_core::branding::store(
        &state.home,
        &kind,
        &bytes.ok_or_else(|| crate::api::ApiError::bad("file is required"))?,
        &mime,
        &filename,
    )
    .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let before = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let key = format!("{kind}_asset_path");
    let old = before
        .branding
        .get(&key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut patch = serde_json::Map::new();
    patch.insert(key, Value::String(stored.rel_path));
    let response = call(
        &state,
        "branding_update",
        object(json!({"by":"user","patch":patch})),
    )
    .await?;
    if !old.is_empty() {
        let _ = cccc_core::branding::delete(&state.home, &old);
    }
    let raw = response.0["result"]["branding"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    Ok(success(
        json!({"branding":cccc_core::branding::payload(&raw)}),
    ))
}

async fn branding_asset_delete(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> ApiResult {
    let global = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let relative = cccc_core::branding::asset_relative(&global.branding, &kind)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    cccc_core::branding::delete(&state.home, &relative)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let key = format!("{kind}_asset_path");
    let mut patch = serde_json::Map::new();
    patch.insert(key, Value::String(String::new()));
    let response = call(
        &state,
        "branding_update",
        object(json!({"by":"user","patch":patch})),
    )
    .await?;
    let raw = response.0["result"]["branding"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    Ok(success(
        json!({"branding":cccc_core::branding::payload(&raw)}),
    ))
}
async fn reconcile(State(state): State<AppState>) -> ApiResult {
    call(&state, "registry_reconcile", Default::default()).await
}
async fn fs_list(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
    let path = query
        .get("path")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let entries: Vec<_> = std::fs::read_dir(&path).into_iter().flatten().filter_map(Result::ok).take(500).map(|entry| json!({"name":entry.file_name(),"path":entry.path(),"is_dir":entry.path().is_dir()})).collect();
    success(json!({"path":path,"entries":entries,"suggestions":entries}))
}
async fn fs_recent() -> Json<Value> {
    success(json!({"suggestions":[]}))
}
async fn fs_scope_root(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
    success(json!({"path":query.get("path"),"scope_root":query.get("path")}))
}
