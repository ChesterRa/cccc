use super::*;
use crate::{browser_surface, im_runtime, ledger_event_hub};
use axum::response::IntoResponse;
use cccc_client::DaemonClient;
use cccc_core::{GroupStore, HomeLayout, ledger};
use futures_util::{StreamExt, stream};
use std::sync::Arc;
use std::time::Duration;

fn local_principal() -> Principal {
    Principal {
        user_id: "local".into(),
        allowed_groups: Vec::new(),
        is_admin: true,
        raw_token: String::new(),
    }
}

#[tokio::test]
async fn an_already_open_group_stream_stops_after_its_token_is_removed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("revocation", "")
        .expect("group");
    let tokens = cccc_core::access_tokens::AccessTokenStore::new(home.clone()).expect("tokens");
    let token = tokens
        .create("viewer", vec![group.group_id.clone()], false, None)
        .expect("viewer");
    tokens
        .create("administrator", Vec::new(), true, None)
        .expect("admin");
    let principal = Principal {
        user_id: token.user_id.clone(),
        allowed_groups: token.allowed_groups.clone(),
        is_admin: false,
        raw_token: token.token.clone(),
    };
    let response = group_events(
        State(test_state(home)),
        Path(group.group_id),
        HeaderMap::new(),
        Extension(principal),
        Query(ResourceFrameQuery::default()),
    )
    .await
    .expect("stream")
    .into_response();
    let mut body = response.into_body().into_data_stream();
    let connected = body.next().await.expect("connected chunk").expect("chunk");
    assert!(String::from_utf8_lossy(&connected).contains("connected"));
    tokens.delete(&token.token_id()).expect("remove viewer");
    let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
        .await
        .expect("current access check")
        .expect("revocation event")
        .expect("chunk");
    assert!(String::from_utf8_lossy(&chunk).contains("auth_required"));
    assert!(body.next().await.is_none());
}

async fn encoded_event_name(name: &'static str) -> String {
    let event = cccc_contracts::Event::new("chat.message", "g_test");
    let response = Sse::new(stream::iter([Ok::<_, Infallible>(
        sse_event(name, event).into_sse(),
    )]))
    .into_response();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read SSE body");
    String::from_utf8(body.to_vec()).expect("SSE is UTF-8")
}

#[tokio::test]
async fn stream_event_names_match_frontend_listeners() {
    assert!(
        encoded_event_name(GLOBAL_EVENT_NAME)
            .await
            .contains("event: event\n")
    );
    assert!(
        encoded_event_name(GROUP_LEDGER_EVENT_NAME)
            .await
            .contains("event: ledger\n")
    );
    assert!(
        encoded_event_name(GROUP_LEDGER_EVENT_NAME)
            .await
            .contains("id: ")
    );
}

fn test_state(home: HomeLayout) -> AppState {
    let ledger_events = ledger_event_hub::LedgerEventHub::new(home.clone());
    AppState {
        client: DaemonClient::new(home.clone()),
        browser_surfaces: Arc::new(browser_surface::BrowserSurfaces::default()),
        connect_frames: Arc::new(crate::connect_frames::ConnectFrames::default()),
        connect_http: crate::connect_frames::http_client()
            .build()
            .map_err(|error| error.to_string()),
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
        home,
    }
}

#[tokio::test]
async fn last_event_id_replay_crosses_multiple_pages_without_gaps() {
    const REPLAY_COUNT: usize = 2_050;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("paged SSE replay", "").expect("group");
    let path = store.ledger_path(&group.group_id).expect("ledger path");
    let cursor = cccc_contracts::Event::new("chat.message", &group.group_id);
    ledger::append(&path, &cursor).expect("cursor");
    let expected = (0..REPLAY_COUNT)
        .map(|index| {
            let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
            event.data.insert("index".into(), serde_json::json!(index));
            ledger::append(&path, &event).expect("append replay event");
            event.id
        })
        .collect::<Vec<_>>();
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("last-event-id"),
        cursor.id.parse().expect("header value"),
    );
    let response = group_events(
        State(test_state(home)),
        Path(group.group_id),
        headers,
        Extension(local_principal()),
        Query(ResourceFrameQuery::default()),
    )
    .await
    .expect("group stream")
    .into_response();
    let mut body = response.into_body().into_data_stream();
    let mut received = Vec::with_capacity(REPLAY_COUNT);
    while received.len() < REPLAY_COUNT {
        let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .expect("SSE replay timeout")
            .expect("SSE body ended")
            .expect("SSE body chunk");
        let text = String::from_utf8(chunk.to_vec()).expect("SSE is UTF-8");
        received.extend(
            text.lines()
                .filter_map(|line| line.strip_prefix("id: "))
                .map(str::to_owned),
        );
    }
    assert_eq!(received, expected);
}

#[tokio::test]
async fn initial_replay_suppresses_events_already_queued_by_the_subscription() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("SSE replay race", "").expect("group");
    let path = store.ledger_path(&group.group_id).expect("ledger path");
    let cursor = cccc_contracts::Event::new("chat.message", &group.group_id);
    ledger::append(&path, &cursor).expect("cursor");
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("last-event-id"),
        cursor.id.parse().expect("header value"),
    );
    let response = group_events(
        State(test_state(home)),
        Path(group.group_id.clone()),
        headers,
        Extension(local_principal()),
        Query(ResourceFrameQuery::default()),
    )
    .await
    .expect("group stream")
    .into_response();
    let expected = (0..2)
        .map(|_| {
            let event = cccc_contracts::Event::new("chat.message", &group.group_id);
            ledger::append(&path, &event).expect("append queued event");
            event.id
        })
        .collect::<Vec<_>>();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let mut body = response.into_body().into_data_stream();
    let mut received = Vec::new();
    while received.len() < expected.len() {
        let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .expect("SSE replay timeout")
            .expect("SSE body ended")
            .expect("SSE body chunk");
        let text = String::from_utf8(chunk.to_vec()).expect("SSE is UTF-8");
        received.extend(
            text.lines()
                .filter_map(|line| line.strip_prefix("id: "))
                .map(str::to_owned),
        );
    }
    assert_eq!(received, expected);
    assert!(
        tokio::time::timeout(Duration::from_millis(150), body.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn reconnect_skips_stale_actor_activity_but_keeps_durable_and_live_events() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("SSE activity replay", "").expect("group");
    let path = store.ledger_path(&group.group_id).expect("ledger path");
    let cursor = cccc_contracts::Event::new("chat.message", &group.group_id);
    ledger::append(&path, &cursor).expect("cursor");

    let missed_activity = cccc_contracts::Event::new("actor.activity", &group.group_id);
    ledger::append(&path, &missed_activity).expect("missed activity");
    let durable_event = cccc_contracts::Event::new("chat.message", &group.group_id);
    ledger::append(&path, &durable_event).expect("durable event");

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("last-event-id"),
        cursor.id.parse().expect("header value"),
    );
    let response = group_events(
        State(test_state(home)),
        Path(group.group_id.clone()),
        headers,
        Extension(local_principal()),
        Query(ResourceFrameQuery::default()),
    )
    .await
    .expect("group stream")
    .into_response();
    let mut body = response.into_body().into_data_stream();
    let mut replayed = String::new();
    while !replayed.contains(&durable_event.id) {
        let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .expect("SSE replay timeout")
            .expect("SSE body ended")
            .expect("SSE body chunk");
        replayed.push_str(std::str::from_utf8(&chunk).expect("SSE is UTF-8"));
    }
    assert!(!replayed.contains(&missed_activity.id));

    let live_activity = cccc_contracts::Event::new("actor.activity", &group.group_id);
    ledger::append(&path, &live_activity).expect("live activity");
    let mut live = String::new();
    while !live.contains(&live_activity.id) {
        let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .expect("live SSE timeout")
            .expect("SSE body ended")
            .expect("SSE body chunk");
        live.push_str(std::str::from_utf8(&chunk).expect("SSE is UTF-8"));
    }
}
