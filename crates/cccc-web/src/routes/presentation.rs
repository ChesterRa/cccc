use axum::extract::{Multipart, Path, Query, State};
use axum::http::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_core::GroupStore;
use cccc_core::presentation;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/presentation",
            get(get_presentation),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/publish",
            post(publish),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/publish_upload",
            post(publish_upload),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/publish_workspace",
            post(publish_workspace),
        )
        .route("/api/v1/groups/{group_id}/presentation/clear", post(clear))
        .route(
            "/api/v1/groups/{group_id}/presentation/workspace/list",
            get(workspace_list),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/slots/{slot_id}/asset",
            get(asset),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/ref_snapshot",
            post(reference_snapshot),
        )
}

async fn get_presentation(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> ApiResult {
    call(
        &state,
        "presentation_get",
        object(json!({"group_id":group_id})),
    )
    .await
}

async fn publish(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id.clone()));
    if args
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|url| !url.trim().is_empty())
        && !args.contains_key("card_type")
    {
        args.insert("card_type".into(), Value::String("web_preview".into()));
    }
    publish_and_cleanup(&state, &group_id, args).await
}

async fn publish_workspace(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id.clone()));
    publish_and_cleanup(&state, &group_id, args).await
}

async fn clear(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id.clone()));
    let mut response = call(&state, "presentation_clear", args).await?;
    let cleared_slots: Vec<String> = response
        .0
        .pointer("/result/cleared_slots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    for slot in cleared_slots {
        close_browser_surface(&state, &group_id, &slot, &mut response).await;
    }
    Ok(response)
}

async fn publish_upload(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    multipart: Multipart,
) -> ApiResult {
    let upload = parse_upload(multipart).await?;
    let file_name = upload
        .file_name
        .ok_or_else(|| ApiError::bad("file is required"))?;
    let data = upload
        .data
        .ok_or_else(|| ApiError::bad("file is required"))?;
    let mime = upload.mime.unwrap_or_else(|| {
        mime_guess::from_path(&file_name)
            .first_or_octet_stream()
            .to_string()
    });
    let card_type = card_type_for(&file_name, &mime);
    let mut args = upload.fields;
    args.insert("group_id".into(), Value::String(group_id.clone()));
    args.entry("title")
        .or_insert_with(|| Value::String(file_name.clone()));
    args.insert("card_type".into(), Value::String(card_type.into()));
    args.insert("source_label".into(), Value::String(file_name.clone()));
    if card_type == "markdown" {
        let content =
            String::from_utf8(data).map_err(|_| ApiError::bad("markdown upload must be UTF-8"))?;
        args.insert("content".into(), Value::String(content));
    } else {
        let blob = cccc_core::blobs::store(&state.home, &group_id, &data)
            .map_err(|error| ApiError::bad(error.to_string()))?;
        args.insert("blob_rel_path".into(), Value::String(blob.path));
    }
    publish_and_cleanup(&state, &group_id, args).await
}

async fn publish_and_cleanup(
    state: &AppState,
    group_id: &str,
    args: Map<String, Value>,
) -> ApiResult {
    let mut response = call(state, "presentation_publish", args).await?;
    if let Some(slot) = replaced_slot(&response) {
        close_browser_surface(state, group_id, &slot, &mut response).await;
    }
    Ok(response)
}

fn replaced_slot(response: &Json<Value>) -> Option<String> {
    response
        .0
        .pointer("/result/replaced")
        .and_then(Value::as_bool)
        .filter(|replaced| *replaced)
        .and_then(|_| response.0.pointer("/result/slot_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|slot| !slot.is_empty())
        .map(str::to_owned)
}

async fn close_browser_surface(
    state: &AppState,
    group_id: &str,
    slot: &str,
    response: &mut Json<Value>,
) {
    if let Err(error) = state
        .browser_surfaces
        .close(&format!("{group_id}::{slot}"))
        .await
    {
        response.0["result"]["browser_cleanup_warning"] = Value::String(error.to_string());
    }
}

async fn workspace_list(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let store =
        GroupStore::new(state.home.clone()).map_err(|error| ApiError::bad(error.to_string()))?;
    let group = store
        .load(&group_id)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let (root, path, parent, items) =
        presentation::list_workspace(&group, query.get("path").map(String::as_str).unwrap_or(""))
            .map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(Json(json!({"ok":true,"result":{
        "root_path":root,
        "path":path,
        "parent":parent,
        "items":items
    }})))
}

async fn asset(
    State(state): State<AppState>,
    Path((group_id, slot_id)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response<axum::body::Body>, ApiError> {
    let store =
        GroupStore::new(state.home.clone()).map_err(|error| ApiError::bad(error.to_string()))?;
    let (path, mime, file_name) = presentation::asset_path(&store, &group_id, &slot_id)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let disposition = if query
        .get("download")
        .is_some_and(|value| value == "1" || value == "true")
    {
        "attachment"
    } else {
        "inline"
    };
    let safe_name = file_name.replace(['\r', '\n', '"'], "_");
    super::file_response::stream(
        &path,
        &mime,
        Some("no-store"),
        Some(&format!("{disposition}; filename=\"{safe_name}\"")),
    )
    .await
    .map_err(|error| ApiError::not_found(error.to_string()))
}

async fn reference_snapshot(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    multipart: Multipart,
) -> ApiResult {
    let upload = parse_upload(multipart).await?;
    let data = upload
        .data
        .ok_or_else(|| ApiError::bad("file is required"))?;
    let blob = cccc_core::blobs::store(&state.home, &group_id, &data)
        .map_err(|error| ApiError::bad(error.to_string()))?;
    let field = |name: &str| {
        upload
            .fields
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let number = |name: &str| field(name).parse::<u64>().unwrap_or_default();
    Ok(Json(json!({"ok":true,"result":{
        "group_id":group_id,
        "snapshot":{
            "path":blob.path,
            "mime_type":upload.mime.unwrap_or_else(||"application/octet-stream".into()),
            "bytes":blob.bytes,
            "sha256":blob.sha256,
            "width":number("width"),
            "height":number("height"),
            "captured_at":field("captured_at"),
            "source":field("source")
        }
    }})))
}

struct Upload {
    fields: Map<String, Value>,
    file_name: Option<String>,
    mime: Option<String>,
    data: Option<Vec<u8>>,
}

async fn parse_upload(mut multipart: Multipart) -> Result<Upload, ApiError> {
    let mut upload = Upload {
        fields: Map::new(),
        file_name: None,
        mime: None,
        data: None,
    };
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?
    {
        let name = field.name().unwrap_or("").to_owned();
        if name == "file" {
            upload.file_name = Some(field.file_name().unwrap_or("upload.bin").to_owned());
            upload.mime = field.content_type().map(str::to_owned);
            upload.data = Some(
                field
                    .bytes()
                    .await
                    .map_err(|error| ApiError::bad(error.to_string()))?
                    .to_vec(),
            );
        } else {
            upload.fields.insert(
                name,
                Value::String(
                    field
                        .text()
                        .await
                        .map_err(|error| ApiError::bad(error.to_string()))?,
                ),
            );
        }
    }
    Ok(upload)
}

fn card_type_for(file_name: &str, mime: &str) -> &'static str {
    let extension = std::path::Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "md" | "markdown") || mime == "text/markdown" {
        "markdown"
    } else if mime.starts_with("image/") {
        "image"
    } else if extension == "pdf" || mime == "application/pdf" {
        "pdf"
    } else if matches!(extension.as_str(), "html" | "htm") || mime == "text/html" {
        "web_preview"
    } else {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_replaced_publications_invalidate_browser_sessions() {
        let replaced = Json(json!({"ok":true,"result":{"replaced":true,"slot_id":"slot-2"}}));
        let first_publish = Json(json!({"ok":true,"result":{"replaced":false,"slot_id":"slot-2"}}));

        assert_eq!(replaced_slot(&replaced).as_deref(), Some("slot-2"));
        assert_eq!(replaced_slot(&first_publish), None);
    }
}
