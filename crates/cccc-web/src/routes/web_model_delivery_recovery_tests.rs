use super::*;
use axum::{
    Json, Router,
    response::Html,
    routing::{get, post},
};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout, ledger};
use std::sync::{Arc, Mutex};

#[test]
fn rejected_session_guards_do_not_release_the_active_owner() {
    for registry in [&WORKERS, &IN_FLIGHT] {
        let key = "guard-contention-regression".to_owned();
        let owner = SessionGuard::acquire(registry, key.clone()).expect("first owner");
        for _ in 0..4 {
            assert!(
                SessionGuard::acquire(registry, key.clone()).is_none(),
                "a rejected contender must leave the active owner registered"
            );
        }
        let other = SessionGuard::acquire(registry, "other-actor-guard-regression".into())
            .expect("another Actor remains independent");
        drop(other);
        assert!(SessionGuard::acquire(registry, key.clone()).is_none());
        drop(owner);
        assert!(SessionGuard::acquire(registry, key).is_some());
    }
}

#[tokio::test]
async fn verified_legacy_draft_recovers_through_rebuilt_prompt_once() {
    if crate::system_browser_path().is_none() {
        return;
    }
    let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("legacy recovery", "").expect("group");
    let mut actor = Actor::new("browser-test");
    actor.runtime = ActorRuntime::WebModel;
    actor.normalize_runtime_constraints();
    group.actors.push(actor);
    group.extra.insert(
        super::super::web_model_browser::TARGETS_KEY.into(),
        json!({}),
    );
    // Keep the fixture Group stopped: only explicit offline recovery is exercised.
    assert!(!group.running);
    store.save(&group).expect("save");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data.insert("to".into(), json!(["browser-test"]));
    event
        .data
        .insert("text".into(), json!("Recover this exact old draft."));
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    ledger::append(&ledger_path, &event).expect("seed message");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if client
                .call(&DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");
    let recorded = client.call(&DaemonRequest {v:1, op:"web_model_browser_delivery_record".into(), args:json!({
        "group_id":group.group_id,"actor_id":"browser-test","turn_id":"old-turn",
        "event_ids":[event.id],"delivery_id":"wmd_original","browser_delivery":{"state":"submitted"}
    }).as_object().expect("args").clone()}).await.expect("seed legacy handoff");
    assert!(recorded.ok, "{recorded:?}");

    let shutdown = tokio::sync::broadcast::channel(1).0;
    let (_, _, surfaces, state) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 0,
        },
        "legacy-test".into(),
    );
    let submitted = Arc::new(Mutex::new(Vec::<String>::new()));
    let submitted_handler = submitted.clone();
    let old = "[user -> browser-test] Recover this exact old draft.";
    let html = format!(
        r#"<main><form onsubmit="event.preventDefault(); const t=document.querySelector('textarea'); const text=t.value; const a=document.createElement('article'); a.dataset.messageAuthorRole='user'; a.textContent=text; document.querySelector('main').append(a); t.value=''; history.replaceState(null,'','/c/recovered-legacy'); fetch('/submitted',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify(text)}});"><textarea id='prompt-textarea' style='width:500px;height:150px'></textarea><button type='submit'>Send</button></form></main><script>document.querySelector('textarea').value={};</script>"#,
        serde_json::to_string(old).expect("old text")
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route(
                    "/",
                    get(move || {
                        let html = html.clone();
                        async move { Html(html) }
                    }),
                )
                .route(
                    "/submitted",
                    post(move |Json(text): Json<String>| {
                        let submitted = submitted_handler.clone();
                        async move {
                            submitted.lock().expect("submissions").push(text);
                            Json(json!({"ok":true}))
                        }
                    }),
                ),
        )
        .await
        .expect("serve");
    });
    let session_key = key(&group.group_id, "browser-test");
    surfaces
        .open(&session_key, &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("browser");
    let target = json!({"kind":"new_chat","url":url,"last_delivery_status":"pending_new_chat_bind",
        "last_delivery_id":"wmd_original","last_delivery_turn_id":"old-turn","last_delivery_event_ids":[event.id]});
    update_target(&state, &group.group_id, "browser-test", target.clone()).expect("target");
    assert!(is_legacy_pending_delivery(&target));
    let original_ledger = std::fs::read(&ledger_path).expect("before");
    let result = recover_legacy_pending_delivery(
        &state,
        &group.group_id,
        "browser-test",
        &session_key,
        &target,
    )
    .await;
    let final_target = load_target(&state, &group.group_id, "browser-test").expect("final target");
    let messages = submitted.lock().expect("submissions").clone();
    let final_ledger = std::fs::read(&ledger_path).expect("after");
    surfaces.close(&session_key).await.expect("close browser");
    let _ = shutdown.send(());
    server.abort();
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("stop daemon");
    daemon.await.expect("daemon task").expect("daemon stopped");

    assert!(
        matches!(result, Ok(DeliveryOutcome::Submitted)),
        "recovery did not submit: {final_target}"
    );
    assert_eq!(messages.len(), 1, "must submit only the recovered batch");
    assert!(messages[0].contains("Recover this exact old draft."));
    assert!(messages[0].contains("Session bootstrap"));
    assert_ne!(messages[0], old, "recovery rebuilds the prompt");
    // Legacy recovery records evidence but cannot grant connector authority.
    assert_eq!(final_target["kind"], "none");
    assert_eq!(
        cccc_core::integration_state::group_get(
            &store,
            &group.group_id,
            super::super::web_model_browser::TARGETS_KEY
        )
        .expect("valid test fixture")["browser-test"]["kind"],
        "existing_chat"
    );
    assert_eq!(final_target["last_delivery_status"], "submitted");
    assert!(
        final_target["last_delivery_id"]
            .as_str()
            .expect("id")
            .starts_with("webdelivery:")
    );
    assert!(!is_legacy_pending_delivery(&final_target));
    assert_eq!(
        original_ledger, final_ledger,
        "recovery must not replay ledger delivery/completion or consume Mail"
    );
}

#[tokio::test]
async fn contended_worker_recovers_manual_send_and_only_delivers_later_work() {
    if crate::system_browser_path().is_none() {
        return;
    }
    let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("delivery recovery", "").expect("group");
    group.running = true;
    let mut actor = Actor::new("browser-test");
    actor.runtime = ActorRuntime::WebModel;
    actor.normalize_runtime_constraints();
    actor.generation = "fixture-generation".into();
    actor
        .env
        .insert("CCCC_WEB_MODEL_DELIVERY_MODE".into(), "browser".into());
    group.actors.push(actor);
    group.extra.insert(
        super::super::web_model_browser::TARGETS_KEY.into(),
        json!({}),
    );
    store.save(&group).expect("save");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
    let mut first = Event::new("chat.message", &group.group_id);
    first.by = "user".into();
    first.data.insert("message_mode".into(), json!("send"));
    first.data.insert("to".into(), json!(["browser-test"]));
    first
        .data
        .insert("text".into(), json!("First message, manually sent later."));
    ledger::append(&ledger_path, &first).expect("first");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if client
                .call(&DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");
    let shutdown = tokio::sync::broadcast::channel(1).0;
    let (_, _, surfaces, state) = crate::app_with_shutdown(
        home,
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 0,
        },
        "delivery-recovery-test".into(),
    );
    const HTML: &str = r#"<main><article data-message-author-role="user">Older message</article><form onsubmit="event.preventDefault();window.attempts++;if(window.accept)send();else restoreHistory()"><textarea id="prompt-textarea" style="width:500px;height:120px"></textarea><button type="submit">Send</button></form></main><script>
    window.attempts=0;window.accept=false;window.sent=[];
    function restoreHistory(){const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent='Another older message returning to the DOM';document.querySelector('main').prepend(a);}
    function send(){const t=document.querySelector('textarea');window.sent.push(t.value);const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent=t.value;document.querySelector('main').append(a);t.value='';}
    </script>"#;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let url = format!("http://{}/", listener.local_addr().expect("addr"));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/", get(|| async { Html(HTML) })),
        )
        .await
        .expect("serve");
    });
    let session_key = key(&group.group_id, "browser-test");
    surfaces
        .open(&session_key, &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("browser");
    let page = surfaces
        .sessions
        .lock()
        .await
        .get(&session_key)
        .expect("session")
        .page
        .clone();
    super::login_tests::pair_fixture(&state, &group.group_id, &url);
    update_target(
        &state,
        &group.group_id,
        "browser-test",
        json!({"url":url,"kind":"existing_chat"}),
    )
    .expect("target");
    assert!(
        super::super::web_model_supervisor::actor_delivery_enabled(
            &state,
            &group.group_id,
            "browser-test"
        ),
        "fixture must enable delivery: {:?}",
        store.load(&group.group_id).expect("group").running
    );
    let initial_state = state.clone();
    let initial_group = group.group_id.clone();
    let initial = tokio::spawn(async move {
        let outcome = deliver_pending(&initial_state, &initial_group, "browser-test")
            .await
            .expect("initial delivery");
        assert!(matches!(outcome, DeliveryOutcome::Ambiguous));
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if page
                .evaluate("window.attempts")
                .await
                .expect("attempts")
                .into_value::<u32>()
                .expect("number")
                > 0
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "first click; task finished={}, target={}",
            initial.is_finished(),
            load_target(&state, &group.group_id, "browser-test").expect("target")
        )
    });
    // Ledger/status wake-ups during the live submission must not orphan it.
    for _ in 0..8 {
        super::super::web_model_supervisor::ensure_running_actor(
            &state,
            Some(&group.group_id),
            true,
        )
        .await;
    }
    tokio::time::timeout(std::time::Duration::from_secs(15),async {
        loop {
            if load_target(&state,&group.group_id,"browser-test").expect("target")["last_delivery_status"]=="submission_ambiguous" {break;}
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    }).await.expect("first uncertainty settles");
    assert!(
        !std::fs::read_to_string(&ledger_path)
            .expect("ledger")
            .contains("interrupted after its at-most-once dispatch fence")
    );
    assert_eq!(
        page.evaluate("window.attempts")
            .await
            .expect("attempts")
            .into_value::<u32>()
            .expect("number"),
        1
    );
    initial.await.expect("initial task");
    let ambiguous = load_target(&state, &group.group_id, "browser-test").expect("target");
    assert!(
        !recover_verified_ambiguous_submission(&state, &group.group_id, "browser-test", &ambiguous)
            .await
            .expect("weak history is not recoverable evidence")
    );
    assert_eq!(
        ambiguous["last_submission_evidence"]["observed"]["user_message_count"],
        2
    );
    assert_eq!(
        ambiguous["last_submission_evidence"]["observed"]["composer_exact"],
        true
    );
    assert_eq!(
        ambiguous["last_submission_evidence"]["observed"]["echo_found"],
        false
    );
    let marker = browser_batch_marker(
        "browser-test",
        ambiguous["last_delivery_id"].as_str().expect("delivery"),
        &first.id,
    );
    assert!(
        surfaces
            .inspect_delivery_receipt(&session_key, &url, &marker)
            .await
            .expect("inspect draft")
            .is_none()
    );
    // Assistant text, another batch, and another conversation are not receipts.
    page.evaluate(format!("const a=document.createElement('article');a.dataset.messageAuthorRole='assistant';a.textContent={};document.querySelector('main').append(a)",json!(marker))).await.expect("assistant echo");
    assert!(
        surfaces
            .inspect_delivery_receipt(&session_key, &url, &marker)
            .await
            .expect("assistant ignored")
            .is_none()
    );
    let mut second = Event::new("chat.message", &group.group_id);
    second.by = "user".into();
    second.data.insert("message_mode".into(), json!("send"));
    second.data.insert("to".into(), json!(["browser-test"]));
    second
        .data
        .insert("text".into(), json!("Only later work should follow."));
    ledger::append(&ledger_path, &second).expect("second");
    page.evaluate("send();const b=document.createElement('button');b.id='generating';b.setAttribute('aria-label','Stop generating');b.textContent='Stop';document.querySelector('main').append(b)").await.expect("manual send");
    assert!(
        surfaces
            .inspect_delivery_receipt(&session_key, &format!("{url}?other-chat"), &marker)
            .await
            .expect("wrong conversation")
            .is_none()
    );
    assert!(
        surfaces
            .inspect_delivery_receipt(
                &session_key,
                &url,
                "[cccc] Browser batch another-delivery events=other actor=other"
            )
            .await
            .expect("wrong batch")
            .is_none()
    );
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            if load_target(&state,&group.group_id,"browser-test").expect("target")["last_delivery_status"]=="submitted" {break;}
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    }).await.expect("manual send reconciled");
    let events = std::fs::read_to_string(&ledger_path)
        .expect("ledger")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("event"))
        .collect::<Vec<_>>();
    assert!(events.iter().any(|e| e["kind"] == "runtime.delivery"
        && e["data"]["source_event_id"] == first.id
        && e["data"]["state"] == "accepted"));
    assert!(
        events
            .iter()
            .any(|e| e["kind"] == "web_model.browser_delivery.submitted"
                && e["data"]["delivery_id"] == ambiguous["last_delivery_id"])
    );
    assert_eq!(
        page.evaluate("window.attempts")
            .await
            .expect("attempts")
            .into_value::<u32>()
            .expect("number"),
        1,
        "do not interrupt generation"
    );
    page.evaluate("document.querySelector('textarea').value='A new human draft';document.querySelector('#generating').remove();window.accept=true").await.expect("new draft");
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            if load_target(&state,&group.group_id,"browser-test").expect("target")["last_delivery_status"]=="draft_blocked" {break;}
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    }).await.expect("new draft protected");
    assert_eq!(
        page.evaluate("document.querySelector('textarea').value")
            .await
            .expect("draft")
            .into_value::<String>()
            .expect("text"),
        "A new human draft"
    );
    page.evaluate("document.querySelector('textarea').value=''")
        .await
        .expect("human clears own draft");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if page
                .evaluate("window.sent.length")
                .await
                .expect("sent")
                .into_value::<u32>()
                .expect("number")
                == 2
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("later queue proceeds");
    let sent = page
        .evaluate("window.sent")
        .await
        .expect("sent")
        .into_value::<Vec<String>>()
        .expect("messages");
    assert!(sent[0].contains(&first.id));
    assert!(!sent[1].contains(&first.id));
    assert!(sent[1].contains(&second.id));
    assert_eq!(
        page.evaluate("window.attempts")
            .await
            .expect("attempts")
            .into_value::<u32>()
            .expect("number"),
        2,
        "no replay or duplicate workers"
    );
    let _ = shutdown.send(());
    surfaces.close(&session_key).await.expect("close browser");
    server.abort();
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("stop daemon");
    daemon.await.expect("daemon task").expect("daemon stopped");
}
