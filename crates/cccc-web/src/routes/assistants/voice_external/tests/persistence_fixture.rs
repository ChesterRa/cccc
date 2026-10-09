use super::*;
use crate::AppState;
use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse, Transport};
use cccc_core::{GroupStore, Scope};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(super) fn delegated(name: &str) -> bool {
    const KEY: &str = "CCCC_VOICE_PERSISTENCE_DAEMON_FIXTURE";
    if std::env::var(KEY).as_deref() == Ok(name) {
        return false;
    }
    // A real daemon owns global runtime registries. Keep it out of other Web
    // unit tests while exercising the native acceptance/recovery boundary.
    let temp = tempfile::tempdir().expect("isolated daemon environment");
    let test = format!("routes::assistants::voice_external::tests::persistence_tests::{name}");
    let result = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", &test, "--nocapture"])
        .env(KEY, name)
        .env("HOME", temp.path())
        .env("CCCC_HOME", temp.path().join("state"))
        .env("CODEX_HOME", temp.path().join("codex"))
        .env_remove("CCCC_GROUP_ID")
        .env_remove("CCCC_ACTOR_ID")
        .env_remove("CCCC_SECRETARY_TASK_TOKEN")
        .env_remove("CCCC_MCP_TOOL_PROFILE")
        .output()
        .expect("isolated daemon test");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    true
}

pub(super) struct Fixture {
    _temp: tempfile::TempDir,
    pub(super) state: AppState,
    pub(super) group: String,
    pub(super) requests: Arc<Mutex<Vec<DaemonRequest>>>,
    pub(super) failures: Arc<AtomicUsize>,
    proxy: tokio::task::JoinHandle<()>,
    daemon: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.proxy.abort();
        if let Some(daemon) = &self.daemon {
            daemon.abort();
        }
    }
}

impl Fixture {
    pub(super) async fn new(lose_reply: bool) -> Self {
        let (temp, home) = home();
        let store = GroupStore::new(home.clone()).expect("initialize persistence fixture");
        let group = store
            .create("external persistence", "")
            .expect("initialize persistence fixture")
            .group_id;
        store
            .mutate(&group, |doc| {
                let mut foreman = cccc_contracts::Actor::new("foreman");
                foreman.role = Some(cccc_contracts::ActorRole::Foreman);
                doc.actors.push(foreman);
                doc.scopes.push(Scope {
                    scope_key: "workspace".into(),
                    url: temp.path().to_string_lossy().into(),
                    label: "workspace".into(),
                    git_remote: String::new(),
                });
                doc.active_scope_key = "workspace".into();
                Ok(())
            })
            .expect("initialize persistence fixture");
        let enabled = cccc_daemon::handle_request(&home, &DaemonRequest {
            v: 1,
            op: "voice_secretary_settings_update".into(),
            args: json!({"by":"user","settings":{"config":{"recognition_backend":"external_provider_asr","external_asr_provider":"volcengine"}}}).as_object().expect("request arguments object").clone(),
        });
        assert!(enabled.ok, "{:?}", enabled.error);
        let daemon = tokio::spawn(cccc_daemon::run(home.clone()));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !cccc_daemon::DaemonPaths::new(home.clone()).address.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "native daemon startup"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let proxy_home = HomeLayout::from_path(temp.path().join("proxy-home")).expect("proxy home");
        proxy_home.initialize().expect("proxy initialization");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("initialize persistence fixture");
        cccc_core::fs::write_json(
            &proxy_home.daemon_dir().join("ccccd.addr.json"),
            &serde_json::to_value(DaemonAddress {
                v: 1,
                transport: Transport::Tcp,
                path: String::new(),
                host: "127.0.0.1".into(),
                port: listener.local_addr().expect("listener address").port(),
                pid: std::process::id(),
                version: "test".into(),
                ts: "test".into(),
            })
            .expect("initialize persistence fixture"),
        )
        .expect("initialize persistence fixture");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let failures = Arc::new(AtomicUsize::new(0));
        let proxy = tokio::spawn({
            let client = cccc_client::DaemonClient::new(home.clone());
            let requests = Arc::clone(&requests);
            let failures = Arc::clone(&failures);
            async move {
                loop {
                    let (stream, _) = listener.accept().await.expect("accept daemon connection");
                    let mut stream = BufReader::new(stream);
                    let mut line = String::new();
                    stream
                        .read_line(&mut line)
                        .await
                        .expect("read daemon request");
                    let request: DaemonRequest =
                        serde_json::from_str(&line).expect("daemon request JSON");
                    requests
                        .lock()
                        .expect("recorded requests lock")
                        .push(request.clone());
                    let fail = failures
                        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                        .is_ok();
                    if fail && !lose_reply {
                        let reply =
                            DaemonResponse::failure("io_error", "injected checkpoint failure");
                        stream
                            .get_mut()
                            .write_all(
                                format!(
                                    "{}\n",
                                    serde_json::to_string(&reply).expect("daemon response JSON")
                                )
                                .as_bytes(),
                            )
                            .await
                            .expect("initialize persistence fixture");
                        continue;
                    }
                    let reply = client.call(&request).await.expect("native daemon request");
                    assert!(
                        reply.ok,
                        "fixture daemon rejected request: {:?}",
                        reply.error
                    );
                    if fail {
                        assert!(reply.ok, "{:?}", reply.error);
                        continue;
                    }
                    stream
                        .get_mut()
                        .write_all(
                            format!(
                                "{}\n",
                                serde_json::to_string(&reply).expect("daemon response JSON")
                            )
                            .as_bytes(),
                        )
                        .await
                        .expect("initialize persistence fixture");
                }
            }
        });
        let ledger_events = crate::ledger_event_hub::LedgerEventHub::new(home.clone());
        let state = AppState {
            client: cccc_client::DaemonClient::new(proxy_home).with_timeout(Duration::from_secs(2)),
            home,
            browser_surfaces: Arc::new(crate::browser_surface::BrowserSurfaces::default()),
            connect_frames: Arc::new(crate::connect_frames::ConnectFrames::default()),
            connect_http: crate::connect_frames::http_client()
                .build()
                .map_err(|error| error.to_string()),
            codex_voice: Arc::new(crate::codex_voice::CodexVoiceSessions::default()),
            notebooklm_auth: Arc::new(crate::notebooklm_auth::AuthFlowManager::default()),
            ledger_events: ledger_events.clone(),
            im_workers: Arc::new(crate::im_runtime::ImWorkerRegistry::new(ledger_events)),
            shutdown: tokio::sync::broadcast::channel(1).0,
            restart: None,
            live_binding: crate::LiveBinding::from_env(),
            runtime_id: "test".into(),
            runtime_proof_key: "test".into(),
            web_mode: crate::WebMode::Normal,
            exhibit_allow_terminal: false,
        };
        Self {
            _temp: temp,
            state,
            group,
            requests,
            failures,
            proxy,
            daemon: Some(daemon),
        }
    }

    pub(super) async fn shutdown(mut self) {
        self.proxy.abort();
        let response = cccc_client::DaemonClient::new(self.state.home.clone())
            .call(&DaemonRequest {
                v: 1,
                op: "shutdown".into(),
                args: Default::default(),
            })
            .await
            .expect("shutdown request");
        assert!(response.ok, "{:?}", response.error);
        tokio::time::timeout(Duration::from_secs(5), self.daemon.take().expect("daemon"))
            .await
            .expect("bounded shutdown")
            .expect("daemon join")
            .expect("daemon cleanup");
    }

    pub(super) async fn active(&self) -> (active::Active, tokio::net::TcpStream) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("initialize persistence fixture");
        let (client, server) = tokio::join!(
            tokio::net::TcpStream::connect(listener.local_addr().expect("listener address")),
            listener.accept()
        );
        let socket = tokio_tungstenite::WebSocketStream::from_raw_socket(
            tokio_tungstenite::MaybeTlsStream::Plain(client.expect("provider connection")),
            tokio_tungstenite::tungstenite::protocol::Role::Client,
            None,
        )
        .await;
        let run = active::Active::from_opened(&self.state.home, &json!({"session_id":"external-test","capture_mode":"document","document_path":"docs/meeting.md"}), connection::Opened {
            socket, codec: connection::Codec { provider: Provider::Volcengine, task: "test".into() }, model: "volcengine:test".into(),
        }).expect("initialize persistence fixture");
        (run, server.expect("accepted provider connection").0)
    }
}
