use axum::extract::{Path, Query, State};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

use crate::AppState;
use crate::api::ApiError;

const MAX_DOWNLOAD_FILENAME_CHARS: usize = 180;

#[derive(Default, serde::Deserialize)]
pub(super) struct BlobDownloadQuery {
    filename: Option<String>,
    download: Option<bool>,
}

pub(super) async fn download(
    State(state): State<AppState>,
    Path((group_id, blob_name)): Path<(String, String)>,
    Query(query): Query<BlobDownloadQuery>,
) -> Result<axum::response::Response, ApiError> {
    let path = cccc_core::blobs::resolve(&state.home, &group_id, &blob_name)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let prefix = super::file_response::prefix(&path, 16)
        .await
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let filename = query
        .filename
        .as_deref()
        .map(sanitize_download_filename)
        .filter(|value| !value.is_empty());
    let content_type = blob_content_type(filename.as_deref().unwrap_or(&blob_name), &prefix);
    let disposition = query
        .download
        .unwrap_or(false)
        .then(|| attachment_disposition(filename.as_deref().unwrap_or("download")));
    super::file_response::stream(&path, &content_type, None, disposition.as_deref())
        .await
        .map_err(|error| ApiError::not_found(error.to_string()))
}

fn sanitize_download_filename(raw: &str) -> String {
    let leaf = raw.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let cleaned = leaf
        .chars()
        .filter_map(|character| {
            if character.is_control() {
                None
            } else if character == '"' {
                Some('_')
            } else {
                Some(character)
            }
        })
        .take(MAX_DOWNLOAD_FILENAME_CHARS)
        .collect::<String>();
    if cleaned.is_empty() || matches!(cleaned.as_str(), "." | "..") {
        "download".to_owned()
    } else {
        cleaned
    }
}

fn attachment_disposition(filename: &str) -> String {
    let filename = sanitize_download_filename(filename);
    let ascii_fallback = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || matches!(character, '.' | '-' | '_' | ' ' | '(' | ')')
            {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let encoded = utf8_percent_encode(&filename, NON_ALPHANUMERIC);
    format!("attachment; filename=\"{ascii_fallback}\"; filename*=UTF-8''{encoded}")
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
    } else {
        "application/octet-stream"
    };
    detected.to_owned()
}

#[cfg(test)]
mod tests {
    use super::{attachment_disposition, blob_content_type, sanitize_download_filename};

    #[test]
    fn keeps_only_a_safe_leaf_filename() {
        assert_eq!(
            sanitize_download_filename("../folder\\report\r\n\"final\".txt"),
            "report_final_.txt"
        );
        assert_eq!(sanitize_download_filename(".."), "download");
        assert_eq!(sanitize_download_filename("\r\n"), "download");
    }

    #[test]
    fn emits_ascii_and_utf8_content_disposition_names() {
        let disposition = attachment_disposition("分析 报告.txt");
        assert!(disposition.starts_with("attachment; filename=\"__ __.txt\""));
        assert!(disposition.contains("filename*=UTF-8''"));
        assert!(disposition.contains("%E5%88%86%E6%9E%90"));
        assert!(!disposition.contains(['\r', '\n']));
    }

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
    }
}
