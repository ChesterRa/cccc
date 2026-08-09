use super::*;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use tokio::sync::mpsc;

#[derive(Debug, PartialEq, Eq)]
struct CapturedUpload {
    raw: Vec<u8>,
    file_type: String,
    filename: String,
    mime: String,
}

struct FakeMedia {
    uploads: mpsc::UnboundedSender<CapturedUpload>,
}

#[async_trait::async_trait]
impl AttachmentMedia for FakeMedia {
    async fn upload(
        &self,
        raw: &[u8],
        file_type: &str,
        filename: &str,
        mime: &str,
    ) -> Result<String, String> {
        self.uploads
            .send(CapturedUpload {
                raw: raw.to_vec(),
                file_type: file_type.into(),
                filename: filename.into(),
                mime: mime.into(),
            })
            .expect("capture upload");
        Ok(if file_type == "image" {
            "@media"
        } else {
            "@file"
        }
        .into())
    }

    async fn access_token(&self) -> Result<String, String> {
        Ok("access-token".into())
    }
}

#[derive(Clone, Copy)]
enum ServerMode {
    Success,
    OtoFails,
}

#[derive(Clone)]
struct ServerState {
    requests: mpsc::UnboundedSender<(String, HeaderMap, Value)>,
    mode: ServerMode,
}

async fn capture(
    State(state): State<ServerState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    state
        .requests
        .send((uri.path().to_owned(), headers, body))
        .expect("capture");
    if matches!(state.mode, ServerMode::OtoFails) && uri.path().contains("oToMessages") {
        return (StatusCode::BAD_GATEWAY, "upstream failed").into_response();
    }
    Json(json!({"processQueryKey":"accepted"})).into_response()
}

async fn test_sender(
    mode: ServerMode,
) -> (
    tempfile::TempDir,
    DingTalkOutboundSender,
    mpsc::UnboundedReceiver<(String, HeaderMap, Value)>,
    mpsc::UnboundedReceiver<CapturedUpload>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let group = store.create("DingTalk outbound", "").expect("group");
    let (tx, rx) = mpsc::unbounded_channel();
    let (upload_tx, upload_rx) = mpsc::unbounded_channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1.0/robot/groupMessages/send", post(capture))
                .route("/v1.0/robot/oToMessages/batchSend", post(capture))
                .with_state(ServerState { requests: tx, mode }),
        )
        .await
        .expect("server");
    });
    let sender = DingTalkOutboundSender {
        home,
        group_id: group.group_id,
        media: std::sync::Arc::new(FakeMedia { uploads: upload_tx }),
        http: reqwest::Client::new(),
        openapi_base: format!("http://{address}"),
        robot_code: "fallback-robot".into(),
    };
    (temp, sender, rx, upload_rx)
}

fn target(conversation_type: &str, chat_id: &str, user_id: &str) -> DingTalkTarget {
    DingTalkTarget {
        chat_id: chat_id.into(),
        robot_code: "callback-robot".into(),
        conversation_type: conversation_type.into(),
        user_id: user_id.into(),
    }
}

#[tokio::test]
async fn partial_target_failure_is_aggregated_and_queryable() {
    let (_temp, sender, _requests, _uploads) = test_sender(ServerMode::OtoFails).await;
    let targets = [
        target("2", "cid-group", ""),
        target("1", "cid-private", "staff-1"),
    ];
    let routes = targets
        .iter()
        .map(|target| (target, route_target(target).expect("valid target")))
        .collect::<Vec<_>>();
    let mut report = AttachmentDeliveryReport::default();
    sender
        .deliver(
            "access-token",
            &routes,
            &attachment_payload("@file", "report.md", false),
            &mut report,
        )
        .await;
    persist_failures(&sender.home, &sender.group_id, &report);
    assert_eq!(report.delivered_targets, 1);
    assert_eq!(
        report.delivered_chat_ids,
        std::collections::HashSet::from(["cid-group".to_owned()])
    );
    assert_eq!(
        report.failed_chat_ids,
        std::collections::HashSet::from(["cid-private".to_owned()])
    );
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].stage, "send");

    let store = cccc_core::GroupStore::new(sender.home.clone()).expect("store");
    let state = cccc_core::integration_state::group_get(&store, &sender.group_id, "im_bridge")
        .expect("state");
    assert_eq!(state["attachment_delivery"]["delivered_targets"], 1);
    assert_eq!(
        state["attachment_delivery"]["failures"]
            .as_array()
            .expect("failure array")
            .len(),
        1
    );
    assert!(
        state["last_error"]
            .as_str()
            .expect("last error")
            .contains("DingTalk attachment")
    );
}

#[tokio::test]
async fn invalid_routes_are_reported_without_upload_or_wrong_endpoint_fallback() {
    let (_temp, sender, mut requests, _uploads) = test_sender(ServerMode::Success).await;
    let blob = cccc_core::blobs::store(&sender.home, &sender.group_id, b"file").expect("blob");
    let report = sender
        .send_attachments(
            &[
                target("1", "cid-private", ""),
                target("2", "", ""),
                target("", "cid-group", "staff-1"),
                target("unexpected", "cid-group", "staff-1"),
            ],
            &[json!({"path":blob.path,"title":"report.md"})],
        )
        .await;
    assert_eq!(report.failures.len(), 4);
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn openapi_file_and_image_payloads_preserve_the_accepted_contract() {
    let (_temp, sender, mut requests, mut uploads) = test_sender(ServerMode::Success).await;
    let targets = [
        target("1", "cid-private", "staff-1"),
        target("2", "cid-group", ""),
    ];
    let readme = b"# CCCC\nDingTalk file attachment\n";
    let png = b"\x89PNG\r\n\x1a\nreal-image-bytes";
    let readme_blob =
        cccc_core::blobs::store(&sender.home, &sender.group_id, readme).expect("blob");
    let png_blob = cccc_core::blobs::store(&sender.home, &sender.group_id, png).expect("blob");
    let report = sender
        .send_attachments(
            &targets,
            &[
                json!({"path":readme_blob.path,"title":"README.zh-CN.md","mime_type":"text/markdown"}),
                json!({"path":png_blob.path,"title":"logo.png","mime_type":"image/png"}),
            ],
        )
        .await;
    assert_eq!(report.delivered_targets, 4);
    assert_eq!(
        report.delivered_chat_ids,
        std::collections::HashSet::from(["cid-private".to_owned(), "cid-group".to_owned()])
    );
    assert!(report.failed_chat_ids.is_empty());
    assert!(report.failures.is_empty());
    assert_eq!(
        uploads.recv().await.expect("file upload"),
        CapturedUpload {
            raw: readme.to_vec(),
            file_type: "file".into(),
            filename: "README.zh-CN.md".into(),
            mime: "text/markdown".into(),
        }
    );
    assert_eq!(
        uploads.recv().await.expect("image upload"),
        CapturedUpload {
            raw: png.to_vec(),
            file_type: "image".into(),
            filename: "logo.png".into(),
            mime: "image/png".into(),
        }
    );
    assert!(uploads.try_recv().is_err());

    let mut captured = Vec::new();
    while let Ok(request) = requests.try_recv() {
        captured.push(request);
    }
    assert_eq!(captured.len(), 4);
    for (path, headers, body) in captured {
        assert_eq!(headers["x-acs-dingtalk-access-token"], "access-token");
        assert_eq!(body["robotCode"], "callback-robot");
        if path == OTO_ENDPOINT {
            assert_eq!(body["userIds"], json!(["staff-1"]));
        } else {
            assert_eq!(path, GROUP_ENDPOINT);
            assert_eq!(body["openConversationId"], "cid-group");
        }
        match body["msgKey"].as_str().expect("msgKey") {
            "sampleFile" => assert_eq!(
                body["msgParam"],
                r#"{"fileName":"README.zh-CN.md","mediaId":"@file"}"#
            ),
            "sampleImageMsg" => {
                assert_eq!(body["msgParam"], r#"{"photoURL":"@media"}"#)
            }
            key => panic!("unexpected msgKey: {key}"),
        }
    }
}

#[tokio::test]
async fn proactive_markdown_fallback_supports_group_and_direct_targets() {
    let (_temp, sender, mut requests, mut uploads) = test_sender(ServerMode::Success).await;
    let delivered = sender
        .send_text(
            &[
                target("2", "cid-group", ""),
                target("1", "cid-private", "staff-1"),
            ],
            "final fallback",
        )
        .await;
    assert_eq!(
        delivered,
        std::collections::HashSet::from(["cid-group".to_owned(), "cid-private".to_owned()])
    );
    assert!(uploads.try_recv().is_err());

    let mut captured = Vec::new();
    while let Ok(request) = requests.try_recv() {
        captured.push(request);
    }
    assert_eq!(captured.len(), 2);
    for (path, headers, body) in captured {
        assert_eq!(headers["x-acs-dingtalk-access-token"], "access-token");
        assert_eq!(body["robotCode"], "callback-robot");
        assert_eq!(body["msgKey"], "sampleMarkdown");
        let params: Value = serde_json::from_str(
            body["msgParam"]
                .as_str()
                .expect("markdown params must be a JSON string"),
        )
        .expect("valid markdown params");
        assert_eq!(params, json!({"title":"CCCC","text":"final fallback"}));
        if path == OTO_ENDPOINT {
            assert_eq!(body["userIds"], json!(["staff-1"]));
        } else {
            assert_eq!(path, GROUP_ENDPOINT);
            assert_eq!(body["openConversationId"], "cid-group");
        }
    }
}

#[tokio::test]
async fn proactive_markdown_fallback_preserves_every_long_unicode_chunk() {
    let (_temp, sender, mut requests, _uploads) = test_sender(ServerMode::Success).await;
    let text = "你".repeat(MAX_MESSAGE_CHARS + 900);

    let delivered = sender
        .send_text(&[target("2", "cid-group", "")], &text)
        .await;

    assert_eq!(
        delivered,
        std::collections::HashSet::from(["cid-group".to_owned()])
    );
    let mut received = String::new();
    let mut chunks = 0;
    while let Ok((_path, _headers, body)) = requests.try_recv() {
        let params: Value =
            serde_json::from_str(body["msgParam"].as_str().expect("params")).expect("valid params");
        let chunk = params["text"].as_str().expect("text");
        assert!(chunk.chars().count() <= MAX_MESSAGE_CHARS);
        received.push_str(chunk);
        chunks += 1;
    }
    assert!(chunks > 1);
    assert_eq!(received, text);
}
