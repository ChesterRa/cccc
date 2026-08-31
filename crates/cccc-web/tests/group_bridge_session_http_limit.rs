mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION;
use cccc_core::HomeLayout;
use serde_json::json;
use tower::ServiceExt;

const TEN_MIB_BASE64_BYTES: usize = (10 * 1024 * 1024_usize).div_ceil(3) * 4;

#[tokio::test]
async fn remote_http_fallback_accepts_a_base64_encoded_ten_mib_attachment() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let content_base64 = "A".repeat(TEN_MIB_BASE64_BYTES);
    let request = Request::post("/api/group-bridge/session/send")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "op":"remote_send",
                "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
                "attachments":[{"content_base64":content_base64}]
            })
            .to_string(),
        ))
        .expect("request");

    let response = auth_support::authenticated_app(home)
        .oneshot(request)
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn remote_http_fallback_remains_bounded_above_the_attachment_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let content_base64 = "A".repeat(TEN_MIB_BASE64_BYTES + 2 * 1024 * 1024);
    let request = Request::post("/api/group-bridge/session/send")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "op":"remote_send",
                "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
                "attachments":[{"content_base64":content_base64}]
            })
            .to_string(),
        ))
        .expect("request");

    let response = auth_support::authenticated_app(home)
        .oneshot(request)
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
