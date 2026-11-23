use super::*;
use axum::{
    Json, Router,
    response::Html,
    routing::{get, post},
};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout, ledger};
use std::sync::{Arc, Mutex};

/// On Unix, a child spawned by a concurrently running test can keep an
/// inherited copy of the lock's descriptor until it execs, and the flock
/// with it, so a released lock can stay busy until that copy closes.
fn reclaim<T>(mut attempt: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let claimed = attempt();
        if claimed.is_some() || std::time::Instant::now() >= deadline {
            return claimed;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn browser_work_and_a_switch_exclude_each_other_without_waiting() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let path = cccc_core::web_model_connectors::browser_work_lock(&home, "group", "actor");
    let lock = || cccc_core::fs::try_exclusive_lock(&path).expect("lock");

    let switch = lock().expect("free");
    assert!(
        browser_work_in(&home, "group", "actor")
            .expect("work")
            .is_none()
    );
    drop(switch);

    let work = reclaim(|| browser_work_in(&home, "group", "actor").expect("work"))
        .expect("released switch lock");
    assert!(lock().is_none());
    assert!(
        browser_work_in(&home, "group", "actor")
            .expect("work")
            .is_none()
    );
    drop(work);
    assert!(reclaim(lock).is_some());
}

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
async fn settled_browser_turns_are_reviewed_without_resubmitting_their_drafts() {
    if crate::system_browser_path().is_none() {
        return;
    }
    let Some(_chrome_guard) = crate::browser_surface::chrome_test_guard().await else {
        return;
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("restart recovery", "").expect("group");
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
    let message = |text: &str| {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data.insert("message_mode".into(), json!("send"));
        event.data.insert("to".into(), json!(["browser-test"]));
        event.data.insert("text".into(), json!(text));
        ledger::append(&ledger_path, &event).expect("message");
        event
    };
    let first = message("claimed before a target fence");
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
    let call = |op: &'static str, extra: Value| {
        let client = client.clone();
        let group_id = group.group_id.clone();
        async move {
            let mut args =
                json!({"group_id":group_id,"actor_id":"browser-test","by":"browser-test"});
            args.as_object_mut()
                .expect("args")
                .extend(extra.as_object().cloned().expect("extra"));
            let response = client
                .call(&DaemonRequest {
                    v: 1,
                    op: op.into(),
                    args: args.as_object().expect("args").clone(),
                })
                .await
                .expect("daemon request");
            assert!(response.ok, "{:?}", response.error);
            response.result
        }
    };
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
        "restart-recovery-test".into(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/", get(|| async {
            Html(r#"<form onsubmit="event.preventDefault();window.attempts++;const t=document.querySelector('textarea');const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent=t.value;document.body.append(a);t.value=''"><textarea id="prompt-textarea" style="width:500px;height:120px"></textarea><button type="submit">Send</button></form><script>window.attempts=0</script>"#)
        }))).await.expect("serve");
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
        json!({"kind":"existing_chat","url":url,"last_delivery_id":"previous",
               "last_delivery_turn_id":"previous-turn","last_delivery_status":"bound"}),
    )
    .expect("target");

    let wait = call(
        "runtime_wait_next_turn",
        json!({"transport":"web_model_browser"}),
    )
    .await;
    let turn = &wait["turn"];
    assert_eq!(turn["event_ids"], json!([first.id]));
    call(
        "web_model_browser_delivery_record",
        json!({"turn_id":turn["turn_id"],"event_ids":turn["event_ids"],"delivery_id":"interrupted",
               "browser_delivery":{"state":"ambiguous","detail":"daemon restarted"}}),
    )
    .await;
    assert!(matches!(
        deliver_pending(&state, &group.group_id, "browser-test")
            .await
            .expect("recover"),
        DeliveryOutcome::Ambiguous
    ));
    let target = load_target(&state, &group.group_id, "browser-test").expect("target");
    let expected_id = browser_delivery_id("browser-test", turn["turn_id"].as_str().expect("turn"));
    assert_eq!(target["last_delivery_status"], "submission_ambiguous");
    assert_eq!(target["last_delivery_id"], expected_id);
    assert_eq!(target["last_delivery_turn_id"], turn["turn_id"]);
    assert_eq!(target["last_delivery_event_ids"], turn["event_ids"]);
    assert_eq!(
        call("headless_status", json!({})).await["state"]["active_turn_id"],
        ""
    );
    resume_after_review(&state, &group.group_id, "browser-test", &expected_id)
        .await
        .expect("user reviews first turn");

    // An older daemon could record failed without releasing the active turn.
    // Its staged draft must survive the same conservative recovery.
    let second = message("legacy failed turn");
    let wait = call(
        "runtime_wait_next_turn",
        json!({"transport":"web_model_browser"}),
    )
    .await;
    let turn = &wait["turn"];
    assert_eq!(turn["event_ids"], json!([second.id]));
    let mut failure = Event::new("runtime.delivery", &group.group_id);
    failure.by = "system".into();
    failure.data = json!({"actor_id":"browser-test","source_event_id":second.id,
        "state":"failed","transport":"web_model_browser"})
    .as_object()
    .expect("delivery")
    .clone();
    ledger::append(&ledger_path, &failure).expect("legacy failure");
    update_target(
        &state,
        &group.group_id,
        "browser-test",
        json!({"last_delivery_status":"deferred","last_delivery_turn_id":turn["turn_id"]}),
    )
    .expect("staged target");
    page.evaluate("document.querySelector('textarea').value='staged draft'")
        .await
        .expect("stage draft");
    assert!(matches!(
        deliver_pending(&state, &group.group_id, "browser-test")
            .await
            .expect("recover legacy"),
        DeliveryOutcome::Ambiguous
    ));
    let target = load_target(&state, &group.group_id, "browser-test").expect("target");
    assert_eq!(target["last_delivery_status"], "submission_ambiguous");
    assert_eq!(target["last_delivery_turn_id"], turn["turn_id"]);
    assert_eq!(
        call("headless_status", json!({})).await["state"]["active_turn_id"],
        ""
    );
    assert_eq!(
        page.evaluate("document.querySelector('textarea').value")
            .await
            .expect("draft")
            .into_value::<String>()
            .expect("text"),
        "staged draft"
    );
    assert_eq!(
        page.evaluate("window.attempts")
            .await
            .expect("attempts")
            .into_value::<u32>()
            .expect("number"),
        0
    );
    let states =
        cccc_core::inbox::delivery_states(&home, &group, &["browser-test".into()], &second.id)
            .expect("states");
    assert_eq!(states["browser-test"].0, "ambiguous");

    // A still-claimed deferred turn is different: after the user reviews the
    // old uncertain batch, its own exact staged draft must be submitted once.
    page.evaluate("document.querySelector('textarea').value=''")
        .await
        .expect("clear reviewed draft");
    let previous_id = target["last_delivery_id"].as_str().expect("review id");
    resume_after_review(&state, &group.group_id, "browser-test", previous_id)
        .await
        .expect("user reviews legacy turn");
    let third = message("continue the same deferred turn");
    let wait = call(
        "runtime_wait_next_turn",
        json!({"transport":"web_model_browser"}),
    )
    .await;
    let turn = &wait["turn"];
    assert_eq!(turn["event_ids"], json!([third.id]));
    let turn_id = turn["turn_id"].as_str().expect("turn");
    let delivery_id = browser_delivery_id("browser-test", turn_id);
    let target = load_target(&state, &group.group_id, "browser-test").expect("target");
    let (prompt, _) =
        build_browser_prompt(turn, &target, &url, "browser-test", &delivery_id, &third.id)
            .expect("prompt");
    update_target(
        &state,
        &group.group_id,
        "browser-test",
        json!({"last_delivery_status":"deferred","last_delivery_id":delivery_id,
               "last_delivery_turn_id":turn_id,"last_delivery_event_ids":turn["event_ids"]}),
    )
    .expect("staged target");
    page.evaluate(format!(
        "document.querySelector('textarea').value={}",
        json!(prompt)
    ))
    .await
    .expect("stage deferred batch");
    assert!(matches!(
        deliver_pending(&state, &group.group_id, "browser-test")
            .await
            .expect("resume claimed batch"),
        DeliveryOutcome::Submitted
    ));
    assert_eq!(
        page.evaluate("window.attempts")
            .await
            .expect("attempts")
            .into_value::<u32>()
            .expect("number"),
        1
    );
    let states =
        cccc_core::inbox::delivery_states(&home, &group, &["browser-test".into()], &third.id)
            .expect("states");
    assert_eq!(states["browser-test"].0, "accepted");
    assert_eq!(
        call("headless_status", json!({})).await["state"]["active_turn_id"],
        ""
    );
    assert!(matches!(
        deliver_pending(&state, &group.group_id, "browser-test")
            .await
            .expect("poll after completed turn"),
        DeliveryOutcome::Idle
    ));
    assert_eq!(
        page.evaluate("window.attempts")
            .await
            .expect("attempts")
            .into_value::<u32>()
            .expect("number"),
        1,
        "the completed batch must not be submitted again"
    );

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
}

#[tokio::test]
async fn verified_legacy_draft_recovers_through_rebuilt_prompt_once() {
    if crate::system_browser_path().is_none() {
        return;
    }
    let Some(_chrome_guard) = crate::browser_surface::chrome_test_guard().await else {
        return;
    };
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
    use chromiumoxide::cdp::browser_protocol::fetch::{
        EnableParams, EventRequestPaused, FulfillRequestParams, HeaderEntry,
    };
    use futures_util::StreamExt;

    if crate::system_browser_path().is_none() {
        return;
    }
    let Some(_chrome_guard) = crate::browser_surface::chrome_test_guard().await else {
        return;
    };
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
    let entry_url = format!("http://{}/", listener.local_addr().expect("addr"));
    let url = "https://chatgpt.com/c/recovery-fixture".to_owned();
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
        .open(
            &session_key,
            &temp.path().join("profile"),
            &entry_url,
            800,
            600,
        )
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
    // Serve a synthetic ChatGPT origin so the same chat can change Project routes.
    // Every request is fulfilled locally, including navigation and subresources.
    let mut requests = page
        .event_listener::<EventRequestPaused>()
        .await
        .expect("intercept");
    page.execute(EnableParams::default())
        .await
        .expect("enable interception");
    let control = page.clone();
    let intercept = tokio::spawn(async move {
        while let Some(request) = requests.next().await {
            let mut response = FulfillRequestParams::new(request.request_id.clone(), 200);
            response.response_headers = Some(vec![HeaderEntry::new("Content-Type", "text/html")]);
            response.body = Some(
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, HTML).into(),
            );
            if control.execute(response).await.is_err() {
                break;
            }
        }
    });
    page.goto(&url).await.expect("offline ChatGPT page");
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
    page.evaluate("history.replaceState(null,'','/g/g-p-fixture/c/recovery-fixture');send();const b=document.createElement('button');b.id='generating';b.setAttribute('aria-label','Stop generating');b.textContent='Stop';document.querySelector('main').append(b)").await.expect("manual send");
    assert!(
        surfaces
            .inspect_delivery_receipt(
                &session_key,
                "https://chatgpt.com/c/other-conversation",
                &marker
            )
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
    let recovered = load_target(&state, &group.group_id, "browser-test").expect("recovered target");
    assert_eq!(
        recovered["bootstrap_seed_conversation_url"], url,
        "a Project-route receipt must keep the bootstrap seed on the bound URL"
    );
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
    assert!(sent[0].contains("Session bootstrap"));
    assert!(
        !sent[1].contains("Session bootstrap"),
        "the next delivery must not seed the same conversation again"
    );
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
    intercept.abort();
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

/// The browsers the session owner keeps, as its private IPC reports them.
async fn kept_resources(
    home: &HomeLayout,
) -> Vec<cccc_daemon::session_owner::browser::BrowserResource> {
    let response = cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: "browser_resources".into(),
            args: json!({"by":"user"}).as_object().expect("args").clone(),
        })
        .await
        .expect("browser resources");
    assert!(response.ok, "{:?}", response.error);
    serde_json::from_value(response.result["resources"].clone()).expect("resources")
}

/// The page records the session owner keeps for its browsers.
async fn kept_pages(home: &HomeLayout) -> Vec<String> {
    kept_resources(home)
        .await
        .into_iter()
        .flat_map(|resource| resource.sessions)
        .map(|page| page.target_id)
        .collect()
}

/// The session owner keeps the shared browser with its conversation across
/// Web and daemon restarts. Each Web holds only a CDP connection and lets go
/// when it stops; the next Web takes the same window back when it opens the
/// member's surface. A running answer holds back the next batch, and a
/// handoff interrupted after submission is recovered from the page's echo
/// without being submitted again.
#[tokio::test]
async fn preserved_browser_keeps_the_same_conversation_across_restarts() {
    use futures_util::StreamExt;
    if crate::system_browser_path().is_none()
        || !cccc_daemon::session_owner::platform_supported()
        || (cfg!(target_os = "linux") && !std::path::Path::new("/usr/bin/Xvfb").is_file())
    {
        return;
    }
    let Some(_chrome_guard) = crate::browser_surface::chrome_test_guard().await else {
        return;
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("preserved browser", "").expect("group");
    group.running = true;
    let mut actor = Actor::new("browser-test");
    actor.runtime = ActorRuntime::WebModel;
    actor.normalize_runtime_constraints();
    actor.generation = "fixture-generation".into();
    actor
        .env
        .insert("CCCC_WEB_MODEL_DELIVERY_MODE".into(), "browser".into());
    let identity = crate::browser_surface::actor_identity(&actor);
    let created_at = actor.created_at.clone();
    group.actors.push(actor);
    group.extra.insert(
        super::super::web_model_browser::TARGETS_KEY.into(),
        json!({}),
    );
    store.save(&group).expect("save");
    let group_id = group.group_id.clone();
    let ledger_path = store.ledger_path(&group_id).expect("ledger");
    let message = |text: &str| {
        let mut event = Event::new("chat.message", &group_id);
        event.by = "user".into();
        event.data.insert("message_mode".into(), json!("send"));
        event.data.insert("to".into(), json!(["browser-test"]));
        event.data.insert("text".into(), json!(text));
        ledger::append(&ledger_path, &event).expect("message");
        event
    };
    async fn ready(client: &cccc_client::DaemonClient) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let ping = DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                };
                if client.call(&ping).await.is_ok_and(|r| r.ok) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("daemon ready");
    }
    async fn call(
        home: &HomeLayout,
        group_id: &str,
        op: &str,
        extra: Value,
    ) -> Result<Value, String> {
        let mut args = json!({"group_id":group_id,"actor_id":"browser-test","by":"browser-test"});
        args.as_object_mut()
            .expect("args")
            .extend(extra.as_object().cloned().expect("extra"));
        let response = cccc_client::DaemonClient::new(home.clone())
            .call(&DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("args").clone(),
            })
            .await
            .map_err(|error| error.to_string())?;
        if !response.ok {
            return Err(format!("{:?}", response.error));
        }
        Ok(Value::Object(response.result))
    }
    // The provider page: an answer runs (a visible Stop button) until the
    // test calls finish(), like a model still generating.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/", get(|| async {
            Html(r#"<form onsubmit="event.preventDefault();window.attempts++;const t=document.querySelector('textarea');const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent=t.value;document.body.append(a);t.value='';document.getElementById('stop').hidden=false"><textarea id="prompt-textarea" style="width:500px;height:120px"></textarea><button type="submit">Send</button></form><button id="stop" type="button" aria-label="Stop generating" hidden>Stop</button><script>window.attempts=0;window.finish=()=>{const a=document.createElement('article');a.dataset.messageAuthorRole='assistant';a.textContent='answer '+window.attempts;document.body.append(a);document.getElementById('stop').hidden=true}</script>"#)
        }))).await.expect("serve");
    });

    // The session owner starts Chrome: it is not a child of any Web below.
    let profile = temp.path().join("profile");
    std::fs::create_dir_all(&profile).expect("profile");
    let session_key = key(&group_id, "browser-test");
    // The browser's windows, read through a connection of the test's own.
    let browser_targets = |websocket: &String| {
        let websocket = websocket.clone();
        async move {
            let (connection, mut events) = chromiumoxide::Browser::connect(websocket)
                .await
                .expect("inspect the kept browser");
            let pump = tokio::spawn(async move { while events.next().await.is_some() {} });
            let targets = connection
                .execute(chromiumoxide::cdp::browser_protocol::target::GetTargetsParams::default())
                .await
                .expect("targets")
                .result
                .target_infos
                .into_iter()
                .filter(|target| target.r#type == "page")
                .map(|target| target.target_id.inner().clone())
                .collect::<Vec<_>>();
            pump.abort();
            targets
        }
    };

    // --- First Web and daemon: deliver M1; the model is still answering.
    let daemon = tokio::spawn({
        let home = home.clone();
        async move { cccc_daemon::run(home).await }
    });
    ready(&cccc_client::DaemonClient::new(home.clone())).await;
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
        "preserved-browser-1".into(),
    );
    surfaces.hold_shared_browsers(cccc_client::DaemonClient::new(home.clone()));
    // Opening the member's surface also takes back a window kept for it.
    let open = |surfaces: Arc<crate::browser_surface::BrowserSurfaces>| {
        let (session_key, profile, url) = (session_key.clone(), profile.clone(), url.clone());
        let (identity, created_at) = (identity.clone(), created_at.clone());
        async move {
            surfaces
                .ensure_open_shared_actor(
                    &session_key,
                    &profile,
                    &url,
                    (800, 600),
                    &identity,
                    &created_at,
                )
                .await
                .expect("actor window");
        }
    };
    open(Arc::clone(&surfaces)).await;
    let websocket = {
        let mut connections = kept_resources(&home)
            .await
            .into_iter()
            .filter_map(|resource| resource.connection);
        let connection = connections.next().expect("kept browser");
        assert!(connections.next().is_none(), "one kept browser");
        connection.websocket_url
    };
    let readiness = |state: AppState| {
        let session_key = session_key.clone();
        async move {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    let readiness = state
                        .browser_surfaces
                        .prompt_readiness(&session_key)
                        .await
                        .unwrap_or_default();
                    if readiness["ready"] == true {
                        break readiness;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .expect("page ready")
        }
    };
    readiness(state.clone()).await;
    let page_target = surfaces
        .sessions
        .lock()
        .await
        .get(&session_key)
        .expect("session")
        .page
        .target_id()
        .inner()
        .clone();
    let pages_before = browser_targets(&websocket).await;
    super::login_tests::pair_fixture(&state, &group_id, &url);
    update_target(
        &state,
        &group_id,
        "browser-test",
        json!({"kind":"existing_chat","url":url,"last_delivery_status":"bound"}),
    )
    .expect("target");
    let first = message("M1 before maintenance");
    assert!(matches!(
        deliver_pending(&state, &group_id, "browser-test")
            .await
            .expect("deliver M1"),
        DeliveryOutcome::Submitted
    ));
    let page = surfaces
        .sessions
        .lock()
        .await
        .get(&session_key)
        .expect("session")
        .page
        .clone();
    let attempts = |page: chromiumoxide::Page| async move {
        page.evaluate("window.attempts")
            .await
            .expect("attempts")
            .into_value::<u32>()
            .expect("number")
    };
    assert_eq!(attempts(page.clone()).await, 1);
    assert_eq!(
        state
            .browser_surfaces
            .prompt_readiness(&session_key)
            .await
            .expect("readiness")["running"],
        true,
        "the model is still answering M1"
    );

    // --- Maintenance: the Web stops and lets go, and the daemon stops
    // keeping its members; the session owner keeps the window recorded.
    crate::shutdown::browser_surfaces(&surfaces).await;
    let _ = shutdown.send(());
    drop(state);
    drop(surfaces);
    assert_eq!(
        kept_pages(&home).await.as_slice(),
        std::slice::from_ref(&page_target)
    );
    call(&home, &group_id, "shutdown_preserving_agents", json!({}))
        .await
        .expect("stop keeping members");
    daemon.await.expect("daemon task").expect("daemon stopped");
    assert_eq!(
        browser_targets(&websocket).await,
        pages_before,
        "no window closed or added"
    );

    // --- Second Web and daemon: the same window comes back; M2 waits for
    // the running answer and is then delivered once.
    let daemon = tokio::spawn({
        let home = home.clone();
        async move { cccc_daemon::run(home).await }
    });
    ready(&cccc_client::DaemonClient::new(home.clone())).await;
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
        "preserved-browser-2".into(),
    );
    surfaces.hold_shared_browsers(cccc_client::DaemonClient::new(home.clone()));
    open(Arc::clone(&surfaces)).await;
    assert!(state.browser_surfaces.info(&session_key).await["active"] == true);
    let page = surfaces
        .sessions
        .lock()
        .await
        .get(&session_key)
        .expect("restored session")
        .page
        .clone();
    assert_eq!(page.target_id().inner(), &page_target);
    assert_eq!(attempts(page.clone()).await, 1, "the page kept its state");
    let second = message("M2 during the running answer");
    assert!(matches!(
        deliver_pending(&state, &group_id, "browser-test")
            .await
            .expect("wait for the answer"),
        DeliveryOutcome::Idle
    ));
    assert_eq!(attempts(page.clone()).await, 1, "M2 waits for the answer");
    page.evaluate("window.finish()").await.expect("answer done");
    readiness(state.clone()).await;
    assert!(matches!(
        deliver_pending(&state, &group_id, "browser-test")
            .await
            .expect("deliver M2"),
        DeliveryOutcome::Submitted
    ));
    assert_eq!(attempts(page.clone()).await, 2);
    page.evaluate("window.finish()").await.expect("answer done");
    readiness(state.clone()).await;

    // --- An interrupted handoff: the Web claimed M3, wrote its fence and
    // submitted it, then stopped before recording the outcome.
    let third = message("M3 interrupted after submission");
    let wait = call(
        &home,
        &group_id,
        "runtime_wait_next_turn",
        json!({"transport":"web_model_browser"}),
    )
    .await
    .expect("claim M3");
    let turn = wait["turn"].clone();
    assert_eq!(turn["event_ids"], json!([third.id]));
    let turn_id = turn["turn_id"].as_str().expect("turn").to_owned();
    let delivery_id = browser_delivery_id("browser-test", &turn_id);
    let target = load_target(&state, &group_id, "browser-test").expect("target");
    let (prompt, _) = build_browser_prompt(
        &turn,
        &target,
        &url,
        "browser-test",
        &delivery_id,
        &third.id,
    )
    .expect("prompt");
    update_target(
        &state,
        &group_id,
        "browser-test",
        json!({"last_delivery_id":delivery_id,"last_delivery_turn_id":turn_id,
               "last_delivery_event_ids":turn["event_ids"],"last_delivery_status":"submitting"}),
    )
    .expect("fence");
    record_delivery(
        &state,
        &group_id,
        "browser-test",
        &turn_id,
        turn["event_ids"].clone(),
        &delivery_id,
        "submitting",
        "",
        json!({"target_url":url}),
    )
    .await;
    page.evaluate(format!(
        "document.querySelector('textarea').value={};document.querySelector('form').requestSubmit()",
        json!(prompt)
    ))
    .await
    .expect("submitted before the Web stopped");
    assert_eq!(attempts(page.clone()).await, 3);
    crate::shutdown::browser_surfaces(&surfaces).await;
    let _ = shutdown.send(());
    drop(state);
    drop(surfaces);
    call(&home, &group_id, "shutdown_preserving_agents", json!({}))
        .await
        .expect("stop keeping members again");
    daemon.await.expect("daemon task").expect("daemon stopped");

    // --- Third Web and daemon: the same window proves M3 arrived.
    let daemon = tokio::spawn({
        let home = home.clone();
        async move { cccc_daemon::run(home).await }
    });
    ready(&cccc_client::DaemonClient::new(home.clone())).await;
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
        "preserved-browser-3".into(),
    );
    surfaces.hold_shared_browsers(cccc_client::DaemonClient::new(home.clone()));
    open(Arc::clone(&surfaces)).await;
    // The model is still answering M3 while this Web comes back.
    let page = surfaces
        .sessions
        .lock()
        .await
        .get(&session_key)
        .expect("restored session")
        .page
        .clone();
    assert_eq!(page.target_id().inner(), &page_target);
    let mut outcomes = Vec::new();
    for _ in 0..3 {
        let outcome = deliver_pending(&state, &group_id, "browser-test")
            .await
            .expect("recover M3");
        let done = matches!(outcome, DeliveryOutcome::Submitted);
        outcomes.push(match outcome {
            DeliveryOutcome::Submitted => "submitted".to_owned(),
            DeliveryOutcome::Idle => "idle".to_owned(),
            DeliveryOutcome::Deferred(turn) => format!("deferred {turn}"),
            DeliveryOutcome::Ambiguous => "ambiguous".to_owned(),
            DeliveryOutcome::Stopped => "stopped".to_owned(),
        });
        if done {
            break;
        }
    }
    let target = load_target(&state, &group_id, "browser-test").expect("target");
    assert_eq!(
        attempts(page.clone()).await,
        3,
        "M3 was never submitted again"
    );
    for event in [&first, &second, &third] {
        let states = cccc_core::inbox::delivery_states(
            &home,
            &store.load(&group_id).expect("group"),
            &["browser-test".into()],
            &event.id,
        )
        .expect("states");
        assert_eq!(
            states.get("browser-test"),
            Some(&("accepted".to_owned(), "web_model_browser".to_owned())),
            "{}",
            event.id
        );
    }
    assert_eq!(outcomes, ["ambiguous", "submitted"]);
    assert_eq!(target["last_delivery_status"], "submitted", "{outcomes:?}");
    assert_eq!(
        browser_targets(&websocket).await,
        pages_before,
        "still the same windows"
    );

    // Stopping CCCC stops the browser the session owner kept.
    crate::shutdown::browser_surfaces(&surfaces).await;
    let _ = shutdown.send(());
    call(&home, &group_id, "shutdown", json!({}))
        .await
        .expect("final stop");
    daemon.await.expect("daemon task").expect("daemon stopped");
    server.abort();
}

/// A Web that is stopping claims no more browser work, and lets go of its
/// browser only after the work it already claimed has reached its record.
/// The session owner keeps Chrome, so the first Web can hand the window to
/// the second.
#[tokio::test]
async fn a_stopping_web_records_claimed_work_and_claims_no_more() {
    if crate::system_browser_path().is_none()
        || !cccc_daemon::session_owner::platform_supported()
        || (cfg!(target_os = "linux") && !std::path::Path::new("/usr/bin/Xvfb").is_file())
    {
        return;
    }
    let Some(_chrome_guard) = crate::browser_surface::chrome_test_guard().await else {
        return;
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("stopping web", "").expect("group");
    group.running = true;
    let mut actor = Actor::new("browser-test");
    actor.runtime = ActorRuntime::WebModel;
    actor.normalize_runtime_constraints();
    actor.generation = "fixture-generation".into();
    actor
        .env
        .insert("CCCC_WEB_MODEL_DELIVERY_MODE".into(), "browser".into());
    let identity = crate::browser_surface::actor_identity(&actor);
    let created_at = actor.created_at.clone();
    group.actors.push(actor);
    group.extra.insert(
        super::super::web_model_browser::TARGETS_KEY.into(),
        json!({}),
    );
    store.save(&group).expect("save");
    let group_id = group.group_id.clone();
    let mut message = Event::new("chat.message", &group_id);
    message.by = "user".into();
    message.data.insert("message_mode".into(), json!("send"));
    message.data.insert("to".into(), json!(["browser-test"]));
    message
        .data
        .insert("text".into(), json!("M1 while stopping"));

    // The provider page shows a sent message a second after Send, so the
    // delivery is still waiting for that evidence when the Web is asked to stop.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/", get(|| async {
            Html(r#"<form onsubmit="event.preventDefault();window.attempts++;const t=document.querySelector('textarea');const text=t.value;t.value='';setTimeout(()=>{const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent=text;document.body.append(a)},1000)"><textarea id="prompt-textarea" style="width:500px;height:120px"></textarea><button type="submit">Send</button></form><script>window.attempts=0</script>"#)
        }))).await.expect("serve");
    });
    let profile = temp.path().join("profile");
    std::fs::create_dir_all(&profile).expect("profile");
    let daemon = tokio::spawn({
        let home = home.clone();
        async move { cccc_daemon::run(home).await }
    });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let ping = DaemonRequest {
            v: 1,
            op: "ping".into(),
            args: Default::default(),
        };
        while !client.call(&ping).await.is_ok_and(|r| r.ok) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");
    // Each Web leaves the shared browser to the session owner, as `serve` does.
    let web = |name: &str| {
        let web = crate::app_with_shutdown(
            home.clone(),
            tokio::sync::broadcast::channel(1).0,
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            name.into(),
        );
        web.2.hold_shared_browsers(client.clone());
        web
    };
    let session_key = key(&group_id, "browser-test");
    // Opening the member's surface also takes back a window kept for it.
    let open = |surfaces: Arc<crate::browser_surface::BrowserSurfaces>| {
        let (session_key, profile, url) = (session_key.clone(), profile.clone(), url.clone());
        let (identity, created_at) = (identity.clone(), created_at.clone());
        async move {
            surfaces
                .ensure_open_shared_actor(
                    &session_key,
                    &profile,
                    &url,
                    (800, 600),
                    &identity,
                    &created_at,
                )
                .await
                .expect("actor window");
        }
    };
    let ready = |state: AppState| {
        let session_key = session_key.clone();
        async move {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while state
                    .browser_surfaces
                    .prompt_readiness(&session_key)
                    .await
                    .unwrap_or_default()["ready"]
                    != true
                {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .expect("page ready");
        }
    };

    // --- The first Web opens the window, then stops: M1 stays queued.
    let (_, _, surfaces, state) = web("stopping-web-1");
    open(Arc::clone(&surfaces)).await;
    ready(state.clone()).await;
    super::login_tests::pair_fixture(&state, &group_id, &url);
    update_target(
        &state,
        &group_id,
        "browser-test",
        json!({"kind":"existing_chat","url":url,"last_delivery_status":"bound"}),
    )
    .expect("target");
    ledger::append(&store.ledger_path(&group_id).expect("ledger"), &message).expect("M1");
    surfaces.stop_opening();
    assert!(matches!(
        deliver_pending(&state, &group_id, "browser-test")
            .await
            .expect("stopping Web"),
        DeliveryOutcome::Idle
    ));
    let target = load_target(&state, &group_id, "browser-test").expect("target");
    assert_eq!(
        target["last_delivery_status"], "bound",
        "M1 was not claimed"
    );
    crate::shutdown::browser_surfaces(&surfaces).await;
    drop(state);
    drop(surfaces);

    // --- The second Web takes the window back, claims M1 and is asked to
    // stop while submitting it.
    let (_, _, surfaces, state) = web("stopping-web-2");
    open(Arc::clone(&surfaces)).await;
    ready(state.clone()).await;
    let delivery = tokio::spawn({
        let state = state.clone();
        let group_id = group_id.clone();
        async move { deliver_pending(&state, &group_id, "browser-test").await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while load_target(&state, &group_id, "browser-test").expect("target")
            ["last_delivery_status"]
            != "submitting"
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("M1 claimed");
    let stopping = std::time::Instant::now();
    crate::shutdown::browser_surfaces(&surfaces).await;
    assert!(
        delivery.is_finished(),
        "the browser was let go only after the claimed work ended"
    );
    assert!(stopping.elapsed() < std::time::Duration::from_secs(10));
    assert!(matches!(
        delivery.await.expect("delivery task").expect("delivery"),
        DeliveryOutcome::Submitted
    ));
    let target = load_target(&state, &group_id, "browser-test").expect("target");
    assert_eq!(target["last_delivery_status"], "submitted");
    let states = cccc_core::inbox::delivery_states(
        &home,
        &store.load(&group_id).expect("group"),
        &["browser-test".into()],
        &message.id,
    )
    .expect("states");
    assert_eq!(
        states.get("browser-test"),
        Some(&("accepted".to_owned(), "web_model_browser".to_owned()))
    );

    drop(state);
    drop(surfaces);
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("stop daemon");
    daemon.await.expect("daemon task").expect("daemon stopped");
    server.abort();
}
