use axum::http::StatusCode;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const TEST_ADMIN_TOKEN: &str = "assistant-voice-ws-test-admin";

#[tokio::test]
async fn mock_streaming_recordings_complete_twice_without_poisoning_the_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    cccc_core::access_tokens::AccessTokenStore::new(home.clone())
        .expect("access token store")
        .create("test-admin", Vec::new(), true, Some(TEST_ADMIN_TOKEN))
        .expect("test admin token");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice websocket", "").expect("group");
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
    let port = free_port().await;
    let address = format!("127.0.0.1:{port}").parse().expect("web address");
    let mut web = spawn_mock_web(&home, port);
    wait_for_port(port).await;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {TEST_ADMIN_TOKEN}")
            .parse()
            .expect("authorization header"),
    );
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("HTTP client");
    assert_mock_readiness(&client, address, &group.group_id).await;

    let failed_owner = "tab-failed";
    let failed_lease = acquire_lease(&client, address, &group.group_id, failed_owner).await;
    fail_before_start(address, &group.group_id, failed_owner, &failed_lease).await;

    record_and_assert(&client, address, &group.group_id, "tab-one", "session-one").await;
    assert!(!daemon.is_finished(), "recording stopped the daemon");

    web.kill().await.expect("stop web child");
    let _ = web.wait().await;
    let _ = cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");

    let restarted_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(restarted_home).await });
    wait_for_daemon(&home).await;
    let mut web = spawn_mock_web(&home, port);
    wait_for_port(port).await;
    assert_mock_readiness(&client, address, &group.group_id).await;
    record_and_assert(&client, address, &group.group_id, "tab-two", "session-two").await;
    assert!(
        !daemon.is_finished(),
        "recording stopped the restarted daemon"
    );

    web.kill().await.expect("stop restarted web child");
    let _ = web.wait().await;
    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon
        .await
        .expect("restarted daemon task")
        .expect("restarted daemon");
}

async fn assert_mock_readiness(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    group_id: &str,
) {
    let response: Value = client
        .get(format!(
            "http://{address}/api/v1/groups/{group_id}/assistants/voice_secretary"
        ))
        .send()
        .await
        .expect("assistant readiness")
        .json()
        .await
        .expect("assistant readiness response");
    let service = &response["result"]["assistant"]["health"]["service"];
    assert_eq!(service["ready"], true, "{service}");
    assert_eq!(service["mock"], true, "{service}");
    assert_eq!(service["streaming_backend"]["ready"], true, "{service}");
    assert_eq!(
        service["streaming_backend"]["model_id"], "mock",
        "{service}"
    );
}

fn spawn_mock_web(home: &HomeLayout, port: u16) -> tokio::process::Child {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc-web"))
        .env("CCCC_HOME", home.root())
        .env("CCCC_WEB_HOST", "127.0.0.1")
        .env("CCCC_WEB_PORT", port.to_string())
        .env("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT", "rust mock transcript")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("web child")
}

async fn record_and_assert(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    group_id: &str,
    owner_id: &str,
    session_id: &str,
) {
    let lease_id = acquire_lease(client, address, group_id, owner_id).await;
    let frames = record_once(address, group_id, owner_id, &lease_id, session_id).await;
    assert!(
        frames.iter().any(|frame| {
            frame["type"] == "final_asr_text"
                && frame["ok"] != false
                && frame["text"] == "rust mock transcript"
        }),
        "final ASR result missing: {frames:?}"
    );
    assert!(
        frames.iter().any(|frame| frame["type"] == "closed"),
        "closed frame missing: {frames:?}"
    );
    let health = client
        .get(format!("http://{address}/api/v1/health"))
        .send()
        .await
        .expect("health after recording");
    assert_eq!(health.status(), StatusCode::OK);
}

async fn fail_before_start(
    address: std::net::SocketAddr,
    group_id: &str,
    owner_id: &str,
    lease_id: &str,
) {
    let (mut socket, _) = connect_ws(format!(
        "ws://{address}/api/v1/groups/{group_id}/assistants/voice_secretary/transcriptions/ws?owner_id={owner_id}&lease_id={lease_id}"
    ))
    .await
    .expect("connect failed transcription websocket");
    socket
        .send(Message::Binary(vec![0_u8; 1_600].into()))
        .await
        .expect("send PCM16 before start");
    let error = next_json(&mut socket).await;
    assert_eq!(error["type"], "error", "{error}");
    assert_eq!(error["error"]["code"], "audio_before_start", "{error}");
}

async fn acquire_lease(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    group_id: &str,
    owner_id: &str,
) -> String {
    let url = format!(
        "http://{address}/api/v1/groups/{group_id}/assistants/voice_secretary/recording_lease"
    );
    let response: Value = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let response = client
                .post(&url)
                .json(&json!({
                    "action":"acquire",
                    "owner_id":owner_id,
                    "capture_mode":"prompt",
                    "recognition_backend":"assistant_service_local_asr",
                    "dispatch_target":"composer"
                }))
                .send()
                .await
                .expect("acquire lease");
            if response.status().is_success() {
                break response.json().await.expect("lease response");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("prior recording lease was not released");
    response["result"]["lease_id"]
        .as_str()
        .unwrap_or_else(|| panic!("lease id missing from response: {response}"))
        .to_owned()
}

async fn record_once(
    address: std::net::SocketAddr,
    group_id: &str,
    owner_id: &str,
    lease_id: &str,
    session_id: &str,
) -> Vec<Value> {
    let (mut socket, _) = connect_ws(format!(
        "ws://{address}/api/v1/groups/{group_id}/assistants/voice_secretary/transcriptions/ws?owner_id={owner_id}&lease_id={lease_id}"
    ))
    .await
    .expect("connect transcription websocket");
    socket
        .send(Message::Text(
            json!({
                "type":"start","seq":1,"session_id":session_id,
                "capture_mode":"prompt","dispatch_target":"composer",
                "sample_rate":16000,"language":"en-US"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("start recording");
    let ready = next_json(&mut socket).await;
    assert_eq!(ready["type"], "ready", "{ready}");
    socket
        .send(Message::Binary(vec![0_u8; 1_600].into()))
        .await
        .expect("send PCM16");
    socket
        .send(Message::Text(
            json!({"type":"stop","seq":2}).to_string().into(),
        ))
        .await
        .expect("stop recording");
    let mut frames = Vec::new();
    for _ in 0..8 {
        let frame = next_json(&mut socket).await;
        let closed = frame["type"] == "closed";
        frames.push(frame);
        if closed {
            break;
        }
    }
    frames
}

async fn next_json(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("websocket response timeout")
        .expect("websocket closed before response")
        .expect("websocket response");
    let Message::Text(text) = message else {
        panic!("expected text websocket response");
    };
    serde_json::from_str(&text).expect("websocket JSON")
}

async fn connect_ws(
    url: String,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    tokio_tungstenite::tungstenite::Error,
> {
    let mut request = url.into_client_request()?;
    request.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {TEST_ADMIN_TOKEN}")
            .parse()
            .expect("authorization header"),
    );
    tokio_tungstenite::connect_async(request).await
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

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("free port listener");
    listener.local_addr().expect("free port address").port()
}

async fn wait_for_port(port: u16) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("web port did not open");
}
