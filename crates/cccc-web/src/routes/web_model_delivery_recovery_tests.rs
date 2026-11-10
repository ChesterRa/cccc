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
    let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
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
