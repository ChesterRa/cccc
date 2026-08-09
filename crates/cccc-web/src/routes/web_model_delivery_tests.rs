use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, integration_state};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;
use tokio::sync::broadcast;
use tokio::time::{Duration, Instant};
use tower::ServiceExt;

use super::web_model_delivery::IDLE_POLL_INTERVAL;
use super::web_model_delivery_test_support::{
    PromptPageBehavior, chrome_available, prompt_page, prompt_page_with,
};
use crate::browser_surface::SUBMISSION_EVIDENCE_TIMEOUT;

#[tokio::test]
async fn browser_session_rejects_non_web_model_actor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(&home, "group_create", json!({"title":"runtime guard"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer1","runtime":"codex","runner":"headless","role":"peer","by":"user"}),
    );
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, _, _) = crate::app_with_shutdown(
        home,
        shutdown,
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );

    let response = request_json(
        &app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=peer1"
        ))
        .body(Body::empty())
        .expect("browser request"),
    )
    .await;

    assert_eq!(
        response["error"]["message"],
        "ChatGPT browser sessions can only be bound to actors using runtime=web_model"
    );
}

#[tokio::test]
async fn browser_session_projects_the_shared_target_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(&home, "group_create", json!({"title":"shared target"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
    );
    let store = GroupStore::new(home.clone()).expect("group store");
    integration_state::group_update(
        &store,
        &group_id,
        super::web_model_browser::TARGETS_KEY,
        |value| {
            *value = json!({
                "web1":{
                    "state":"new_chat_submitted",
                    "kind":"new_chat",
                    "url":"https://chatgpt.com/",
                    "saved_at":"2026-08-07T00:00:00Z",
                    "submitted_at":"2026-08-07T00:00:01Z",
                    "delivery_id":"python-delivery",
                    "next_delivery":"wait_for_new_chat_bind",
                    "last_delivery_at":"2026-08-07T00:00:01Z",
                    "last_delivery_id":"python-delivery",
                    "last_delivery_turn_id":"python-turn",
                    "last_delivery_event_ids":["python-event"],
                    "last_delivery_status":"pending_new_chat_bind",
                    "last_submission_evidence":{
                        "submission_evidence":"message_echo",
                        "send_selector":"#composer-submit-button"
                    },
                    "last_error":"conversation_url_pending"
                }
            });
            Ok(())
        },
    )
    .expect("pending target");
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, _, _) = crate::app_with_shutdown(
        home,
        shutdown,
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );

    let pending = request_json(
        &app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=web1"
        ))
        .body(Body::empty())
        .expect("pending browser request"),
    )
    .await;
    assert_eq!(
        pending["result"]["browser_session"]["pending_new_chat_submitted"],
        true
    );
    assert_eq!(
        pending["result"]["browser_session"]["pending_new_chat_delivery_id"],
        "python-delivery"
    );
    assert_eq!(
        pending["result"]["health_snapshot"]["delivery"]["state"],
        "pending_bind"
    );

    integration_state::group_update(
        &store,
        &group_id,
        super::web_model_browser::TARGETS_KEY,
        |value| {
            *value = json!({
                "web1":{
                    "state":"bound_existing_chat",
                    "kind":"existing_chat",
                    "url":"https://chatgpt.com/c/from-python",
                    "saved_at":"2026-08-07T00:00:02Z",
                    "bound_at":"2026-08-07T00:00:02Z",
                    "next_delivery":"existing_chat",
                    "last_delivery_at":"2026-08-07T00:00:02Z",
                    "last_delivery_id":"python-delivery",
                    "last_delivery_turn_id":"python-turn",
                    "last_delivery_event_ids":["python-event"],
                    "last_delivery_status":"bound"
                }
            });
            Ok(())
        },
    )
    .expect("bound target");
    let bound = request_json(
        &app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=web1"
        ))
        .body(Body::empty())
        .expect("bound browser request"),
    )
    .await;
    assert_eq!(
        bound["result"]["browser_session"]["conversation_url"],
        "https://chatgpt.com/c/from-python"
    );
    assert_eq!(
        bound["result"]["health_snapshot"]["delivery"]["state"],
        "bound"
    );
    assert_eq!(
        bound["result"]["health_snapshot"]["delivery"]["cursor_committed"],
        true
    );

    integration_state::group_update(
        &store,
        &group_id,
        super::web_model_browser::TARGETS_KEY,
        |value| {
            value["web1"] = json!({
                "state":"bound_existing_chat",
                "kind":"existing_chat",
                "url":"https://chatgpt.com/c/WEB:temporary",
                "saved_at":"2026-08-07T00:00:03Z",
                "next_delivery":"existing_chat"
            });
            Ok(())
        },
    )
    .expect("provisional target");
    let invalid = request_json(
        &app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=web1"
        ))
        .body(Body::empty())
        .expect("invalid browser request"),
    )
    .await;
    assert_eq!(invalid["result"]["browser_session"]["conversation_url"], "");
    assert_eq!(
        invalid["result"]["browser_session"]["delivery_target"]["state"],
        "invalid_existing_chat"
    );
    assert_eq!(
        invalid["result"]["health_snapshot"]["target"]["state"],
        "invalid"
    );
}

#[tokio::test]
async fn browser_session_inspection_does_not_deliver_or_commit_messages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(&home, "group_create", json!({"title":"read only inspect"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
    );
    daemon_sync(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    daemon_sync(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["web1"],"text":"must remain unread"}),
    );
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, _, _) = crate::app_with_shutdown(
        home.clone(),
        shutdown,
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );

    let inspected = request_json(
        &app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=web1&inspect=true"
        ))
        .body(Body::empty())
        .expect("inspect browser request"),
    )
    .await;
    let turn = daemon_sync(
        &home,
        "web_model_runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1"}),
    );

    assert_eq!(inspected["ok"], true);
    assert_eq!(turn["status"], "work_available");
    assert!(
        turn["turn"]["coalesced_text"]
            .as_str()
            .is_some_and(|text| text.contains("must remain unread"))
    );
}

#[tokio::test]
async fn cached_browser_status_does_not_reinspect_the_page() {
    if !chrome_available() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(&home, "group_create", json!({"title":"cached inspect"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
    );
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, surfaces, _) = crate::app_with_shutdown(
        home,
        shutdown,
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );
    let (page_url, page_server) = prompt_page().await;
    let session_key = super::web_model_browser::key(&group_id, "web1");
    surfaces
        .ensure_open(
            &session_key,
            &temp.path().join("profile"),
            &page_url,
            900,
            700,
        )
        .await
        .expect("browser surface");

    let inspected = request_json(
        &app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=web1&inspect=true"
        ))
        .body(Body::empty())
        .expect("inspect browser request"),
    )
    .await;
    let page = surfaces
        .sessions
        .lock()
        .await
        .get(&session_key)
        .expect("session")
        .page
        .clone();
    page.evaluate("document.querySelector('#prompt-textarea').remove()")
        .await
        .expect("remove composer");
    let cached = request_json(
        &app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=web1&inspect=false"
        ))
        .body(Body::empty())
        .expect("cached browser request"),
    )
    .await;
    let refreshed = request_json(
        &app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=web1&inspect=true"
        ))
        .body(Body::empty())
        .expect("refresh browser request"),
    )
    .await;
    let _ = surfaces.close(&session_key).await;
    page_server.abort();

    assert_eq!(inspected["result"]["browser_session"]["ready"], true);
    assert_eq!(cached["result"]["browser_session"]["ready"], true);
    assert_eq!(refreshed["result"]["browser_session"]["ready"], false);
}

#[tokio::test]
async fn delivery_preference_persists_through_rust_web_and_daemon_turns() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(
        &home,
        "group_create",
        json!({"title":"delivery preference"}),
    );
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
    );
    daemon_sync(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, _, _) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );

    let updated = request_json(
        &app,
        Request::post("/api/v1/web-model/browser-session/delivery-preference")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","mode":"image_compat"}).to_string(),
            ))
            .expect("preference request"),
    )
    .await;
    let client = DaemonClient::new(home.clone());
    client
        .call(&request(
            "send",
            json!({"group_id":group_id,"by":"user","to":["web1"],"text":"mode snapshot"}),
        ))
        .await
        .expect("send");
    let wait = client
        .call(&request(
            "web_model_runtime_wait_next_turn",
            json!({"group_id":group_id,"actor_id":"web1"}),
        ))
        .await
        .expect("wait turn");
    let _ = shutdown.send(());
    daemon.abort();
    let _ = daemon.await;

    assert_eq!(
        updated["result"]["browser_session"]["delivery_mode"],
        "image_compat"
    );
    assert_eq!(
        wait.result["turn"]["delivery"]["web_model_mode"],
        "image_compat"
    );
}

#[tokio::test]
async fn connector_mcp_uses_its_bound_actor_for_listing_and_calls() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(&home, "group_create", json!({"title":"connector scope"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "attach",
        json!({"group_id":group_id,"path":temp.path(),"by":"user"}),
    );
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
    );
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, _, _) = crate::app_with_shutdown(
        home,
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );

    let create = request_json(
        &app,
        Request::post("/api/v1/web-model/connectors")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","provider":"chatgpt"}).to_string(),
            ))
            .expect("create connector request"),
    )
    .await;
    let connector_id = create["result"]["connector"]["connector_id"]
        .as_str()
        .expect("connector id");
    let secret = create["result"]["secret"].as_str().expect("secret");
    let endpoint = format!("/mcp/web-model/{connector_id}?token={secret}");
    let listed = request_json(
        &app,
        Request::post(&endpoint)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}).to_string(),
            ))
            .expect("list tools request"),
    )
    .await;
    let names = listed["result"]["tools"]
        .as_array()
        .expect("tool list")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let bootstrap = request_json(
        &app,
        Request::post(&endpoint)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "jsonrpc":"2.0",
                    "id":2,
                    "method":"tools/call",
                    "params":{
                        "name":"cccc_bootstrap",
                        "arguments":{"actor_id":"user","by":"user"}
                    }
                })
                .to_string(),
            ))
            .expect("bootstrap request"),
    )
    .await;
    let code_exec = request_json(
        &app,
        Request::post(&endpoint)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "jsonrpc":"2.0",
                    "id":3,
                    "method":"tools/call",
                    "params":{
                        "name":"cccc_code_exec",
                        "arguments":{
                            "source":"text(String(ALL_TOOLS.some((tool) => tool.raw_name === 'cccc_repo')));",
                            "yield_time_ms":5000
                        }
                    }
                })
                .to_string(),
            ))
            .expect("code mode request"),
    )
    .await;
    let forbidden_status = app
        .clone()
        .oneshot(
            Request::post(&endpoint)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "jsonrpc":"2.0",
                        "id":4,
                        "method":"tools/call",
                        "params":{
                            "name":"cccc_bootstrap",
                            "arguments":{"group_id":"another-group"}
                        }
                    })
                    .to_string(),
                ))
                .expect("cross-group request"),
        )
        .await
        .expect("cross-group response")
        .status();
    let _ = shutdown.send(());
    daemon.abort();
    let _ = daemon.await;

    assert_eq!(
        names.len(),
        31,
        "unexpected Web Model tool surface: {names:?}"
    );
    assert!(names.contains("cccc_code_exec"));
    assert!(names.contains("cccc_code_wait"));
    assert!(names.contains("cccc_repo"));
    assert_eq!(
        bootstrap["result"]["structuredContent"]["session"]["actor_id"],
        "web1"
    );
    assert_eq!(
        code_exec["result"]["structuredContent"]["status"],
        "completed"
    );
    assert_eq!(code_exec["result"]["structuredContent"]["output"], "true");
    assert_eq!(forbidden_status, StatusCode::FORBIDDEN);
}

const BACKGROUND_DELIVERY_MARGIN: Duration = Duration::from_secs(5);
const DELIVERY_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[tokio::test]
async fn connector_activity_binding_and_browser_delivery_share_one_turn() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(&home, "group_create", json!({"title":"web model e2e"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
    );
    daemon_sync(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, surfaces, _) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );
    let (page_url, page_server) = prompt_page().await;
    surfaces
        .open(
            &super::web_model_browser::key(&group_id, "web1"),
            &temp.path().join("profile"),
            &page_url,
            800,
            600,
        )
        .await
        .expect("browser surface");

    let create = request_json(
        &app,
        Request::post("/api/v1/web-model/connectors")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","provider":"chatgpt"}).to_string(),
            ))
            .expect("create request"),
    )
    .await;
    let connector_id = create["result"]["connector"]["connector_id"]
        .as_str()
        .expect("connector id");
    let secret = create["result"]["secret"].as_str().expect("secret");
    let probe_status = app
        .clone()
        .oneshot(
            Request::get(format!("/mcp/web-model/{connector_id}?token={secret}"))
                .body(Body::empty())
                .expect("probe request"),
        )
        .await
        .expect("probe")
        .status();
    let armed = request_json(
        &app,
        Request::post("/api/v1/web-model/browser-session/bind-current")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","conversation_url":page_url,"new_chat":true}).to_string(),
            ))
            .expect("arm new chat request"),
    )
    .await;
    let bind = request_json(
        &app,
        Request::post("/api/v1/web-model/browser-session/bind-current")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","conversation_url":page_url})
                    .to_string(),
            ))
            .expect("bind request"),
    )
    .await;
    arm_local_new_chat(&home, &group_id, "web1", &page_url);
    let client = DaemonClient::new(home.clone());
    let sent = client
        .call(&request(
            "send",
            json!({"group_id":group_id,"by":"user","to":["web1"],"text":"hello browser"}),
        ))
        .await
        .expect("send");
    let inspected = wait_for_background_delivery(&app, &group_id, "web1").await;
    let page = surfaces
        .sessions
        .lock()
        .await
        .get(&super::web_model_browser::key(&group_id, "web1"))
        .expect("session")
        .page
        .clone();
    let submitted: String = page
        .evaluate("globalThis.submitted || ''")
        .await
        .expect("submitted value")
        .into_value()
        .expect("submitted string");
    let connectors = request_json(
        &app,
        Request::get("/api/v1/web-model/connectors")
            .body(Body::empty())
            .expect("list request"),
    )
    .await;
    let idle = client
        .call(&request(
            "web_model_runtime_wait_next_turn",
            json!({"group_id":group_id,"actor_id":"web1"}),
        ))
        .await
        .expect("completed runtime turn");
    let _ = shutdown.send(());
    let _ = surfaces
        .close(&super::web_model_browser::key(&group_id, "web1"))
        .await;
    daemon.abort();
    let _ = daemon.await;
    page_server.abort();

    assert_eq!(probe_status, StatusCode::OK);
    assert!(sent.ok);
    assert_eq!(
        armed["result"]["browser_session"]["delivery_target"]["kind"],
        "new_chat"
    );
    assert_eq!(
        armed["result"]["browser_session"]["delivery_target"]["url"],
        "https://chatgpt.com/"
    );
    assert_eq!(
        bind["result"]["browser_session"]["delivery_target"]["kind"],
        "existing_chat"
    );
    assert!(submitted.contains("hello browser"), "{submitted}");
    assert!(
        submitted.contains("[CCCC] Session bootstrap for this browser chat:"),
        "{submitted}"
    );
    assert!(submitted.contains("[CCCC] You are web1"), "{submitted}");
    assert!(submitted.contains("[CCCC] Web transport:"), "{submitted}");
    assert!(
        submitted.contains("[cccc] Browser batch webdelivery:web1:"),
        "{submitted}"
    );
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["last_delivery_status"],
        "submitted"
    );
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["kind"],
        "existing_chat"
    );
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["url"],
        format!("{page_url}/c/test-conversation")
    );
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["bootstrap_seed_version"],
        "web-model-bootstrap-normal-system-prompt-v2"
    );
    assert!(
        inspected["result"]["browser_session"]["delivery_target"]["bootstrap_seed_digest"]
            .as_str()
            .is_some_and(|digest| !digest.is_empty())
    );
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["bootstrap_seed_conversation_url"],
        format!("{page_url}/c/test-conversation")
    );
    assert_eq!(
        inspected["result"]["browser_session"]["pending_new_chat_bind"],
        false
    );
    assert_eq!(inspected["result"]["browser_session"]["ready"], true);
    assert_eq!(
        inspected["result"]["browser_session"]["last_delivery_status"],
        "submitted"
    );
    assert_eq!(
        inspected["result"]["browser_session"]["last_submission_evidence"],
        "message_echo"
    );
    assert_eq!(
        inspected["result"]["health_snapshot"]["target"]["state"],
        "bound"
    );
    assert_eq!(
        inspected["result"]["health_snapshot"]["delivery"]["state"],
        "submitted"
    );
    assert_eq!(
        inspected["result"]["health_snapshot"]["delivery"]["cursor_committed"],
        true
    );
    assert_eq!(
        connectors["result"]["connectors"][0]["last_call_status"],
        "submitted"
    );
    assert!(idle.ok);
    assert_eq!(idle.result["status"], "idle");
}

#[tokio::test]
async fn visible_stop_control_defers_without_clicking_or_claiming_submission() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = crate::browser_surface::BrowserSurfaces::default();
    let (page_url, page_server) = prompt_page_with(PromptPageBehavior::StopOnly).await;
    manager
        .open(
            "stop-only",
            &temp.path().join("profile"),
            &page_url,
            800,
            600,
        )
        .await
        .expect("browser surface");

    let outcome = manager
        .submit_prompt_with_attachment(
            "stop-only",
            &page_url,
            "wait for the current response",
            None,
            "",
        )
        .await
        .expect("deferred submission outcome");
    let evidence = match outcome {
        crate::browser_surface::PromptSubmissionOutcome::Deferred(evidence) => evidence,
        crate::browser_surface::PromptSubmissionOutcome::Verified(_) => {
            panic!("stop control must not be treated as a verified submission")
        }
        crate::browser_surface::PromptSubmissionOutcome::Ambiguous(_) => {
            panic!("stop control should be recognized before attempting submission")
        }
    };
    let page = manager
        .sessions
        .lock()
        .await
        .get("stop-only")
        .expect("session")
        .page
        .clone();
    let stop_clicks: i64 = page
        .evaluate("globalThis.stopClicks || 0")
        .await
        .expect("stop click count")
        .into_value()
        .expect("stop click number");
    let _ = manager.close("stop-only").await;
    page_server.abort();

    assert_eq!(evidence["submitted"], false);
    assert_eq!(evidence["submission_evidence"], "send_control_deferred");
    assert_eq!(stop_clicks, 0);
}

#[tokio::test]
async fn externally_accepted_prompt_is_not_deferred_for_automatic_retry() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = crate::browser_surface::BrowserSurfaces::default();
    let (page_url, page_server) = prompt_page_with(PromptPageBehavior::AutoSubmitThenStop).await;
    manager
        .open(
            "auto-submit",
            &temp.path().join("profile"),
            &page_url,
            800,
            600,
        )
        .await
        .expect("browser surface");

    let prompt =
        "[cccc] Browser batch webdelivery:web1:auto events=0123456789abcdef actor=web1\nhello";
    let outcome = manager
        .submit_prompt_with_attachment("auto-submit", &page_url, prompt, None, "")
        .await
        .expect("submission outcome");
    let evidence = match outcome {
        crate::browser_surface::PromptSubmissionOutcome::Verified(evidence) => evidence,
        crate::browser_surface::PromptSubmissionOutcome::Ambiguous(evidence) => {
            panic!("matching external submission should be verified: {evidence}")
        }
        crate::browser_surface::PromptSubmissionOutcome::Deferred(evidence) => {
            panic!("accepted prompt must not be automatically retryable: {evidence}")
        }
    };
    let page = manager
        .sessions
        .lock()
        .await
        .get("auto-submit")
        .expect("session")
        .page
        .clone();
    let send_clicks: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send click count")
        .into_value()
        .expect("send click number");
    let _ = manager.close("auto-submit").await;
    page_server.abort();

    assert_eq!(evidence["submission_evidence"], "message_echo");
    assert_eq!(send_clicks, 1);
}

#[tokio::test]
async fn compatibility_image_is_attached_once_before_verified_submission() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let image = temp.path().join("cccc-mcp-compat.png");
    std::fs::write(&image, b"test image bytes").expect("test image");
    let manager = crate::browser_surface::BrowserSurfaces::default();
    let (page_url, page_server) = prompt_page().await;
    manager
        .open(
            "attachment",
            &temp.path().join("profile"),
            &page_url,
            800,
            600,
        )
        .await
        .expect("browser surface");

    let prompt = "[cccc] Browser batch webdelivery:web1:attachment events=one actor=web1\nhello";
    let first = manager
        .submit_prompt_with_attachment(
            "attachment",
            &page_url,
            prompt,
            Some(&image),
            "webdelivery:web1:attachment",
        )
        .await
        .expect("verified attachment submission");
    assert!(matches!(
        first,
        crate::browser_surface::PromptSubmissionOutcome::Verified(_)
    ));
    let second = manager
        .submit_prompt_with_attachment(
            "attachment",
            &page_url,
            prompt,
            Some(&image),
            "webdelivery:web1:attachment",
        )
        .await
        .expect("idempotent submission lookup");
    assert!(matches!(
        second,
        crate::browser_surface::PromptSubmissionOutcome::Verified(_)
    ));
    let page = manager
        .sessions
        .lock()
        .await
        .get("attachment")
        .expect("session")
        .page
        .clone();
    let submitted_files: Vec<String> = page
        .evaluate("globalThis.submittedFiles || []")
        .await
        .expect("submitted files")
        .into_value()
        .expect("submitted file list");
    let send_clicks: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send click count")
        .into_value()
        .expect("send click number");
    let _ = manager.close("attachment").await;
    page_server.abort();

    assert_eq!(submitted_files, vec!["cccc-mcp-compat.png"]);
    assert_eq!(send_clicks, 1);
}

#[tokio::test]
async fn compatibility_image_accepts_current_generic_preview_after_file_input_clears() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let image = temp.path().join("cccc-mcp-compat-current.png");
    std::fs::write(&image, b"test image bytes").expect("test image");
    let manager = crate::browser_surface::BrowserSurfaces::default();
    let (page_url, page_server) =
        prompt_page_with(PromptPageBehavior::AttachmentPreviewClearsInput).await;
    manager
        .open(
            "attachment-preview",
            &temp.path().join("profile"),
            &page_url,
            800,
            600,
        )
        .await
        .expect("browser surface");

    let prompt = "[cccc] Browser batch webdelivery:web1:preview events=one actor=web1\nhello";
    let outcome = manager
        .submit_prompt_with_attachment(
            "attachment-preview",
            &page_url,
            prompt,
            Some(&image),
            "webdelivery:web1:preview",
        )
        .await
        .expect("verified attachment submission");
    assert!(matches!(
        outcome,
        crate::browser_surface::PromptSubmissionOutcome::Verified(_)
    ));
    let page = manager
        .sessions
        .lock()
        .await
        .get("attachment-preview")
        .expect("session")
        .page
        .clone();
    let upload_attempts: i64 = page
        .evaluate("globalThis.uploadAttempts || 0")
        .await
        .expect("upload attempts")
        .into_value()
        .expect("upload attempt number");
    let send_clicks: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send clicks")
        .into_value()
        .expect("send click number");
    let submitted_files: Vec<String> = page
        .evaluate("globalThis.submittedFiles || []")
        .await
        .expect("submitted files")
        .into_value()
        .expect("submitted file list");
    let _ = manager.close("attachment-preview").await;
    page_server.abort();

    assert_eq!(upload_attempts, 1);
    assert_eq!(send_clicks, 1);
    assert_eq!(submitted_files, vec!["cccc-mcp-compat-current.png"]);
}

#[tokio::test]
async fn duplicate_compatibility_dialog_recovers_without_reuploading_the_draft() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let image = temp.path().join("cccc-mcp-compat-recovery.png");
    std::fs::write(&image, b"test image bytes").expect("test image");
    let manager = crate::browser_surface::BrowserSurfaces::default();
    let (page_url, page_server) =
        prompt_page_with(PromptPageBehavior::DuplicateAttachmentDialog).await;
    manager
        .open(
            "attachment-dialog",
            &temp.path().join("profile"),
            &page_url,
            800,
            600,
        )
        .await
        .expect("browser surface");

    let prompt = "[cccc] Browser batch webdelivery:web1:recovery events=one actor=web1\nhello";
    let outcome = manager
        .submit_prompt_with_attachment(
            "attachment-dialog",
            &page_url,
            prompt,
            Some(&image),
            "webdelivery:web1:recovery",
        )
        .await
        .expect("recovered attachment submission");
    assert!(matches!(
        outcome,
        crate::browser_surface::PromptSubmissionOutcome::Verified(_)
    ));
    let page = manager
        .sessions
        .lock()
        .await
        .get("attachment-dialog")
        .expect("session")
        .page
        .clone();
    let upload_attempts: i64 = page
        .evaluate("globalThis.uploadAttempts || 0")
        .await
        .expect("upload attempts")
        .into_value()
        .expect("upload attempt number");
    let dismissals: i64 = page
        .evaluate("globalThis.dialogDismissals || 0")
        .await
        .expect("dialog dismissals")
        .into_value()
        .expect("dialog dismissal number");
    let send_clicks: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send clicks")
        .into_value()
        .expect("send click number");
    let _ = manager.close("attachment-dialog").await;
    page_server.abort();

    assert_eq!(dismissals, 1);
    assert_eq!(upload_attempts, 0);
    assert_eq!(send_clicks, 1);
}

#[tokio::test]
async fn ambiguous_browser_submission_commits_at_most_once_without_claiming_success() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(
        &home,
        "group_create",
        json!({"title":"ambiguous web model delivery"}),
    );
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
    );
    daemon_sync(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, surfaces, _) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );
    let (page_url, page_server) = prompt_page_with(PromptPageBehavior::IgnoreSend).await;
    let session_key = super::web_model_browser::key(&group_id, "web1");
    surfaces
        .open(
            &session_key,
            &temp.path().join("profile"),
            &page_url,
            800,
            600,
        )
        .await
        .expect("browser surface");
    request_json(
        &app,
        Request::post("/api/v1/web-model/connectors")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","provider":"chatgpt_web"}).to_string(),
            ))
            .expect("create connector request"),
    )
    .await;
    request_json(
        &app,
        Request::post("/api/v1/web-model/browser-session/bind-current")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","conversation_url":page_url})
                    .to_string(),
            ))
            .expect("bind request"),
    )
    .await;
    let client = DaemonClient::new(home.clone());
    let sent = client
        .call(&request(
            "send",
            json!({"group_id":group_id,"by":"user","to":["web1"],"text":"at most once prompt"}),
        ))
        .await
        .expect("send");

    let inspected = wait_for_delivery_status(&app, &group_id, "web1", "submission_ambiguous").await;
    let page = surfaces
        .sessions
        .lock()
        .await
        .get(&session_key)
        .expect("session")
        .page
        .clone();
    let send_clicks: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send click count")
        .into_value()
        .expect("send click number");
    let runtime_after = client
        .call(&request(
            "web_model_runtime_wait_next_turn",
            json!({"group_id":group_id,"actor_id":"web1"}),
        ))
        .await
        .expect("runtime state after ambiguous submission");
    tokio::time::sleep(std::time::Duration::from_millis(5_200)).await;
    let send_clicks_after_retry_window: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send click count after retry window")
        .into_value()
        .expect("send click number after retry window");
    let _ = shutdown.send(());
    let _ = surfaces.close(&session_key).await;
    daemon.abort();
    let _ = daemon.await;
    page_server.abort();

    assert!(sent.ok);
    assert_eq!(send_clicks, 1);
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["last_delivery_status"],
        "submission_ambiguous"
    );
    assert_eq!(
        inspected["result"]["browser_session"]["last_delivery_status"],
        "ambiguous"
    );
    assert_eq!(
        inspected["result"]["browser_session"]["last_submission_evidence"],
        "submission_verification_ambiguous"
    );
    assert_eq!(
        inspected["result"]["health_snapshot"]["delivery"]["state"],
        "ambiguous"
    );
    assert_eq!(
        inspected["result"]["health_snapshot"]["delivery"]["cursor_committed"],
        true
    );
    assert_eq!(
        inspected["result"]["health_snapshot"]["next_action"]["recommended"],
        "inspect_error"
    );
    assert!(runtime_after.ok);
    assert_eq!(runtime_after.result["status"], "idle");
    assert_eq!(send_clicks_after_retry_window, 1);
}

#[tokio::test]
async fn interrupted_dispatch_is_fenced_and_later_turns_still_deliver() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let fixture = LegacyPendingFixture::open_interrupted().await;
    let interrupted = wait_for_delivery_status(
        &fixture.app,
        &fixture.group_id,
        "web1",
        "submission_ambiguous",
    )
    .await;
    let page = fixture.page().await;
    let clicks_after_recovery: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send clicks after interrupted recovery")
        .into_value()
        .expect("send click count");
    let client = DaemonClient::new(fixture.home.clone());
    let second = client
        .call(&request(
            "send",
            json!({"group_id":fixture.group_id,"by":"user","to":["web1"],"text":"later turn still delivers"}),
        ))
        .await
        .expect("send later turn");
    let delivered =
        wait_for_delivery_status(&fixture.app, &fixture.group_id, "web1", "submitted").await;
    let submitted: String = page
        .evaluate("globalThis.submitted || ''")
        .await
        .expect("submitted later prompt")
        .into_value()
        .expect("submitted prompt string");

    assert_eq!(clicks_after_recovery, 0);
    assert_eq!(
        interrupted["result"]["browser_session"]["delivery_target"]["last_submission_evidence"]["submission_evidence"],
        "interrupted_dispatch"
    );
    assert!(second.ok);
    assert!(
        submitted.contains("later turn still delivers"),
        "{submitted}"
    );
    assert!(!submitted.contains("legacy pending task"), "{submitted}");
    assert_eq!(
        delivered["result"]["browser_session"]["delivery_target"]["last_delivery_status"],
        "submitted"
    );
    fixture.close().await;
}

#[tokio::test]
async fn persisted_direct_submission_evidence_recovers_and_binds_new_chat() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let fixture = LegacyPendingFixture::open_verified_ambiguous().await;
    let inspected =
        wait_for_delivery_status(&fixture.app, &fixture.group_id, "web1", "submitted").await;
    let page = fixture.page().await;
    let send_clicks: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send click count")
        .into_value()
        .expect("send click count number");
    let target = &inspected["result"]["browser_session"]["delivery_target"];

    assert_eq!(send_clicks, 0);
    assert_eq!(target["last_delivery_status"], "submitted");
    assert_eq!(target["kind"], "existing_chat");
    assert_eq!(
        target["url"],
        format!("{}/c/recovered-conversation", fixture.page_url)
    );
    assert_eq!(
        target["last_submission_evidence"]["submission_evidence"],
        "user_message_count_increased"
    );
    assert_eq!(
        target["last_submission_evidence"]["recovered_from"],
        "submission_ambiguous"
    );
    assert_eq!(
        target["bootstrap_seed_conversation_url"],
        format!("{}/c/recovered-conversation", fixture.page_url)
    );
    fixture.close().await;
}

#[tokio::test]
async fn legacy_pending_new_chat_recovers_only_when_the_staged_prompt_matches() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let fixture = LegacyPendingFixture::open(PromptPageBehavior::LegacyStaged).await;
    let inspected =
        wait_for_delivery_status(&fixture.app, &fixture.group_id, "web1", "submitted").await;
    let page = fixture.page().await;
    let submitted: String = page
        .evaluate("globalThis.submitted || ''")
        .await
        .expect("submitted prompt")
        .into_value()
        .expect("submitted prompt string");
    let send_clicks: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send click count")
        .into_value()
        .expect("send click count number");

    assert_eq!(send_clicks, 1);
    assert!(submitted.contains("[CCCC] Session bootstrap for this browser chat:"));
    assert!(submitted.contains("[cccc] Browser batch webdelivery:web1:"));
    assert!(submitted.contains("legacy pending task"));
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["last_delivery_status"],
        "submitted"
    );
    fixture.close().await;
}

#[tokio::test]
async fn legacy_pending_new_chat_pauses_when_the_staged_prompt_was_edited() {
    if !chrome_available() {
        return;
    }
    let _browser_test = browser_test_lock().lock().await;
    let fixture = LegacyPendingFixture::open(PromptPageBehavior::LegacyEdited).await;
    let inspected = wait_for_delivery_status(
        &fixture.app,
        &fixture.group_id,
        "web1",
        "legacy_submission_unverified",
    )
    .await;
    let page = fixture.page().await;
    let send_clicks: i64 = page
        .evaluate("globalThis.sendClicks || 0")
        .await
        .expect("send click count")
        .into_value()
        .expect("send click count number");

    assert_eq!(send_clicks, 0);
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["last_delivery_status"],
        "legacy_submission_unverified"
    );
    fixture.close().await;
}

struct LegacyPendingFixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    app: axum::Router,
    group_id: String,
    page_url: String,
    session_key: String,
    surfaces: std::sync::Arc<crate::browser_surface::BrowserSurfaces>,
    shutdown: broadcast::Sender<()>,
    daemon: tokio::task::JoinHandle<anyhow::Result<()>>,
    page_server: tokio::task::JoinHandle<()>,
}

#[derive(Clone, Copy)]
enum PersistedDeliveryState {
    LegacyPending,
    VerifiedAmbiguous,
    InterruptedSubmitting,
}

impl LegacyPendingFixture {
    async fn open(behavior: PromptPageBehavior) -> Self {
        Self::open_with_state(behavior, PersistedDeliveryState::LegacyPending).await
    }

    async fn open_verified_ambiguous() -> Self {
        Self::open_with_state(
            PromptPageBehavior::IgnoreSend,
            PersistedDeliveryState::VerifiedAmbiguous,
        )
        .await
    }

    async fn open_interrupted() -> Self {
        Self::open_with_state(
            PromptPageBehavior::Submit,
            PersistedDeliveryState::InterruptedSubmitting,
        )
        .await
    }

    async fn open_with_state(
        behavior: PromptPageBehavior,
        persisted_state: PersistedDeliveryState,
    ) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let created = daemon_sync(
            &home,
            "group_create",
            json!({"title":"legacy pending delivery"}),
        );
        let group_id = created["group"]["group_id"]
            .as_str()
            .expect("group id")
            .to_owned();
        daemon_sync(
            &home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
        );
        daemon_sync(
            &home,
            "actor_start",
            json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
        );
        let sent = daemon_sync(
            &home,
            "send",
            json!({"group_id":group_id,"by":"user","to":["web1"],"text":"legacy pending task"}),
        );
        let wait = daemon_sync(
            &home,
            "web_model_runtime_wait_next_turn",
            json!({"group_id":group_id,"actor_id":"web1"}),
        );
        let turn = wait["turn"].clone();
        if !matches!(
            persisted_state,
            PersistedDeliveryState::InterruptedSubmitting
        ) {
            daemon_sync(
                &home,
                "web_model_runtime_complete_turn",
                json!({
                    "group_id":group_id,
                    "actor_id":"web1",
                    "by":"web1",
                    "turn_id":turn["turn_id"],
                    "event_ids":turn["event_ids"],
                    "delivery_id":"wmd_legacy_pending",
                    "status":"done"
                }),
            );
        }
        let (page_url, page_server) = prompt_page_with(behavior).await;
        let recovered_url = format!("{page_url}/c/recovered-conversation");
        let store = GroupStore::new(home.clone()).expect("group store");
        integration_state::group_update(
            &store,
            &group_id,
            super::web_model_browser::TARGETS_KEY,
            |value| {
                if !value.is_object() {
                    *value = json!({});
                }
                let target = match persisted_state {
                    PersistedDeliveryState::LegacyPending => json!({
                        "state":"new_chat_armed",
                        "kind":"new_chat",
                        "url":page_url,
                        "last_delivery_status":"pending_new_chat_bind",
                        "last_delivery_id":"wmd_legacy_pending",
                        "last_delivery_turn_id":turn["turn_id"],
                        "last_delivery_event_ids":turn["event_ids"],
                        "last_submission_evidence":{"submitted":true,"tab_url":page_url},
                        "last_error":"conversation_url_pending"
                    }),
                    PersistedDeliveryState::VerifiedAmbiguous => json!({
                        "state":"new_chat_armed",
                        "kind":"new_chat",
                        "url":page_url,
                        "last_delivery_status":"submission_ambiguous",
                        "last_delivery_id":"webdelivery:web1:persisted",
                        "last_delivery_turn_id":turn["turn_id"],
                        "last_delivery_event_ids":turn["event_ids"],
                        "last_submission_evidence":{
                            "submitted":false,
                            "submission_evidence":"user_message_count_increased",
                            "baseline":{"url":page_url,"user_message_count":0,"composer_contains_prompt":true},
                            "observed":{"url":recovered_url,"user_message_count":1,"composer_contains_prompt":false}
                        },
                        "last_error":"browser submission was attempted but could not be verified"
                    }),
                    PersistedDeliveryState::InterruptedSubmitting => json!({
                        "state":"bound_existing_chat",
                        "kind":"existing_chat",
                        "url":page_url,
                        "last_delivery_status":"submitting",
                        "last_delivery_id":"webdelivery:web1:interrupted",
                        "last_delivery_turn_id":turn["turn_id"],
                        "last_delivery_event_ids":turn["event_ids"],
                        "last_delivery_started_at":"2026-08-09T00:00:00Z",
                        "last_submission_evidence":{},
                        "last_error":""
                    }),
                };
                value
                    .as_object_mut()
                    .expect("target map")
                    .insert("web1".into(), target);
                Ok(())
            },
        )
        .expect("legacy target");
        assert!(sent["event"]["id"].as_str().is_some());
        let daemon_home = home.clone();
        let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        wait_for_daemon(&home).await;
        let (shutdown, _) = broadcast::channel(2);
        let (app, _, surfaces, _) = crate::app_with_shutdown(
            home.clone(),
            shutdown.clone(),
            crate::WebMode::Normal,
            None,
            crate::LiveBinding::from_env(),
        );
        let session_key = super::web_model_browser::key(&group_id, "web1");
        request_json(
            &app,
            Request::post("/api/v1/web-model/connectors")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"group_id":group_id,"actor_id":"web1","provider":"chatgpt_web"})
                        .to_string(),
                ))
                .expect("create connector request"),
        )
        .await;
        let opened = request_json(
            &app,
            Request::post("/api/v1/web-model/browser-session/open")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"group_id":group_id,"actor_id":"web1","width":800,"height":600})
                        .to_string(),
                ))
                .expect("browser open request"),
        )
        .await;
        assert_eq!(
            opened["result"]["browser_session"]["active"], true,
            "{opened}"
        );
        Self {
            _temp: temp,
            home,
            app,
            group_id,
            page_url,
            session_key,
            surfaces,
            shutdown,
            daemon,
            page_server,
        }
    }

    async fn page(&self) -> chromiumoxide::Page {
        self.surfaces
            .sessions
            .lock()
            .await
            .get(&self.session_key)
            .expect("session")
            .page
            .clone()
    }

    async fn close(self) {
        let _ = self.shutdown.send(());
        let _ = self.surfaces.close(&self.session_key).await;
        self.daemon.abort();
        let _ = self.daemon.await;
        self.page_server.abort();
    }
}

fn daemon_sync(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = cccc_daemon::handle_request(home, &request(op, args));
    assert!(response.ok, "{:?}", response.error);
    Value::Object(response.result)
}

fn browser_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn arm_local_new_chat(home: &HomeLayout, group_id: &str, actor_id: &str, url: &str) {
    let store = GroupStore::new(home.clone()).expect("group store");
    integration_state::group_update(
        &store,
        group_id,
        super::web_model_browser::TARGETS_KEY,
        |value| {
            if !value.is_object() {
                *value = json!({});
            }
            value.as_object_mut().expect("target map").insert(
                actor_id.to_owned(),
                json!({
                    "state":"new_chat_armed",
                    "kind":"new_chat",
                    "url":url,
                    "saved_at":cccc_contracts::utc_now(),
                    "next_delivery":"new_chat"
                }),
            );
            Ok(())
        },
    )
    .expect("arm local new chat target");
}

fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    }
}

async fn request_json(app: &axum::Router, request: Request<Body>) -> Value {
    let response = app.clone().oneshot(request).await.expect("response");
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("json")
}

async fn wait_for_background_delivery(app: &axum::Router, group_id: &str, actor_id: &str) -> Value {
    wait_for_delivery_status(app, group_id, actor_id, "submitted").await;
    request_json(
        app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id={actor_id}&inspect=true"
        ))
        .body(Body::empty())
        .expect("inspected browser state request"),
    )
    .await
}

async fn wait_for_delivery_status(
    app: &axum::Router,
    group_id: &str,
    actor_id: &str,
    status: &str,
) -> Value {
    // The worker may enter its idle interval immediately before the message arrives.
    // Cover that full interval plus bounded time for the browser submission itself.
    let deadline = Instant::now()
        + IDLE_POLL_INTERVAL
        + SUBMISSION_EVIDENCE_TIMEOUT
        + BACKGROUND_DELIVERY_MARGIN;
    loop {
        let value = request_json(
            app,
            Request::get(format!(
                "/api/v1/web-model/browser-session?group_id={group_id}&actor_id={actor_id}"
            ))
            .body(Body::empty())
            .expect("browser state request"),
        )
        .await;
        if value["result"]["browser_session"]["delivery_target"]["last_delivery_status"] == status {
            return value;
        }
        if Instant::now() >= deadline {
            panic!("background browser delivery did not reach {status}; last={value}");
        }
        tokio::time::sleep(DELIVERY_STATUS_POLL_INTERVAL).await;
    }
}

async fn wait_for_daemon(home: &HomeLayout) {
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").is_file() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}
