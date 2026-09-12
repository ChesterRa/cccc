use super::*;
use crate::AppState;
use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse, Transport};
use cccc_core::{GroupStore, Scope};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(super) struct Fixture {
    _temp: tempfile::TempDir,
    pub(super) state: AppState,
    pub(super) group: String,
    pub(super) requests: Arc<Mutex<Vec<DaemonRequest>>>,
    pub(super) failures: Arc<AtomicUsize>,
    daemon: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.daemon.abort();
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
            op: "assistant_settings_update".into(),
            args: json!({"group_id":group,"assistant_id":"voice_secretary","by":"user","patch":{"enabled":true,"config":{"recognition_backend":"external_provider_asr","external_asr_provider":"volcengine"}}}).as_object().expect("request arguments object").clone(),
        });
        assert!(enabled.ok, "{:?}", enabled.error);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("initialize persistence fixture");
        cccc_core::fs::write_json(
            &home.daemon_dir().join("ccccd.addr.json"),
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
        let daemon = tokio::spawn({
            let home = home.clone();
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
                    let reply = cccc_daemon::handle_request(&home, &request);
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
            client: cccc_client::DaemonClient::new(home.clone())
                .with_timeout(Duration::from_secs(2)),
            home,
            browser_surfaces: Arc::new(crate::browser_surface::BrowserSurfaces::default()),
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
            daemon,
        }
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
