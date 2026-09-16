//! Raw workspace bytes for media previews and downloads. Scope and authorization
//! match the text endpoint; tower-http owns streaming, HEAD and byte ranges.

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, header};
use axum::response::Response;
use cccc_core::workspace;
use serde::Deserialize;
use tower_http::services::ServeFile;

use crate::{AppState, api::ApiError};

#[derive(Deserialize)]
pub(super) struct ContentQuery {
    scope_key: String,
    scope_url: String,
    path: String,
    #[serde(default)]
    download: bool,
}

pub(super) async fn read(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<ContentQuery>,
    mut request: Request,
) -> Result<Response, ApiError> {
    let download = query.download;
    let path = tokio::task::spawn_blocking(move || {
        let group = super::load_group(&state, &group_id, &query.scope_key, &query.scope_url)?;
        workspace::resolve_file(&group, &query.path)
            .map_err(|error| super::path_error(&query.path, error))
    })
    .await
    .map_err(|error| ApiError::unavailable("workspace_unavailable", error.to_string()))??;

    let guessed = mime_guess::from_path(&path).first_or_octet_stream();
    let pdf = guessed == mime_guess::mime::APPLICATION_PDF;
    let preview =
        !download && (pdf || matches!(guessed.type_().as_str(), "image" | "video" | "audio"));
    // Never serve workspace HTML or other active documents as a same-origin page.
    // SVG renders as an image; sandbox also protects direct navigation to its URL.
    let mime = if preview {
        guessed
    } else {
        mime_guess::mime::APPLICATION_OCTET_STREAM
    };
    // ServeFile does not evaluate If-Range. Workspace files are mutable and we
    // expose no strong validator, so return the current full representation
    // instead of combining a client's older bytes with a new partial response.
    if request.headers().contains_key(header::IF_RANGE) {
        request.headers_mut().remove(header::RANGE);
    }
    let mut response = ServeFile::new_with_mime(&path, &mime)
        .try_call(request)
        .await
        .map_err(|error| super::path_error(&path.to_string_lossy(), error))?
        .map(Body::new);
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        // Native PDF viewers require plugin/document support, which CSP sandbox blocks.
        // PDF is served with its exact MIME and nosniff, never interpreted as HTML.
        HeaderValue::from_static(if pdf && preview {
            "default-src 'none'; object-src 'self'; style-src 'unsafe-inline'"
        } else {
            "sandbox; default-src 'none'; style-src 'unsafe-inline'"
        }),
    );
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let disposition = super::super::file_response::content_disposition("attachment", &name);
    let disposition = if preview {
        disposition.replacen("attachment;", "inline;", 1)
    } else {
        disposition
    };
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).expect("encoded filename is a valid header"),
    );
    Ok(response)
}
