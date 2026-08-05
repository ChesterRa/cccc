use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::HomeLayout;
use tower::ServiceExt;

#[tokio::test]
async fn transcription_accepts_binary_bodies_above_axum_default_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let audio = vec![0_u8; 3 * 1024 * 1024];

    let response = cccc_web::app(home)
        .oneshot(
            Request::post("/api/v1/groups/missing/assistants/voice_secretary/transcriptions")
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .body(Body::from(audio))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_ne!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn transcription_rejects_declared_audio_above_the_recording_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");

    let response = cccc_web::app(home)
        .oneshot(
            Request::post("/api/v1/groups/missing/assistants/voice_secretary/transcriptions")
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, 100 * 1024 * 1024 + 1)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
