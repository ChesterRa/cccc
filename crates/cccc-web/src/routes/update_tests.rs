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
    let (shutdown, closed) = broadcast::channel(1);
    let restart = Arc::new(crate::RestartHandle::new(shutdown.clone(), Some(host)));
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
