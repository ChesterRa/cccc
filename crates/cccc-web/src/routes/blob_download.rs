use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, Method, header};
use tower_http::services::ServeFile;

use super::file_response::{content_disposition, sanitize_filename};
use crate::AppState;
use crate::api::ApiError;

#[derive(Default, serde::Deserialize)]
pub(super) struct BlobDownloadQuery {
    filename: Option<String>,
    download: Option<bool>,
}

pub(super) async fn download(
    State(state): State<AppState>,
    Path((group_id, blob_name)): Path<(String, String)>,
    Query(query): Query<BlobDownloadQuery>,
    mut request: Request,
) -> Result<axum::response::Response, ApiError> {
    let path = cccc_core::blobs::resolve(&state.home, &group_id, &blob_name)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let prefix = super::file_response::prefix(&path, 16)
        .await
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let filename = query
        .filename
        .as_deref()
        .map(sanitize_filename)
        .filter(|value| !value.is_empty());
    let content_type = blob_content_type(filename.as_deref().unwrap_or(&blob_name), &prefix);
    let disposition = query
        .download
        .unwrap_or(false)
        .then(|| content_disposition("attachment", filename.as_deref().unwrap_or("download")));
    // ServeFile answers byte ranges, which media players need to start and seek.
    // Serve the full representation where a range must be ignored: HEAD, an
    // If-Range (blobs expose no strong validator to match), or an empty blob.
    let length = tokio::fs::metadata(&path)
        .await
        .map_err(|error| ApiError::not_found(error.to_string()))?
        .len();
    if request.method() == Method::HEAD
        || request.headers().contains_key(header::IF_RANGE)
        || length == 0
        || covers_whole_file(request.headers().get(header::RANGE), length)
    {
        request.headers_mut().remove(header::RANGE);
    }
    let mime = content_type
        .parse()
        .unwrap_or(mime_guess::mime::APPLICATION_OCTET_STREAM);
    let mut response = ServeFile::new_with_mime(&path, &mime)
        .try_call(request)
        .await
        .map_err(|error| ApiError::not_found(error.to_string()))?
        .map(Body::new);
    // Group access can change while the content stays the same. Revalidate every
    // reuse so a cached copy is never shown without the current permission.
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache"),
    );
    if let Some(value) = disposition {
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("inline")),
        );
    }
    Ok(response)
}

/// A suffix range at least as long as the file asks for all of it, which
/// RFC 9110 satisfies with the whole representation; ServeFile refuses it.
fn covers_whole_file(range: Option<&HeaderValue>, length: u64) -> bool {
    range
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().strip_prefix("bytes=-"))
        .and_then(|suffix| suffix.trim().parse::<u64>().ok())
        .is_some_and(|suffix| suffix >= length)
}

fn blob_content_type(filename: &str, bytes: &[u8]) -> String {
    let guessed = mime_guess::from_path(filename).first_or_octet_stream();
    // Only trust the display filename for inert plain text. Raster images are
    // identified by signature below; active HTML/SVG content must not become
    // same-origin executable content merely because of a query parameter.
    if guessed == mime_guess::mime::TEXT_PLAIN {
        return guessed.essence_str().to_owned();
    }
    let detected = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        "image/avif"
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && &bytes[8..12] == b"qt  " {
        "video/quicktime"
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        // ISO base media (MP4, M4V); still images were matched above.
        "video/mp4"
    } else if bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        "video/webm"
    } else {
        "application/octet-stream"
    };
    detected.to_owned()
}

#[cfg(test)]
mod tests {
    use super::blob_content_type;

    #[test]
    fn uses_the_display_filename_for_text_and_binary_signature_for_images() {
        assert_eq!(blob_content_type("notes.txt", b"hello"), "text/plain");
        assert_eq!(
            blob_content_type("content-address", b"\x89PNG\r\n\x1a\nbody"),
            "image/png"
        );
        assert_eq!(
            blob_content_type("payload.html", b"<script>alert(1)</script>"),
            "application/octet-stream"
        );
        assert_eq!(
            blob_content_type("vector.svg", b"<svg onload='alert(1)'/>"),
            "application/octet-stream"
        );
        // Video is recognized by its container signature, never by the name.
        assert_eq!(
            blob_content_type("clip", b"\0\0\0\x20ftypisom\0\0\x02\0"),
            "video/mp4"
        );
        assert_eq!(
            blob_content_type("clip", b"\0\0\0\x14ftypqt  \0\0\0\0"),
            "video/quicktime"
        );
        assert_eq!(
            blob_content_type("clip", b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81"),
            "video/webm"
        );
        assert_eq!(
            blob_content_type("fake.mp4", b"<script>alert(1)</script>"),
            "application/octet-stream"
        );
    }
}
