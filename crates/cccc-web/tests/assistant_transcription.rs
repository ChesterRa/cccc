use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, assistant_state};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
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
async fn clearing_a_transcript_updates_the_shared_assistant_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice transcript", "").expect("group");
    assistant_state::update(&home, &group.group_id, |state| {
        state.insert(
            "sessions".into(),
            json!([
                {
                    "session_id":"session-old",
                    "capture_mode":"document",
                    "document_path":"notes.md",
                    "segments":[{"text":"old","is_final":true}],
                    "transcript":"old",
                    "updated_at":"2026-08-10T01:00:00Z"
                },
                {
                    "session_id":"session-new",
                    "capture_mode":"document",
                    "document_path":"notes.md",
                    "segments":[{"text":"new","is_final":true}],
                    "transcript":"new",
                    "updated_at":"2026-08-10T02:00:00Z"
                }
            ]),
        );
        Ok(())
    })
    .expect("seed shared assistant state");

    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::delete(format!(
                "/api/v1/groups/{}/assistants/voice_secretary/sessions/latest/transcript",
                group.group_id
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"document_path":"notes.md"}"#))
            .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let state = assistant_state::load(&home, &group.group_id).expect("shared state");
    assert_eq!(state["sessions"][0]["session_id"], "session-old");
    assert_eq!(state["sessions"][0]["transcript"], "old");
    assert_eq!(state["sessions"][1]["session_id"], "session-new");
    assert_eq!(state["sessions"][1]["transcript"], "");
    assert_eq!(state["sessions"][1]["segments"], json!([]));
    let group = groups.load(&group.group_id).expect("group");
    assert!(group.extra.get("assistants").is_none());
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
                        "enabled":true,
                        "config":{"recognition_backend":"assistant_service_local_asr"}
                    }
                }),
            );
            Ok(())
        })
        .expect("enable local ASR route");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
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
    let lease_id = acquired["result"]["lease_id"]
        .as_str()
        .unwrap_or_else(|| panic!("lease id missing from response: {acquired}"));
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
    let second_lease_id = reacquired["result"]["lease_id"]
        .as_str()
        .unwrap_or_else(|| panic!("second lease id missing from response: {reacquired}"));

    let (mut missing_model_socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/groups/{}/assistants/voice_secretary/transcriptions/ws?owner_id=tab-two&lease_id={second_lease_id}",
        group.group_id
    ))
    .await
    .expect("connect missing-model transcription websocket");
    missing_model_socket
        .send(Message::Text(
            json!({
                "type":"start",
                "seq":1,
                "session_id":"missing-model",
                "capture_mode":"document",
                "dispatch_target":"document",
                "document_path":"notes.md",
                "sample_rate":16000
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("start missing-model transcription");
    let response = tokio::time::timeout(Duration::from_secs(2), missing_model_socket.next())
        .await
        .expect("missing-model response timeout")
        .expect("websocket closed before missing-model error")
        .expect("missing-model websocket response");
    let Message::Text(response) = response else {
        panic!("expected missing-model text error frame");
    };
    let response: Value = serde_json::from_str(&response).expect("missing-model error payload");
    assert_eq!(response["error"]["code"], "voice_model_not_installed");
    assert!(
        !daemon.is_finished(),
        "voice start failure stopped the daemon"
    );
    let health = client
        .get(format!("http://{address}/api/v1/health"))
        .send()
        .await
        .expect("health after voice start failure");
    assert_eq!(health.status(), StatusCode::OK);
    let recovered = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let response = client
                .post(&lease_url)
                .json(&json!({"action":"acquire","owner_id":"tab-three"}))
                .send()
                .await
                .expect("acquire after voice start failure");
            if response.status().is_success() {
                break response;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("voice start failure did not release its recording lease");
    let recovered: Value = recovered.json().await.expect("recovered lease response");
    assert_eq!(recovered["result"]["acquired"], true);
    assert_eq!(recovered["result"]["lease"]["owner_id"], "tab-three");

    server.abort();
    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

async fn wait_for_daemon(home: &HomeLayout) {
    let address = home.daemon_dir().join("ccccd.addr.json");
    for _ in 0..100 {
        if address.is_file() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon address was not created");
}
