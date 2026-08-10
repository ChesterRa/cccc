use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::{GroupStore, HomeLayout};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
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

#[tokio::test]
async fn websocket_failure_releases_its_owned_recording_lease() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice lease cleanup", "").expect("group");
    groups
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "assistants".into(),
                json!({
                    "assistant": {
                        "assistant_id":"voice_secretary",
                        "enabled":false,
                        "config":{"recognition_backend":"assistant_service_local_asr"}
                    }
                }),
            );
            Ok(())
        })
        .expect("enable local ASR route");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let web_home = home.clone();
    let server = tokio::spawn(async move { axum::serve(listener, cccc_web::app(web_home)).await });
    let client = reqwest::Client::new();
    let lease_url = format!(
        "http://{address}/api/v1/groups/{}/assistants/voice_secretary/recording_lease",
        group.group_id
    );
    let acquired: Value = client
        .post(&lease_url)
        .json(&json!({"action":"acquire","owner_id":"tab-one"}))
        .send()
        .await
        .expect("acquire lease")
        .json()
        .await
        .expect("lease response");
    let lease_id = acquired["result"]["lease_id"].as_str().expect("lease id");
    let (mut socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/groups/{}/assistants/voice_secretary/transcriptions/ws?owner_id=tab-one&lease_id={lease_id}",
        group.group_id
    ))
    .await
    .expect("connect transcription websocket");

    socket
        .send(Message::Binary(vec![0_u8, 0].into()))
        .await
        .expect("send invalid lifecycle frame");
    let response = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("websocket response timeout")
        .expect("websocket closed before error")
        .expect("websocket response");
    let Message::Text(response) = response else {
        panic!("expected text error frame");
    };
    let response: Value = serde_json::from_str(&response).expect("error payload");
    assert_eq!(response["error"]["code"], "audio_before_start");

    let reacquired = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let response = client
                .post(&lease_url)
                .json(&json!({"action":"acquire","owner_id":"tab-two"}))
                .send()
                .await
                .expect("reacquire lease");
            if response.status().is_success() {
                break response;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned lease was not released");
    let reacquired: Value = reacquired.json().await.expect("reacquire response");
    assert_eq!(reacquired["result"]["acquired"], true);
    assert_eq!(reacquired["result"]["lease"]["owner_id"], "tab-two");

    server.abort();
}
