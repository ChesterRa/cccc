use ::tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, access_tokens};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use std::process::Stdio;
use std::time::Duration;

const ADMIN_TOKEN: &str = "assistant-voice-ws-revision-admin";
pub type VoiceSocket = ::tokio_tungstenite::WebSocketStream<
    ::tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

pub struct Harness {
    pub _temp: tempfile::TempDir,
    pub home: HomeLayout,
    pub group_id: String,
    pub daemon: tokio::task::JoinHandle<anyhow::Result<()>>,
    pub web: tokio::process::Child,
    pub port: u16,
    client: reqwest::Client,
}

pub async fn setup() -> Harness {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    access_tokens::AccessTokenStore::new(home.clone())
        .expect("access token store")
        .create("test-admin", Vec::new(), true, Some(ADMIN_TOKEN))
        .expect("admin token");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice revision", "").expect("group");
    cccc_core::settings::update(&home, |settings| {
        settings.voice_secretary.config.recognition_backend = "assistant_service_local_asr".into();
        Ok(())
    })
    .expect("global recognition configuration");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let mut web = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc-web"))
        .env("CCCC_HOME", home.root())
        .env("CCCC_WEB_HOST", "127.0.0.1")
        .env("CCCC_WEB_PORT", "0")
        .env("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT", "最终文本。")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("web child");
    let port = wait_for_web_port(&home, &mut web).await;
    Harness {
        _temp: temp,
        home,
        group_id: group.group_id,
        daemon,
        web,
        port,
        client: authenticated_client(),
    }
}

pub async fn acquire_document_lease(harness: &Harness) -> String {
    let response: Value = harness
        .client
        .post(format!(
            "http://127.0.0.1:{}/api/v1/groups/{}/assistants/voice_secretary/recording_lease",
            harness.port, harness.group_id
        ))
        .json(&json!({
            "action":"acquire","owner_id":"tab-one","capture_mode":"document",
            "recognition_backend":"assistant_service_local_asr","dispatch_target":"document"
        }))
        .send()
        .await
        .expect("acquire lease")
        .json()
        .await
        .expect("lease response");
    response["result"]["lease_id"]
        .as_str()
        .expect("lease id")
        .to_owned()
}

pub async fn start_document_recording(
    harness: &Harness,
    lease_id: &str,
    session_id: &str,
) -> VoiceSocket {
    let url = format!(
        "ws://127.0.0.1:{}/api/v1/groups/{}/assistants/voice_secretary/transcriptions/ws?owner_id=tab-one&lease_id={lease_id}",
        harness.port, harness.group_id
    );
    let mut request = url.into_client_request().expect("websocket request");
    request.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {ADMIN_TOKEN}")
            .parse()
            .expect("authorization header"),
    );
    let (mut socket, _) = ::tokio_tungstenite::connect_async(request)
        .await
        .expect("connect websocket");
    socket
        .send(Message::Text(
            json!({
                "type":"start","seq":1,"session_id":session_id,
                "capture_mode":"document","dispatch_target":"document",
                "document_path":"docs/voice-secretary/meeting.md",
                "sample_rate":16000,"language":"zh-CN"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("start recording");
    assert_eq!(next_json(&mut socket).await["type"], "ready");
    socket
}

pub async fn send_stop(socket: &mut VoiceSocket) {
    socket
        .send(Message::Text(
            json!({"type":"stop","seq":2}).to_string().into(),
        ))
        .await
        .expect("stop recording");
}

pub async fn collect_frames(socket: &mut VoiceSocket) -> Vec<Value> {
    let mut frames = Vec::new();
    for _ in 0..8 {
        let frame = next_json(socket).await;
        let closed = frame["type"] == "closed";
        frames.push(frame);
        if closed {
            break;
        }
    }
    frames
}

async fn next_json(socket: &mut VoiceSocket) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("websocket timeout")
        .expect("websocket closed")
        .expect("websocket response");
    let Message::Text(text) = message else {
        panic!("expected text websocket response");
    };
    serde_json::from_str(&text).expect("websocket JSON")
}

fn authenticated_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {ADMIN_TOKEN}")
            .parse()
            .expect("authorization header"),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("HTTP client")
}

pub async fn shutdown(home: &HomeLayout) {
    let _ = cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
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

/// Web binds a free port itself and records it in this home. A number picked here and
/// released first can reach another test's server, or nothing, before this Web binds it.
async fn wait_for_web_port(home: &HomeLayout, web: &mut tokio::process::Child) -> u16 {
    let runtime = home.daemon_dir().join("web_runtime.json");
    // Debug builds can take several seconds to start under load.
    for _ in 0..2_000 {
        if let Ok(state) = cccc_core::fs::read_json::<Value>(&runtime)
            && let Some(port) = state["port"].as_u64().filter(|port| *port != 0)
        {
            return u16::try_from(port).expect("web port");
        }
        assert!(
            web.try_wait().expect("web status").is_none(),
            "web exited before it was ready"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("web did not report its port");
}
