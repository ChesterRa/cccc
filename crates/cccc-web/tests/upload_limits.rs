mod auth_support;

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use cccc_core::{GroupStore, HomeLayout, branding};
use http_body_util::BodyExt;
use serde_json::Value;
use std::convert::Infallible;
use tower::ServiceExt;

const ONE_MIB: usize = 1024 * 1024;
static FILL_MIB: [u8; ONE_MIB] = [b'x'; ONE_MIB];

fn streamed_file_multipart(
    boundary: &str,
    field_name: &str,
    filename: &str,
    content_type: &str,
    mib_chunks: usize,
) -> Body {
    let head = Bytes::from(format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
    ));
    let tail = Bytes::from(format!("\r\n--{boundary}--\r\n"));
    let stream = futures_util::stream::iter(
        std::iter::once(Ok::<Bytes, Infallible>(head))
            .chain((0..mib_chunks).map(|_| Ok(Bytes::from_static(&FILL_MIB))))
            .chain(std::iter::once(Ok(tail))),
    );
    Body::from_stream(stream)
}

async fn post_multipart(
    app: &axum::Router,
    uri: &str,
    boundary: &str,
    body: Body,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post(uri)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(body)
                .expect("multipart request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or_else(|err| {
        panic!(
            "expected JSON error body (status {status}): {err}; raw={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, json)
}

#[tokio::test]
async fn malformed_multipart_keeps_invalid_request() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("malformed multipart", "")
        .expect("group");
    let app = auth_support::authenticated_app(home);

    let (status, json) = post_multipart(
        &app,
        &format!(
            "/api/v1/groups/{}/presentation/publish_upload",
            group.group_id
        ),
        "pres-bad-boundary",
        Body::from(
            "--pres-bad-boundary\r\nContent-Disposition: form-data; name=\"file\"\r\n\r\nunterminated",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(json["error"]["code"], "invalid_request", "{json}");
}

#[tokio::test]
async fn actor_avatar_exceeding_body_limit_maps_to_avatar_too_large() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("avatar limit", "")
        .expect("group");
    let app = auth_support::authenticated_app(home);

    // 3 MiB exceeds DefaultBodyLimit (2 MiB + 64 KiB) -> HTTP 413 with existing avatar_too_large body.
    let (status, json) = post_multipart(
        &app,
        &format!("/api/v1/groups/{}/actors/a1/avatar", group.group_id),
        "avatar-limit-boundary",
        streamed_file_multipart(
            "avatar-limit-boundary",
            "file",
            "avatar.png",
            "image/png",
            3,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{json}");
    assert_eq!(json["error"]["code"], "avatar_too_large", "{json}");
    assert_eq!(json["error"]["message"], "avatar file exceeds 2 MiB");
    assert_eq!(json["error"]["details"]["max_bytes"], 2 * ONE_MIB);

    // Explicit limit window (2 MiB + 1 KiB < DefaultBodyLimit) keeps the existing HTTP 400 contract.
    let head = Bytes::from(
        "--avatar-explicit-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"avatar.png\"\r\nContent-Type: image/png\r\n\r\n",
    );
    let tail = Bytes::from("\r\n--avatar-explicit-boundary--\r\n");
    let explicit_body = Body::from_stream(futures_util::stream::iter([
        Ok::<Bytes, Infallible>(head),
        Ok(Bytes::from_static(&FILL_MIB)),
        Ok(Bytes::from_static(&FILL_MIB)),
        Ok(Bytes::from_static(&FILL_MIB[..1024])),
        Ok(tail),
    ]));
    let (status, json) = post_multipart(
        &app,
        &format!("/api/v1/groups/{}/actors/a1/avatar", group.group_id),
        "avatar-explicit-boundary",
        explicit_body,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(json["error"]["code"], "avatar_too_large", "{json}");
    assert_eq!(json["error"]["details"]["max_bytes"], 2 * ONE_MIB);
}

#[tokio::test]
async fn branding_asset_exceeding_limit_maps_to_branding_asset_too_large() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let app = auth_support::authenticated_app(home);

    // 4 MiB exceeds the 3 MiB DefaultBodyLimit (MultipartError 413 path).
    let (status, json) = post_multipart(
        &app,
        "/api/v1/branding/assets/logo_icon",
        "branding-limit-boundary",
        streamed_file_multipart(
            "branding-limit-boundary",
            "file",
            "logo.png",
            "image/png",
            4,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{json}");
    assert_eq!(json["error"]["code"], "branding_asset_too_large", "{json}");
    assert_eq!(json["error"]["message"], "branding asset exceeds 2 MiB");
    assert_eq!(json["error"]["details"]["max_bytes"], branding::MAX_BYTES);

    // 2.5 MiB is below DefaultBodyLimit but above branding::MAX_BYTES (explicit size check path).
    let head = Bytes::from(
        "--branding-explicit-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"logo.png\"\r\nContent-Type: image/png\r\n\r\n",
    );
    let tail = Bytes::from("\r\n--branding-explicit-boundary--\r\n");
    let explicit_body = Body::from_stream(futures_util::stream::iter([
        Ok::<Bytes, Infallible>(head),
        Ok(Bytes::from_static(&FILL_MIB)),
        Ok(Bytes::from_static(&FILL_MIB)),
        Ok(Bytes::from_static(&FILL_MIB[..ONE_MIB / 2])),
        Ok(tail),
    ]));
    let (status, json) = post_multipart(
        &app,
        "/api/v1/branding/assets/favicon",
        "branding-explicit-boundary",
        explicit_body,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{json}");
    assert_eq!(json["error"]["code"], "branding_asset_too_large", "{json}");
    assert_eq!(json["error"]["details"]["max_bytes"], branding::MAX_BYTES);
}

fn small_file_multipart(boundary: &str, filename: &str, content_type: &str, data: &str) -> Body {
    Body::from(format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n{data}\r\n--{boundary}--\r\n"
    ))
}

#[tokio::test]
async fn branding_asset_of_wrong_type_names_the_accepted_types() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let app = auth_support::authenticated_app(home);

    for (kind, accepted) in [
        (
            "logo_icon",
            serde_json::json!(["PNG", "SVG", "ICO", "JPEG", "WebP", "GIF", "AVIF"]),
        ),
        ("favicon", serde_json::json!(["PNG", "SVG", "ICO"])),
    ] {
        let (status, json) = post_multipart(
            &app,
            &format!("/api/v1/branding/assets/{kind}"),
            "branding-type-boundary",
            small_file_multipart("branding-type-boundary", "notes.txt", "text/plain", "hello"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{kind}: {json}");
        assert_eq!(
            json["error"]["code"], "branding_asset_unsupported_type",
            "{kind}: {json}"
        );
        assert_eq!(json["error"]["details"]["accepted"], accepted, "{kind}");
    }
}

#[tokio::test]
async fn group_copy_preview_of_a_non_package_says_so() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let app = auth_support::authenticated_app(home);

    let (status, json) = post_multipart(
        &app,
        "/api/v1/groups/copy/preview_import",
        "copy-type-boundary",
        small_file_multipart("copy-type-boundary", "notes.txt", "text/plain", "hello"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(
        json["error"]["code"], "group_copy_invalid_package",
        "{json}"
    );
}
