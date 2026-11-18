use super::*;
use crate::update_hook::{UpdateHost, UpdatePreparation};
use crate::{browser_surface, im_runtime, ledger_event_hub};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, oneshot};

/// A host whose preparation finishes only when the test says so.
struct PausedHost {
    ready: Mutex<Option<oneshot::Receiver<Result<(), UpdateRejection>>>>,
    abandoned: Mutex<Vec<String>>,
    /// Another installer put this host's program in place: it can restart
    /// into a rebuilt program, but cannot install a release.
    local_only: bool,
}

impl UpdateHost for PausedHost {
    fn begin(&self, _request: UpdateRequest) -> Result<UpdatePreparation, UpdateRejection> {
        let ready = self
            .ready
            .lock()
            .expect("ready lock")
            .take()
            .expect("one update");
        Ok(UpdatePreparation {
            transaction: "txn".into(),
            ready: Box::pin(async move { ready.await.expect("the test finishes the preparation") }),
        })
    }

    fn release_in_place(&self) -> bool {
        !self.local_only
    }

    fn local_restart(&self) -> bool {
        self.local_only
    }

    fn release_keep(&self) -> bool {
        !self.local_only
    }

    fn abandon(&self, transaction: &str) {
        self.abandoned
            .lock()
            .expect("abandoned lock")
            .push(transaction.into());
    }
}

fn state(
    home: HomeLayout,
    host: Arc<PausedHost>,
) -> (AppState, Arc<crate::RestartHandle>, broadcast::Receiver<()>) {
    state_with(
        home,
        crate::UpdateServices {
            host: Some(host),
            ..Default::default()
        },
    )
}

fn state_with(
    home: HomeLayout,
    services: crate::UpdateServices,
) -> (AppState, Arc<crate::RestartHandle>, broadcast::Receiver<()>) {
    let (shutdown, closed) = broadcast::channel(1);
    let restart = Arc::new(crate::RestartHandle::new(shutdown.clone(), services));
    let ledger_events = ledger_event_hub::LedgerEventHub::new(home.clone());
    let state = AppState {
        client: DaemonClient::new(home.clone()),
        browser_surfaces: Arc::new(browser_surface::BrowserSurfaces::default()),
        connect_frames: Arc::new(crate::connect_frames::ConnectFrames::default()),
        connect_http: crate::connect_frames::HttpClients::new().map_err(|error| error.to_string()),
        codex_voice: Arc::new(crate::codex_voice::CodexVoiceSessions::default()),
        notebooklm_auth: Arc::new(crate::notebooklm_auth::AuthFlowManager::default()),
        ledger_events: ledger_events.clone(),
        im_workers: Arc::new(im_runtime::ImWorkerRegistry::new(ledger_events)),
        shutdown,
        restart: Some(Arc::clone(&restart)),
        live_binding: crate::LiveBinding::from_env(),
        runtime_id: "web_test".into(),
        runtime_proof_key: "proof_test".into(),
        web_mode: crate::WebMode::Normal,
        exhibit_allow_terminal: false,
        push: Default::default(),
        home,
    };
    (state, restart, closed)
}

/// Starts an update as a token administrator and, when `revoke` is set,
/// removes that token while the release is still being prepared.
async fn update_while(revoke: bool) -> (ApiResult, Arc<PausedHost>, Arc<crate::RestartHandle>) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let tokens = cccc_core::access_tokens::AccessTokenStore::new(home.clone()).expect("tokens");
    let token = tokens
        .create("administrator", Vec::new(), true, None)
        .expect("administrator");
    // Another administrator stays, so this one's access can be revoked.
    tokens
        .create("owner", Vec::new(), true, None)
        .expect("another administrator");
    let principal = crate::auth::Principal {
        user_id: token.user_id.clone(),
        allowed_groups: Vec::new(),
        is_admin: true,
        raw_token: token.token.clone(),
    };
    let (finish, ready) = oneshot::channel();
    let host = Arc::new(PausedHost {
        ready: Mutex::new(Some(ready)),
        abandoned: Mutex::new(Vec::new()),
        local_only: false,
    });
    let (state, restart, _closed) = state(home, Arc::clone(&host));
    let request = tokio::spawn(start(
        State(state),
        Extension(principal),
        Json(json!({"version": "0.4.59", "mode": "restart"})),
    ));
    tokio::task::yield_now().await;
    if revoke {
        tokens.delete(&token.token_id()).expect("revoke");
    }
    finish
        .send(Ok(()))
        .expect("the request waits for its preparation");
    let result = request.await.expect("request task");
    (result, host, restart)
}

#[tokio::test]
async fn an_update_is_handed_over_only_while_its_administrator_keeps_access() {
    // Positive control: with access kept, the Web closes for the update.
    let (result, host, restart) = update_while(false).await;
    assert!(
        result.is_ok(),
        "{:?}",
        result.err().map(|error| error.to_string())
    );
    assert!(host.abandoned.lock().expect("abandoned").is_empty());
    assert!(
        restart
            .update_requested
            .load(std::sync::atomic::Ordering::Acquire)
    );

    // Revoked while the release was being prepared: nothing is handed over.
    let (result, host, restart) = update_while(true).await;
    let error = result.expect_err("revoked access is refused");
    assert_eq!(error.into_response().status(), StatusCode::FORBIDDEN);
    assert_eq!(
        *host.abandoned.lock().expect("abandoned"),
        vec!["txn".to_owned()]
    );
    assert!(
        !restart
            .update_requested
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

/// A host check that answers from fixed facts and counts its release checks.
struct FixedChecker {
    checks: std::sync::atomic::AtomicUsize,
}

impl crate::UpdateChecker for FixedChecker {
    fn status(&self) -> crate::UpdateStatus {
        crate::UpdateStatus {
            current: "0.4.58".into(),
            installation: "standalone".into(),
            last: None,
        }
    }

    fn check(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::UpdateCheck> + Send>> {
        self.checks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            crate::UpdateCheck {
                current: "0.4.58".into(),
                channel: "stable".into(),
                installation: "standalone".into(),
                latest: Some("0.4.59".into()),
                status: Some("newer".into()),
                failure: None,
                release_notes_url: None,
                command: Some("cccc update --channel stable".into()),
                local: Some(crate::LocalBuild::rebuilt(
                    "0.4.58".into(),
                    "a".repeat(64),
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_794_908_400),
                    "fixture-token".into(),
                    crate::KeepPreview {
                        allowed: false,
                        refusals: vec![crate::KeepRefusal {
                            code: "keep_not_supported".into(),
                            group_id: None,
                            actor_id: None,
                            owner_id: None,
                            display_name: None,
                            message: "fixture sentence".into(),
                        }],
                    },
                )),
            }
        })
    }
}

fn result(response: ApiResult) -> Value {
    match response {
        Ok(Json(body)) => body["result"].clone(),
        Err(error) => panic!("not answered: {error}"),
    }
}

#[tokio::test]
async fn status_and_check_come_from_the_host_and_say_whether_it_can_update_itself() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let checker = Arc::new(FixedChecker {
        checks: Default::default(),
    });
    let (finish, ready) = oneshot::channel();
    drop(finish);
    let host = Arc::new(PausedHost {
        ready: Mutex::new(Some(ready)),
        abandoned: Mutex::new(Vec::new()),
        local_only: false,
    });

    // A host that can replace itself.
    let (state, _restart, _closed) = state_with(
        home.clone(),
        crate::UpdateServices {
            checker: Some(checker.clone()),
            host: Some(host),
            on_trigger: None,
        },
    );
    let status = result(super::status(State(state.clone())).await);
    assert_eq!(status["status"]["current"], "0.4.58");
    assert_eq!(status["in_place"], true);
    // Reading the status never asks release discovery.
    assert_eq!(checker.checks.load(std::sync::atomic::Ordering::SeqCst), 0);
    let check = result(super::check(State(state)).await);
    assert_eq!(check["check"]["latest"], "0.4.59");
    assert_eq!(check["check"]["status"], "newer");
    // A rebuilt program at the host's launch path, as the page reads it.
    assert_eq!(
        check["check"]["local"],
        serde_json::json!({
            "state": "rebuilt",
            "version": "0.4.58",
            "source_id": "a".repeat(64),
            "built_at": "2026-11-17T09:40:00Z",
            "token": "fixture-token",
            "keep": {
                "allowed": false,
                "refusals": [{
                    "code": "keep_not_supported",
                    "group_id": null,
                    "actor_id": null,
                    "owner_id": null,
                    "display_name": null,
                    "message": "fixture sentence",
                }],
            },
        })
    );
    assert_eq!(check["in_place"], true);
    // A host restarts into a rebuilt program only once it says it can.
    assert_eq!(check["local_restart"], false);
    assert_eq!(check["release_keep"], true);
    assert_eq!(checker.checks.load(std::sync::atomic::Ordering::SeqCst), 1);

    // A host another installer put in place restarts into a rebuilt program,
    // but names the command for a release.
    let (_finish, ready) = oneshot::channel();
    let local_host = Arc::new(PausedHost {
        ready: Mutex::new(Some(ready)),
        abandoned: Mutex::new(Vec::new()),
        local_only: true,
    });
    let (state, _restart, _closed) = state_with(
        home.clone(),
        crate::UpdateServices {
            checker: Some(checker.clone()),
            host: Some(local_host),
            on_trigger: None,
        },
    );
    assert_eq!(
        result(super::status(State(state.clone())).await)["in_place"],
        false
    );
    let check = result(super::check(State(state)).await);
    assert_eq!(check["in_place"], false);
    assert_eq!(check["local_restart"], true);
    assert_eq!(check["release_keep"], false);

    // A host that can only check: it shows the command instead.
    let (state, _restart, _closed) = state_with(
        home.clone(),
        crate::UpdateServices {
            checker: Some(checker),
            host: None,
            on_trigger: None,
        },
    );
    let check = result(super::check(State(state)).await);
    assert_eq!(check["in_place"], false);
    assert_eq!(check["local_restart"], false);
    assert_eq!(check["release_keep"], false);

    // Without a CCCC host behind this Web there is nothing to ask.
    let (state, _restart, _closed) = state_with(home, crate::UpdateServices::default());
    let error = super::status(State(state))
        .await
        .expect_err("no host check");
    assert_eq!(error.into_response().status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_hosts_own_entry_hands_over_like_the_route_and_gives_up_when_dropped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let request = || UpdateRequest {
        source: UpdateSource::Release {
            version: "0.4.59".into(),
        },
        mode: UpdateMode::Restart,
    };

    // Dropped while the release is prepared: the update is given up.
    let (_unfinished, ready) = oneshot::channel();
    let host = Arc::new(PausedHost {
        ready: Mutex::new(Some(ready)),
        abandoned: Mutex::new(Vec::new()),
        local_only: false,
    });
    let (app, restart, _closed) = state(home.clone(), Arc::clone(&host));
    let trigger = crate::UpdateTrigger {
        restart: Arc::clone(&restart),
        voice: Arc::clone(&app.codex_voice),
    };
    let preparing = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        trigger.start(request()),
    )
    .await;
    assert!(preparing.is_err(), "the preparation never finishes");
    assert_eq!(
        *host.abandoned.lock().expect("abandoned"),
        vec!["txn".to_owned()]
    );
    assert!(
        !restart
            .update_requested
            .load(std::sync::atomic::Ordering::Acquire)
    );

    // Prepared: the Web closes for the update, as it does for the route.
    let (finish, ready) = oneshot::channel();
    let host = Arc::new(PausedHost {
        ready: Mutex::new(Some(ready)),
        abandoned: Mutex::new(Vec::new()),
        local_only: false,
    });
    let (app, restart, _closed) = state(home, Arc::clone(&host));
    let trigger = crate::UpdateTrigger {
        restart: Arc::clone(&restart),
        voice: Arc::clone(&app.codex_voice),
    };
    finish.send(Ok(())).expect("preparation");
    assert_eq!(trigger.start(request()).await.expect("handed over"), "txn");
    assert!(host.abandoned.lock().expect("abandoned").is_empty());
    assert!(
        restart
            .update_requested
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[test]
fn a_start_names_a_release_by_version_or_a_rebuilt_program_by_its_check() {
    let release = update_source(&json!({"version": "0.4.59", "mode": "restart"})).expect("release");
    assert_eq!(
        release,
        UpdateSource::Release {
            version: "0.4.59".into()
        }
    );
    let local = update_source(&json!({
        "source": "local",
        "token": "fixture-token",
        "source_id": "a".repeat(64),
        "mode": "keep",
        // A path in the request is never used.
        "program": "/elsewhere/cccc",
    }))
    .expect("local");
    assert_eq!(
        local,
        UpdateSource::Local {
            token: "fixture-token".into(),
            source_id: "a".repeat(64),
        }
    );
    for bad in [
        json!({"mode": "restart"}),
        json!({"source": "local", "source_id": "a"}),
        json!({"source": "local", "token": "t", "source_id": ""}),
        json!({"source": "elsewhere", "version": "0.4.59"}),
        // A source that is not a name is not read as no source.
        json!({"source": 42, "version": "0.4.59"}),
        json!({"source": {}, "version": "0.4.59"}),
        json!({"source": null, "version": "0.4.59"}),
    ] {
        let error = update_source(&bad).expect_err("rejected");
        assert_eq!(
            error.into_response().status(),
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
}
