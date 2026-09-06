//! Actual WebSocket routes with an isolated shell TUI and a fake app-server.
//! No microphone, daemon, installed Analyst or provider request is involved.
use super::*;
use cccc_core::access_tokens::AccessTokenStore;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

struct TerminalCleanup(Arc<AnalystRuntime>);
impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        self.0.stop_terminal();
    }
}

#[tokio::test]
async fn voice_sockets_revoke_idle_terminals_and_report_notification_failure() {
    // The production launcher resolves the CCCC executable from process state.
    // Give only this test child a disposable launcher; never mutate global env
    // while other tests run or depend on an installed CCCC executable.
    if std::env::var_os("CCCC_TEST_VOICE_SOCKET_CHILD").is_none() {
        let temp = tempfile::tempdir().expect("launcher directory");
        let launcher = temp.path().join("cccc");
        std::fs::write(&launcher, "#!/bin/sh\nexit 1\n").expect("fixture launcher");
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "codex_voice::socket_tests::voice_sockets_revoke_idle_terminals_and_report_notification_failure", "--nocapture"])
            .env("CCCC_TEST_VOICE_SOCKET_CHILD", "1")
            .env("CCCC_LAUNCHER_PATH", launcher)
            .output().expect("isolated test child");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fake app-server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let app_server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("app-server connection");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("handshake");
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let request: Value = serde_json::from_str(&text).expect("request");
            let Some(id) = request.get("id") else {
                continue;
            };
            let result = match request["method"].as_str() {
                Some("thread/start" | "thread/read") => {
                    json!({"thread":{"id":"fixture-thread","turns":[]}})
                }
                _ => json!({}),
            };
            if socket
                .send(Message::Text(
                    json!({"id":id,"result":result}).to_string().into(),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let program = temp.path().join("fake-codex");
    std::fs::write(&program, format!("#!/bin/sh\ncase \" $* \" in\n*' app-server '*) printf 'listening on: {endpoint}\\n'; exec sleep 120;;\n*) printf 'Fixture terminal ready\\n'; exec cat;;\nesac\n")).expect("fake runtime");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("executable");
    let workdir = cccc_core::codex_voice_settings::workdir(&home).expect("workdir");
    let mut config = LaunchConfig::new(&workdir);
    config.command = vec![program.to_string_lossy().into_owned()];
    let launch_runtime = ResolvedAgentRuntime {
        runtime: cccc_contracts::ActorRuntime::Codex,
        command: config.command.clone(),
        environment: BTreeMap::new(),
    };
    let analyst = CodexVoiceAnalyst::launch(&home, config)
        .await
        .expect("fake Analyst");
    let runtime = Arc::new(AnalystRuntime::new(
        workdir,
        analyst,
        launch_runtime,
        "ready",
        String::new(),
    ));
    let generation = runtime.info().generation;
    let _terminal_cleanup = TerminalCleanup(Arc::clone(&runtime));
    let (shutdown, _) = tokio::sync::broadcast::channel(1);
    let (router, _, _, state) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 0,
        },
        "socket-fixture".into(),
    );
    state.codex_voice.state.lock().await.analyst = Some(Arc::clone(&runtime));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture Web");
    let address = listener.local_addr().expect("Web address");
    let web = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    let owner = tokens.create("owner", vec![], true, None).expect("owner");

    for mode in ["viewer", "control"] {
        let token = tokens
            .create("temporary admin", vec![], true, None)
            .expect("admin");
        let mut request =
            format!("ws://{address}/api/v1/codex_voice/analysts/{generation}/terminal?mode={mode}")
                .into_client_request()
                .expect("request");
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", token.token).parse().expect("header"),
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(request)
            .await
            .expect("terminal connection");
        let first = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("attach timeout")
            .expect("attach response")
            .expect("attach frame");
        assert!(
            matches!(first, Message::Binary(bytes) if bytes.first() == Some(&b'3')),
            "terminal must attach before revocation"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(250), async {
                loop {
                    match socket.next().await {
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                            panic!("authorized terminal closed before revocation")
                        }
                        _ => {}
                    }
                }
            })
            .await
            .is_err(),
            "authorized terminal stays connected"
        );
        if mode == "viewer" {
            tokens.delete(&token.token_id()).expect("revoke");
        } else {
            tokens
                .update(&token.token_id(), None, Some(false))
                .expect("downgrade");
        }
        // No input or new output is needed to notice revocation.
        tokio::time::timeout(Duration::from_secs(3), async {
            while let Some(Ok(message)) = socket.next().await {
                if matches!(message, Message::Close(_)) {
                    return;
                }
            }
        })
        .await
        .expect("idle terminal must close after authorization is removed");
    }

    let call = Arc::new(
        CodexVoiceCall::start(&home, runtime.analyst())
            .await
            .expect("local call lease"),
    );
    let call_generation = call.generation().to_owned();
    let session = Arc::new(ActiveSession {
        notification_paused: tokio::sync::watch::channel(false).0,
        call,
        analyst: Arc::clone(&runtime),
        client_session_id: "fixture".into(),
        offer_digest: [0; 32],
        answer_sdp: String::new(),
        voice: "cove".into(),
        connection_state: AtomicU8::new(CONNECTION_UNATTACHED),
    });
    state.codex_voice.state.lock().await.active = Some(Arc::clone(&session));
    // Corruption is a credible ingestion failure. It must be visible to the
    // connected owner without falsely declaring the Analyst disconnected.
    let notifications = home.root().join("state/codex_voice/notifications.json");
    std::fs::write(&notifications, "{").expect("inject isolated ingestion error");
    let mut request = format!("ws://{address}/api/v1/codex_voice/calls/{call_generation}/events")
        .into_client_request()
        .expect("events request");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", owner.token).parse().expect("header"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("call connection");
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Text(text) = message {
                let event: Value = serde_json::from_str(&text).expect("event");
                if event["type"] == "notification_status" {
                    assert_eq!(event["paused"], true);
                    return;
                }
            }
        }
        panic!("call closed without surfacing paused notifications");
    })
    .await
    .expect("paused event deadline");
    assert!(session.info().connected);
    assert_eq!(runtime.info().phase, "ready");
    assert_eq!(
        std::fs::read_to_string(&notifications).expect("retained data"),
        "{"
    );
    socket.close(None).await.expect("close call");
    runtime.stop_terminal();
    runtime.analyst.shutdown().await.expect("stop fake Analyst");
    let _ = shutdown.send(());
    web.abort();
    app_server.abort();
}
