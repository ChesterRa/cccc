use super::*;
use crate::{browser_surface, im_runtime, ledger_event_hub};
use axum::response::IntoResponse;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use std::sync::Arc;
use tokio::sync::broadcast;

fn test_state(home: HomeLayout) -> AppState {
    let ledger_events = ledger_event_hub::LedgerEventHub::new(home.clone());
    AppState {
        client: DaemonClient::new(home.clone()),
        browser_surfaces: Arc::new(browser_surface::BrowserSurfaces::default()),
        connect_frames: Arc::new(crate::connect_frames::ConnectFrames::default()),
        connect_http: crate::connect_frames::HttpClients::new().map_err(|error| error.to_string()),
        codex_voice: Arc::new(crate::codex_voice::CodexVoiceSessions::default()),
        notebooklm_auth: Arc::new(crate::notebooklm_auth::AuthFlowManager::default()),
        ledger_events: ledger_events.clone(),
        im_workers: Arc::new(im_runtime::ImWorkerRegistry::new(ledger_events)),
        shutdown: broadcast::channel(1).0,
        restart: None,
        live_binding: crate::LiveBinding::from_env(),
        runtime_id: "web_test".into(),
        runtime_proof_key: "proof_test".into(),
        web_mode: crate::WebMode::Normal,
        exhibit_allow_terminal: false,
        push: Default::default(),
        home,
    }
}

fn fixture() -> (tempfile::TempDir, AppState) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let dir = cccc_core::web_mods::mods_dir(&home).join("glass");
    std::fs::create_dir_all(&dir).expect("mod dir");
    std::fs::write(
        dir.join("mod.json"),
        r#"{"id":"glass","name":"Glass","version":"1","api":1,"style":"style.css"}"#,
    )
    .expect("manifest");
    std::fs::write(dir.join("style.css"), "body{color:red}").expect("style");
    std::fs::write(home.root().join("secret.txt"), "secret").expect("secret");
    (temp, test_state(home))
}

async fn body_text(response: Response<axum::body::Body>) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

#[tokio::test]
async fn lists_mods_and_switches_one_on_and_off() {
    let (_temp, state) = fixture();
    let listed = list_mods(State(state.clone())).await.expect("list").0;
    assert_eq!(listed["result"]["api"], 1);
    assert_eq!(listed["result"]["mods"][0]["id"], "glass");
    assert_eq!(listed["result"]["mods"][0]["enabled"], false);
    let before = listed["result"]["revision"].clone();

    let enabled = set_enabled(
        State(state.clone()),
        Path("glass".into()),
        Json(json!({"enabled": true})),
    )
    .await
    .expect("enable")
    .0;
    assert_eq!(enabled["result"]["mods"][0]["enabled"], true);
    assert_ne!(enabled["result"]["revision"], before);

    assert!(
        set_enabled(
            State(state.clone()),
            Path("glass".into()),
            Json(json!({"enabled": "yes"}))
        )
        .await
        .is_err()
    );
    assert!(
        set_enabled(
            State(state),
            Path("missing".into()),
            Json(json!({"enabled": true}))
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn serves_a_mods_files_as_their_type_and_nothing_outside_it() {
    let (_temp, state) = fixture();
    let response = mod_file(
        State(state.clone()),
        Path(("glass".into(), "style.css".into())),
    )
    .await
    .expect("style");
    assert!(
        response.headers()["content-type"]
            .to_str()
            .expect("type")
            .starts_with("text/css")
    );
    assert_eq!(response.headers()["cache-control"], "no-cache");
    assert_eq!(body_text(response).await, "body{color:red}");

    for (id, path) in [
        ("glass", "../../secret.txt"),
        ("glass", "../style.css"),
        ("..", "secret.txt"),
        ("glass", "missing.css"),
    ] {
        let refused = mod_file(State(state.clone()), Path((id.into(), path.into())))
            .await
            .expect_err(&format!("{id}/{path} is refused"));
        assert_eq!(
            refused.into_response().status(),
            axum::http::StatusCode::NOT_FOUND
        );
    }
}
