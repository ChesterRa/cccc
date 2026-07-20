use axum::body::Body;
use axum::http::{HeaderMap, header};
use cccc_core::HomeLayout;
use futures_util::StreamExt;
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

use super::voice_asr;
use crate::api::ApiError;

static UPLOAD_WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(super) fn validate_content_length(headers: &HeaderMap) -> Result<(), ApiError> {
    if headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|bytes| bytes > voice_asr::MAX_AUDIO_BYTES)
    {
        return Err(too_large());
    }
    Ok(())
}

pub(super) async fn receive(
    home: &HomeLayout,
    body: Body,
) -> Result<tempfile::NamedTempFile, ApiError> {
    receive_with_limit(home, body, voice_asr::MAX_AUDIO_BYTES).await
}

async fn receive_with_limit(
    home: &HomeLayout,
    body: Body,
    limit: usize,
) -> Result<tempfile::NamedTempFile, ApiError> {
    let _permit = UPLOAD_WORKERS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::unavailable("audio_upload_busy", "audio upload capacity is exhausted")
        })?;
    let temp_dir = home.root().join("cache/voice-http-uploads");
    std::fs::create_dir_all(&temp_dir).map_err(write_error)?;
    let audio_file = tempfile::NamedTempFile::new_in(temp_dir).map_err(write_error)?;
    let output = audio_file.reopen().map_err(write_error)?;
    let mut output = tokio::fs::File::from_std(output);
    let mut received = 0_usize;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            ApiError::bad_code("invalid_audio_stream", error.to_string(), json!({}))
        })?;
        received = received.saturating_add(chunk.len());
        if received > limit {
            return Err(too_large());
        }
        output.write_all(&chunk).await.map_err(write_error)?;
    }
    output.flush().await.map_err(write_error)?;
    Ok(audio_file)
}

fn too_large() -> ApiError {
    ApiError::payload_too_large("audio_too_large", "audio payload exceeds 100 MiB")
}

fn write_error(error: std::io::Error) -> ApiError {
    ApiError::bad_code("audio_write_failed", error.to_string(), json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn streams_to_an_auto_deleted_file_and_enforces_the_limit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let upload = receive_with_limit(&home, Body::from(vec![1_u8, 2, 3]), 3)
            .await
            .expect("upload");
        assert_eq!(
            tokio::fs::read(upload.path()).await.expect("read"),
            [1, 2, 3]
        );
        let path = upload.path().to_owned();
        drop(upload);
        assert!(!path.exists());

        let error = receive_with_limit(&home, Body::from(vec![1_u8, 2, 3, 4]), 3)
            .await
            .expect_err("oversized");
        assert!(error.to_string().contains("audio_too_large"));
    }
}
