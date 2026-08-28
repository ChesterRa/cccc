use std::fs;
use std::path::Path;
use std::time::Duration;

use reqwest::blocking::{Body, Client as HttpClient};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, COOKIE, ORIGIN, REFERER};
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::{BASE_HOST, BASE_URL, Client, LEGACY_BASE_HOST, Source, rpc};

const UPLOAD_PATH: &str = "/upload/_/";
const MAX_UPLOAD_BYTES: u64 = 200 * 1024 * 1024;

pub(crate) fn add_file(
    client: &Client,
    notebook_id: &str,
    raw_path: &Path,
    title: Option<&str>,
) -> Result<Source> {
    let path = raw_path
        .canonicalize()
        .map_err(|error| Error::Refused(format!("file is unavailable: {error}")))?;
    let metadata = fs::metadata(&path)
        .map_err(|error| Error::Refused(format!("file metadata is unavailable: {error}")))?;
    if !metadata.is_file() {
        return Err(Error::Refused(format!(
            "file source is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_UPLOAD_BYTES {
        return Err(Error::Refused(format!(
            "file source exceeds the 200 MiB NotebookLM limit: {}",
            path.display()
        )));
    }
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Refused("file source name is not valid UTF-8".into()))?;
    let content_type = content_type(&path);
    if matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("html" | "htm" | "xhtml")
    ) {
        return Err(Error::Refused(
            "NotebookLM does not accept HTML file uploads; convert the file to Markdown, text, or PDF".into(),
        ));
    }

    let baseline = client.list_sources(notebook_id).ok().map(|sources| {
        sources
            .into_iter()
            .map(|source| source.id)
            .collect::<std::collections::HashSet<_>>()
    });
    let source_id = register_file(client, notebook_id, filename, baseline.as_ref())?;
    let upload_url = start_upload(
        client,
        notebook_id,
        filename,
        metadata.len(),
        &source_id,
        &content_type,
    )?;
    finalize_upload(client, &upload_url, &path, metadata.len()).map_err(|error| {
        Error::Unresolved(format!(
            "file source {source_id} was registered, but byte upload did not complete: {error}; retrying the whole add may duplicate the source"
        ))
    })?;

    let desired_title = title.map(str::trim).filter(|value| !value.is_empty());
    let title = if let Some(desired_title) = desired_title
        && desired_title != filename
        && client
            .rename_source(notebook_id, &source_id, desired_title)
            .is_ok()
    {
        desired_title
    } else {
        filename
    };
    Ok(Source {
        id: source_id,
        title: Some(title.to_owned()),
        kind: source_kind(&path),
        status: "processing".into(),
        url: None,
        drive_document_id: None,
    })
}

fn register_file(
    client: &Client,
    notebook_id: &str,
    filename: &str,
    baseline: Option<&std::collections::HashSet<String>>,
) -> Result<String> {
    let result = client.rpc(
        rpc::ADD_SOURCE_FILE,
        json!([[[filename]], notebook_id, crate::template_block()]),
        &format!("/notebook/{notebook_id}"),
    );
    if let Ok(value) = &result
        && let Some(source_id) = extract_source_id(value, filename)
        && baseline.is_none_or(|ids| !ids.contains(&source_id))
    {
        return Ok(source_id);
    }
    let Some(baseline) = baseline else {
        return Err(Error::Unresolved(format!(
            "file source {filename:?} may have registered, but the pre-create baseline was unavailable; inspect the source list before retrying"
        )));
    };
    let candidates = client
        .list_sources(notebook_id)
        .map_err(|error| {
            Error::Unresolved(format!(
                "file source {filename:?} may have registered, but the recovery probe failed: {error}"
            ))
        })?
        .into_iter()
        .filter(|source| {
            !baseline.contains(&source.id) && source.title.as_deref() == Some(filename)
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [source] => Ok(source.id.clone()),
        [] => result.and_then(|value| {
            extract_source_id(&value, filename).ok_or_else(|| {
                Error::drift(
                    "file source registration",
                    "response did not contain a trustworthy source id",
                )
            })
        }),
        _ => Err(Error::Unresolved(format!(
            "file source {filename:?} registration is ambiguous: {} new matching sources exist",
            candidates.len()
        ))),
    }
}

fn start_upload(
    client: &Client,
    notebook_id: &str,
    filename: &str,
    file_size: u64,
    source_id: &str,
    content_type: &str,
) -> Result<String> {
    let authuser = client.auth.authuser.to_string();
    let cookie = client
        .cookie_header_for(BASE_HOST, UPLOAD_PATH)?
        .ok_or_else(|| Error::InvalidCredential("NotebookLM upload cookies are missing".into()))?;
    let http = upload_client()?;
    let response = http
        .post(format!("{BASE_URL}{UPLOAD_PATH}"))
        .query(&[("authuser", authuser.as_str())])
        .header(COOKIE, cookie)
        .header(ORIGIN, BASE_URL)
        .header(REFERER, format!("{BASE_URL}/"))
        .header("x-goog-authuser", &authuser)
        .header("x-goog-upload-command", "start")
        .header("x-goog-upload-header-content-length", file_size)
        .header("x-goog-upload-header-content-type", content_type)
        .header("x-goog-upload-protocol", "resumable")
        .header(
            CONTENT_TYPE,
            "application/x-www-form-urlencoded;charset=UTF-8",
        )
        .body(
            serde_json::to_string(&json!({
                "PROJECT_ID":notebook_id,
                "SOURCE_NAME":filename,
                "SOURCE_ID":source_id
            }))
            .map_err(|error| Error::drift("upload start body", error.to_string()))?,
        )
        .send()?;
    client.capture_cookies(&response)?;
    classify_upload_status(&response, filename)?;
    let raw = response
        .headers()
        .get("x-goog-upload-url")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| Error::drift("upload start", "x-goog-upload-url is absent"))?;
    validate_upload_url(raw).map(|url| url.to_string())
}

fn finalize_upload(client: &Client, raw_url: &str, path: &Path, file_size: u64) -> Result<()> {
    let url = validate_upload_url(raw_url)?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::Refused("upload URL has no host".into()))?;
    let origin = format!("https://{host}");
    let cookie = client
        .cookie_header_for(host, url.path())?
        .ok_or_else(|| Error::InvalidCredential("NotebookLM upload cookies are missing".into()))?;
    let file = fs::File::open(path)
        .map_err(|error| Error::Refused(format!("failed to open upload source: {error}")))?;
    let response = upload_client()?
        .post(url)
        .header(COOKIE, cookie)
        .header(ORIGIN, &origin)
        .header(REFERER, format!("{origin}/"))
        .header("x-goog-authuser", client.auth.authuser.to_string())
        .header("x-goog-upload-command", "upload, finalize")
        .header("x-goog-upload-offset", "0")
        .header(
            CONTENT_TYPE,
            "application/x-www-form-urlencoded;charset=utf-8",
        )
        .header(CONTENT_LENGTH, file_size)
        .body(Body::new(file))
        .send()?;
    client.capture_cookies(&response)?;
    classify_upload_status(&response, &path.display().to_string())
}

fn upload_client() -> Result<HttpClient> {
    HttpClient::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 CCCC NotebookLM Rust file uploader")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(Error::from)
}

fn classify_upload_status(response: &reqwest::blocking::Response, name: &str) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    match status.as_u16() {
        300..=399 | 401 | 403 => Err(Error::Authentication),
        429 => Err(Error::RateLimited(format!(
            "upload of {name:?} returned HTTP 429"
        ))),
        400..=499 => Err(Error::Refused(format!(
            "NotebookLM rejected upload of {name:?} with HTTP {status}"
        ))),
        _ => Err(Error::Rpc {
            rpc_id: "upload".into(),
            message: format!("upload of {name:?} returned HTTP {status}"),
        }),
    }
}

fn validate_upload_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw)
        .map_err(|error| Error::Refused(format!("invalid upload URL: {error}")))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(url.host_str(), Some(BASE_HOST | LEGACY_BASE_HOST))
        || normalized_path(url.path()) != normalized_path(UPLOAD_PATH)
    {
        return Err(Error::Refused(
            "NotebookLM upload URL is not trusted".into(),
        ));
    }
    let upload_ids = url
        .query_pairs()
        .filter(|(key, _)| key.eq_ignore_ascii_case("upload_id"))
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    if upload_ids.len() != 1 || upload_ids.first().is_none_or(String::is_empty) {
        return Err(Error::Refused(
            "NotebookLM upload URL must contain exactly one upload_id".into(),
        ));
    }
    Ok(url)
}

fn normalized_path(path: &str) -> String {
    format!("{}/", path.trim_end_matches('/'))
}

fn content_type(path: &Path) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown") => "text/markdown".into(),
        _ => mime_guess::from_path(path)
            .first_or_octet_stream()
            .essence_str()
            .to_owned(),
    }
}

fn source_kind(path: &Path) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown") => "markdown",
        Some("epub") => "epub",
        Some("pdf") => "pdf",
        Some("doc" | "docx" | "odt" | "rtf") => "docx",
        Some("csv" | "tsv" | "xls" | "xlsx" | "ods") => "csv",
        Some(
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "heic" | "heif",
        ) => "image",
        Some(
            "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "mp4" | "m4v" | "mov" | "avi"
            | "mkv" | "webm",
        ) => "media",
        _ => "unknown",
    }
    .into()
}

fn extract_source_id(value: &Value, filename: &str) -> Option<String> {
    let mut candidates = Vec::new();
    collect_explicit_id_candidates(value, filename, 0, true, &mut candidates);
    candidates.sort();
    candidates.dedup();
    match candidates.len() {
        1 => return candidates.pop(),
        2.. => return None,
        _ => {}
    }

    collect_contextual_id_candidates(value, filename, 0, &mut candidates);
    candidates.sort();
    candidates.dedup();
    match candidates.len() {
        1 => return candidates.pop(),
        2.. => return None,
        _ => {}
    }

    if let Value::Array(items) = value
        && let [Value::Null, inner] = items.as_slice()
        && let Some(candidate) = singleton_id(inner, filename)
    {
        return Some(candidate);
    }
    singleton_id(value, filename)
}

fn collect_explicit_id_candidates(
    value: &Value,
    filename: &str,
    depth: usize,
    root: bool,
    out: &mut Vec<String>,
) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Object(fields) => {
            let context_names = fields
                .iter()
                .filter(|(key, _)| {
                    matches!(
                        key.as_str(),
                        "SOURCE_NAME"
                            | "source_name"
                            | "sourceName"
                            | "filename"
                            | "fileName"
                            | "name"
                            | "title"
                    )
                })
                .filter_map(|(_, value)| singleton_string(value))
                .collect::<Vec<_>>();
            let context_matches = context_names.iter().any(|name| name == filename);
            let context_mismatches = !context_names.is_empty() && !context_matches;
            for (key, value) in fields {
                let explicit = matches!(key.as_str(), "SOURCE_ID" | "source_id" | "sourceId")
                    && !context_mismatches
                    && (root || context_matches);
                let contextual = key == "id" && context_matches;
                if (explicit || contextual)
                    && let Some(candidate) = source_id_candidate(value, filename)
                {
                    out.push(candidate);
                }
                collect_explicit_id_candidates(value, filename, depth + 1, false, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_explicit_id_candidates(item, filename, depth + 1, false, out);
            }
        }
        _ => {}
    }
}

fn collect_contextual_id_candidates(
    value: &Value,
    filename: &str,
    depth: usize,
    out: &mut Vec<String>,
) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Array(items) => {
            if items.len() >= 2 {
                if singleton_string(&items[1]).as_deref() == Some(filename)
                    && let Some(candidate) = source_id_candidate(&items[0], filename)
                {
                    out.push(candidate);
                }
                if singleton_string(&items[0]).as_deref() == Some(filename)
                    && let Some(candidate) = source_id_candidate(&items[1], filename)
                {
                    out.push(candidate);
                }
            }
            for item in items {
                collect_contextual_id_candidates(item, filename, depth + 1, out);
            }
        }
        Value::Object(fields) => {
            for item in fields.values() {
                collect_contextual_id_candidates(item, filename, depth + 1, out);
            }
        }
        _ => {}
    }
}

fn singleton_id(value: &Value, filename: &str) -> Option<String> {
    let mut value = value;
    let mut depth = 0;
    while let Value::Array(items) = value
        && let [item] = items.as_slice()
        && depth < 8
    {
        value = item;
        depth += 1;
    }
    (depth > 0)
        .then(|| source_id_candidate(value, filename))
        .flatten()
}

fn singleton_string(value: &Value) -> Option<String> {
    let mut value = value;
    let mut depth = 0;
    while let Value::Array(items) = value
        && let [item] = items.as_slice()
        && depth < 8
    {
        value = item;
        depth += 1;
    }
    value.as_str().map(str::trim).map(str::to_owned)
}

fn source_id_candidate(value: &Value, filename: &str) -> Option<String> {
    let candidate = singleton_string(value)?;
    if candidate.is_empty()
        || candidate == filename
        || candidate.len() > 1_000
        || candidate
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '/' | '\\'))
        || candidate.len() < 4
        || !candidate
            .chars()
            .any(|character| character.is_ascii_digit() || matches!(character, '-' | '_'))
    {
        return None;
    }
    Some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_urls_are_origin_and_path_bound() {
        for raw in [
            "https://notebook.google.com/upload/_/?upload_id=abc",
            "https://notebooklm.google.com/upload/_/?upload_id=abc",
        ] {
            assert!(validate_upload_url(raw).is_ok(), "{raw}");
        }
        for raw in [
            "http://notebook.google.com/upload/_/?upload_id=abc",
            "https://evil.test/upload/_/?upload_id=abc",
            "https://notebook.google.com/other/?upload_id=abc",
            "https://notebook.google.com/upload/_/",
            "https://notebook.google.com/upload/_/?upload_id=a&upload_id=b",
            "https://notebook.google.com:444/upload/_/?upload_id=abc",
        ] {
            assert!(validate_upload_url(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn file_registration_accepts_only_one_trustworthy_id() {
        assert_eq!(
            extract_source_id(&json!([["source_123"]]), "notes.md").as_deref(),
            Some("source_123")
        );
        assert_eq!(
            extract_source_id(
                &json!({"SOURCE_ID":"source_123","other":{"source_id":"source_456"}}),
                "notes.md"
            )
            .as_deref(),
            Some("source_123")
        );
        assert_eq!(
            extract_source_id(
                &json!({"SOURCE_ID":"source_123","source_id":"source_456"}),
                "notes.md"
            ),
            None
        );
        assert_eq!(extract_source_id(&json!([["notes.md"]]), "notes.md"), None);
        assert_eq!(extract_source_id(&json!([["owner"]]), "notes.md"), None);
        assert_eq!(
            extract_source_id(&json!([["unrelated_1"], ["source_123"]]), "notes.md"),
            None
        );
        assert_eq!(
            extract_source_id(&json!([["source_123", "notes.md"]]), "notes.md").as_deref(),
            Some("source_123")
        );
        assert_eq!(
            extract_source_id(
                &json!({"source_id":"source_123","source_name":"other.md"}),
                "notes.md"
            ),
            None
        );
    }

    #[test]
    fn markdown_content_type_is_host_independent() {
        assert_eq!(content_type(Path::new("notes.md")), "text/markdown");
    }
}
