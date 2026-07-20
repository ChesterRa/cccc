use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Response, header};
use std::io;
use std::path::Path;
use tokio::io::AsyncReadExt;

const STREAM_CHUNK_BYTES: usize = 64 * 1024;

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
    use super::stream;
    use axum::body::to_bytes;
    use axum::http::header;

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
