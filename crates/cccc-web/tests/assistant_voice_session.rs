mod auth_support;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, assistant_state};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test]
async fn latest_document_session_aggregates_the_shared_transcript_log() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = initialized_home(temp.path());
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice transcript", "").expect("group");
    seed_document_transcript(&home, &group.group_id);
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/assistants/voice_secretary/sessions/latest?document_path=notes.md",
                group.group_id
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["result"]["session"]["source"], "document_transcript");
    assert_eq!(
        body["result"]["session"]["segments"],
        json!([
            {"schema":1,"document_id":"document-shared","document_path":"notes.md","session_id":"session-one","segment_id":"one","text":"first","is_final":true,"created_at":"2026-08-10T01:00:00Z","updated_at":"2026-08-10T01:00:00Z"},
            {"schema":1,"document_id":"document-shared","document_path":"notes.md","session_id":"session-two","segment_id":"two","text":"second","is_final":true,"created_at":"2026-08-10T02:00:00Z","updated_at":"2026-08-10T02:00:00Z"}
        ])
    );

    shutdown_daemon(&home).await;
    daemon.await.expect("daemon task").expect("daemon");
}

#[tokio::test]
async fn session_by_id_preserves_the_daemon_empty_session_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = initialized_home(temp.path());
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice session", "").expect("group");
    assistant_state::update(&home, &group.group_id, |state| {
        state.insert(
            "sessions".into(),
            json!([{
                "session_id":"session-known",
                "capture_mode":"document",
                "document_path":"notes.md",
                "segments":[{"segment_id":"one","text":"known transcript"}],
                "updated_at":"2026-08-10T01:00:00Z"
            }]),
        );
        Ok(())
    })
    .expect("seed assistant state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = auth_support::authenticated_app(home.clone());

    let known = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/assistants/voice_secretary/sessions/session-known",
                group.group_id
            ))
            .body(Body::empty())
            .expect("known request"),
        )
        .await
        .expect("known response");
    assert_eq!(known.status(), StatusCode::OK);
    assert_eq!(
        response_json(known).await["result"]["session"]["transcript"],
        "known transcript"
    );

    let missing = app
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/assistants/voice_secretary/sessions/session-missing",
                group.group_id
            ))
            .body(Body::empty())
            .expect("missing request"),
        )
        .await
        .expect("missing response");
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(response_json(missing).await["result"]["session"], json!({}));

    shutdown_daemon(&home).await;
    daemon.await.expect("daemon task").expect("daemon");
}

#[tokio::test]
async fn clearing_a_transcript_removes_shared_state_and_persisted_logs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = initialized_home(temp.path());
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice transcript", "").expect("group");
    let document_log = seed_document_transcript(&home, &group.group_id);
    assistant_state::update(&home, &group.group_id, |state| {
        state.insert(
            "sessions".into(),
            json!([
                {
                    "session_id":"session-old","capture_mode":"document",
                    "document_path":"notes.md","segments":[{"text":"old"}],
                    "transcript":"old","updated_at":"2026-08-10T01:00:00Z"
                },
                {
                    "session_id":"session-new","capture_mode":"document",
                    "document_path":"notes.md","segments":[{"text":"new"}],
                    "window_segments":[{"text":"python-window"}],
                    "transcript":"new","updated_at":"2026-08-10T02:00:00Z"
                }
            ]),
        );
        Ok(())
    })
    .expect("seed shared assistant state");
    let session_log = home
        .root()
        .join("voice-secretary")
        .join(&group.group_id)
        .join("session-new/transcripts/segments.jsonl");
    std::fs::create_dir_all(session_log.parent().expect("session log parent"))
        .expect("session transcript directory");
    std::fs::write(&session_log, "persisted session transcript\n").expect("session transcript");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    let response = auth_support::authenticated_app(home.clone())
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
    let body = response_json(response).await;
    assert_eq!(body["result"]["cleared"], true);
    assert_eq!(body["result"]["session_id"], "session-new");
    let state = assistant_state::load(&home, &group.group_id).expect("shared state");
    assert_eq!(state["sessions"][0]["transcript"], "old");
    assert_eq!(state["sessions"][1]["transcript"], "");
    assert_eq!(state["sessions"][1]["segments"], json!([]));
    assert_eq!(state["sessions"][1]["window_segments"], json!([]));
    assert!(!session_log.exists());
    assert!(!document_log.exists());
    assert!(
        groups
            .load(&group.group_id)
            .expect("group")
            .extra
            .get("assistants")
            .is_none()
    );

    shutdown_daemon(&home).await;
    daemon.await.expect("daemon task").expect("daemon");
}

fn initialized_home(root: &std::path::Path) -> HomeLayout {
    let home = HomeLayout::from_path(root.join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    home
}

fn seed_document_transcript(home: &HomeLayout, group_id: &str) -> PathBuf {
    let documents = home
        .root()
        .join("voice-secretary")
        .join(group_id)
        .join("documents");
    let document_log = documents.join("document-shared/transcript.jsonl");
    std::fs::create_dir_all(document_log.parent().expect("document log parent"))
        .expect("document directory");
    std::fs::write(
        documents.join("index.json"),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"group_id":group_id,"active_document_id":"document-shared",
            "documents":{"document-shared":{
                "document_id":"document-shared","document_path":"notes.md","status":"active"
            }}
        }))
        .expect("document index"),
    )
    .expect("write document index");
    std::fs::write(
        &document_log,
        concat!(
            "{\"schema\":1,\"document_id\":\"document-shared\",\"document_path\":\"notes.md\",\"session_id\":\"session-one\",\"segment_id\":\"one\",\"text\":\"first\",\"is_final\":true,\"created_at\":\"2026-08-10T01:00:00Z\",\"updated_at\":\"2026-08-10T01:00:00Z\"}\n",
            "{\"schema\":1,\"document_id\":\"document-shared\",\"document_path\":\"notes.md\",\"session_id\":\"session-two\",\"segment_id\":\"two\",\"text\":\"second\",\"is_final\":true,\"created_at\":\"2026-08-10T02:00:00Z\",\"updated_at\":\"2026-08-10T02:00:00Z\"}\n"
        ),
    )
    .expect("write transcript log");
    document_log
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("response body")
            .to_bytes(),
    )
    .expect("response JSON")
}

async fn wait_for_daemon(home: &HomeLayout) {
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Map::new(),
            })
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

async fn shutdown_daemon(home: &HomeLayout) {
    cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await
        .expect("shutdown daemon");
}
