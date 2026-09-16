use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Response, header};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use std::io;
use std::path::Path;
use tokio::io::AsyncReadExt;

const STREAM_CHUNK_BYTES: usize = 64 * 1024;
const MAX_DOWNLOAD_FILENAME_CHARS: usize = 180;

/// Reduce a caller-supplied name to a safe leaf: no directories, no control
/// characters, no quotes that would break out of the header parameter.
pub fn sanitize_filename(raw: &str) -> String {
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

/// Build an RFC 6266 disposition for `kind` ("attachment" or "inline").
///
/// Header values travel as opaque bytes and browsers decode them as
/// ISO-8859-1, so a non-ASCII name must ride in the percent-encoded
/// `filename*` parameter; `filename=` stays ASCII-only as the fallback.
pub fn content_disposition(kind: &str, filename: &str) -> String {
    let filename = sanitize_filename(filename);
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
    format!("{kind}; filename=\"{ascii_fallback}\"; filename*=UTF-8''{encoded}")
}

pub async fn stream(
    path: &Path,
    content_type: &str,
    cache_control: Option<&'static str>,
    disposition: Option<&str>,
) -> io::Result<Response<Body>> {
    let mut file = tokio::fs::File::open(path).await?;
    let length = file.metadata().await?.len();
    let chunks = async_stream::stream! {
        let mut buffer = vec![0_u8; STREAM_CHUNK_BYTES];
        loop {
            match file.read(&mut buffer).await {
                Ok(0) => break,
                Ok(count) => yield Ok::<Bytes, io::Error>(Bytes::copy_from_slice(&buffer[..count])),
                Err(error) => {
                    yield Err(error);
                    break;
                }
            }
        }
    };
    let mut response = Response::new(Body::from_stream(chunks));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).expect("file length is a valid header"),
    );
    if let Some(value) = cache_control {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    }
    if let Some(value) = disposition {
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static("inline")),
        );
    }
    Ok(response)
}

pub async fn prefix(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut bytes = vec![0_u8; limit];
    let count = file.read(&mut bytes).await?;
    bytes.truncate(count);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{content_disposition, sanitize_filename, stream};
    use axum::body::to_bytes;
    use axum::http::header;

    #[test]
    fn keeps_only_a_safe_leaf_filename() {
        assert_eq!(
            sanitize_filename("../folder\\report\r\n\"final\".txt"),
            "report_final_.txt"
        );
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename("\r\n"), "download");
    }

    #[test]
    fn emits_ascii_and_utf8_content_disposition_names() {
        let disposition = content_disposition("attachment", "分析 报告.txt");
        assert!(disposition.starts_with("attachment; filename=\"__ __.txt\""));
        assert!(disposition.contains("filename*=UTF-8''"));
        assert!(disposition.contains("%E5%88%86%E6%9E%90"));
        assert!(!disposition.contains(['\r', '\n']));
    }

    #[test]
    fn percent_encodes_every_non_ascii_byte_of_a_chinese_group_package() {
        let disposition = content_disposition(
            "attachment",
            "cccc-group--安卓新设备平台--g_0e88539bb583.zip",
        );
        // A raw UTF-8 byte in the header is what browsers mis-render as
        // ISO-8859-1 mojibake, so none may survive encoding.
        assert!(disposition.is_ascii(), "{disposition}");
        assert!(disposition.contains("filename*=UTF-8''cccc%2Dgroup%2D%2D%E5%AE%89"));
    }

    #[test]
    fn inline_disposition_shares_the_same_encoding_rules() {
        let disposition = content_disposition("inline", "封面 图.png");
        assert!(disposition.starts_with("inline; filename=\"__ _.png\""));
        assert!(disposition.contains("filename*=UTF-8''%E5%B0%81%E9%9D%A2"));
    }

    #[tokio::test]
    async fn streams_file_with_length_and_headers() {
        let file = tempfile::NamedTempFile::new().expect("tempfile");
        let content = vec![7_u8; 128 * 1024 + 3];
        std::fs::write(file.path(), &content).expect("write");
        let response = stream(
            file.path(),
            "application/test",
            Some("no-store"),
            Some("attachment; filename=\"test.bin\""),
        )
        .await
        .expect("response");
        assert_eq!(
            response.headers()[header::CONTENT_LENGTH],
            content.len().to_string()
        );
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/test");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            to_bytes(response.into_body(), content.len() + 1)
                .await
                .expect("body"),
            content
        );
    }
}
