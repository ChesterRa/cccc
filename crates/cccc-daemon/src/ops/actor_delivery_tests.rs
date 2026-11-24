#[test]
fn closing_work_keeps_what_may_have_been_submitted_uncertain() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("closing work", "").expect("group");
    let actor = Actor::new("peer1");
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let job = |text: &str| {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["peer1"],"text":text,"message_mode":"send"})
            .as_object()
            .expect("data")
            .clone();
        DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event,
            claimed: true,
            transport: "headless",
        }
    };
    let (submitted, unsubmitted) = (job("maybe sent"), job("never sent"));

    close_jobs(
        &[submitted.clone(), unsubmitted.clone()],
        &HashSet::from([submitted.event.id.clone()]),
        "it stopped",
    );

    let state = |job: &DeliveryJob| {
        crate::ops::runtime_delivery::latest_state(&home, &group.group_id, &actor.id, &job.event.id)
            .expect("state")
            .map(|(state, _)| state)
    };
    assert_eq!(state(&submitted).as_deref(), Some("ambiguous"));
    assert_eq!(state(&unsubmitted).as_deref(), Some("failed"));
}

#[test]
fn the_sender_hears_how_each_message_ended_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("closing work", "").expect("group");
    let actor = Actor::new("peer1");
    let sender = Actor::new("sender");
    group.actors.push(actor.clone());
    group.actors.push(sender.clone());
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger path");
    let job = |by: &str, mode: &str| {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = by.into();
        event.data = json!({"to":["peer1"],"text":"work","message_mode":mode})
            .as_object()
            .expect("data")
            .clone();
        ledger::append(&path, &event).expect("message");
        DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event,
            claimed: true,
            transport: "headless",
        }
    };
    // Sent under the same name before this member was added.
    let from_someone_earlier = job("sender", "send");
    let mut added = Event::new("actor.add", &group.group_id);
    added.by = "user".into();
    added.data = json!({"actor":sender}).as_object().expect("data").clone();
    ledger::append(&path, &added).expect("actor.add");
    let (maybe_sent, never_sent) = (job("sender", "send"), job("sender", "request_reply"));
    let (from_user, mail) = (job("user", "send"), job("sender", "mail"));

    close_jobs(
        &[
            from_someone_earlier.clone(),
            maybe_sent.clone(),
            never_sent.clone(),
            from_user.clone(),
            mail.clone(),
        ],
        &HashSet::from([maybe_sent.event.id.clone()]),
        "a runtime's own text",
    );
    drain_group(&home, &group.group_id);
    // The same outcome after a Retry adds nothing, but failed turning
    // uncertain is news: it changes what the sender may safely do.
    tell_sender(&never_sent, false);
    tell_sender(&maybe_sent, true);
    tell_sender(&never_sent, true);
    drain_group(&home, &group.group_id);

    let notices = ledger::read_all(&path)
        .expect("ledger")
        .into_iter()
        .filter(|event| event.kind == "system.notify")
        .collect::<Vec<_>>();
    assert_eq!(notices.len(), 3, "{notices:?}");
    assert!(!format!("{notices:?}").contains("a runtime's own text"));
    let about = |job: &DeliveryJob, state: &str| {
        notices
            .iter()
            .find(|notice| {
                notice.data["related_event_id"] == job.event.id
                    && notice.data["context"]["state"] == state
            })
            .expect("notice")
    };
    let uncertain = about(&maybe_sent, "ambiguous");
    assert_eq!(uncertain.data["target_actor_id"], "sender");
    assert_eq!(uncertain.data["title"], "Delivery to peer1 was uncertain");
    let text = uncertain.data["message"].as_str().expect("text");
    assert!(text.contains("may have reached"), "{text}");
    let failed = about(&never_sent, "failed");
    assert_eq!(failed.data["title"], "Delivery to peer1 failed");
    about(&never_sent, "ambiguous");
}

#[test]
fn scopeless_worker_records_failure_and_releases_claim() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("scopeless worker", "").expect("group");
    let actor = Actor::new("peer1");
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["peer1"],"text":"work","message_mode":"send"})
        .as_object()
        .expect("data")
        .clone();
    let job = DeliveryJob {
        home: home.clone(),
        group: group.clone(),
        actor: actor.clone(),
        event: event.clone(),
        claimed: true,
        transport: "pty",
    };
    let key = (group.group_id.clone(), actor.id.clone(), event.id.clone());
    in_flight().lock().expect("claims").insert(key.clone());
    crate::ops::runtime_delivery::append_state(
        &home,
        &group.group_id,
        &actor.id,
        &actor.created_at,
        &event.id,
        delivery_transport(&home, &group, &actor).expect("owner"),
        crate::ops::runtime_delivery::DeliveryOutcome::Claimed,
    )
    .expect("claim");
    let worker = spawn_worker(&(group.group_id.clone(), actor.id.clone()));
    worker
        .sender
        .as_ref()
        .expect("sender")
        .send(job)
        .expect("enqueue");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let state = crate::ops::runtime_delivery::latest_state(
            &home,
            &group.group_id,
            &actor.id,
            &event.id,
        )
        .expect("state");
        if state.is_some_and(|(state, _)| state == "failed") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "delivery must fail without retrying forever"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    worker.shutdown();
    assert!(!in_flight().lock().expect("claims").contains(&key));
    let events =
        ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger")).expect("events");
    let states: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "runtime.delivery")
        .filter_map(|event| event.data.get("state").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(states, ["claimed", "failed"]);
}

#[cfg(unix)]
#[test]
fn ordinary_worker_stop_after_deepseek_input_is_ambiguous_without_automatic_replay() {
    let temp = tempfile::tempdir().expect("isolated test directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
    let store = GroupStore::new(home.clone()).expect("open isolated group store");
    let mut group = store
        .create("interrupted turn", "")
        .expect("create fixture group");
    let prompt_log = temp.path().join("prompts");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
else
  printf '%s\n' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p' >> "$PROMPT_LOG"
fi
done"#;
    let mut actor = Actor::new("interrupted");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    actor.env.insert(
        "PROMPT_LOG".into(),
        prompt_log.to_string_lossy().into_owned(),
    );
    group.actors.push(actor.clone());
    store.save(&group).expect("save fixture group");
    crate::ops::deepseek_runtime::start(&home, &group, &actor, temp.path())
        .expect("start fake DeepSeek runtime");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":[actor.id],"text":"interrupted work","message_mode":"send"})
        .as_object()
        .expect("object result")
        .clone();
    let ledger_path = store
        .ledger_path(&group.group_id)
        .expect("resolve group ledger");
    ledger::append(&ledger_path, &event).expect("append delivery event");
    let worker = spawn_worker(&(group.group_id.clone(), actor.id.clone()));
    worker
        .sender
        .as_ref()
        .expect("present fixture value")
        .send(DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: event.clone(),
            claimed: false,
            transport: "deepseek",
        })
        .expect("submit fixture delivery");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !std::fs::read_to_string(&prompt_log)
        .is_ok_and(|methods| methods.lines().any(|method| method == "session/prompt"))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "provider did not receive input"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    worker.shutdown();
    drain_group(&home, &group.group_id);
    crate::ops::deepseek_runtime::stop(&group.group_id, &actor.id);
    let recorded = std::fs::read_to_string(prompt_log).expect("read provider method log");
    let methods = recorded.lines().collect::<Vec<_>>();
    eprintln!("fake provider methods: {methods:?}");
    assert_eq!(
        methods
            .iter()
            .filter(|method| **method == "session/prompt")
            .count(),
        1
    );
    let events = ledger::read_all(&ledger_path).expect("read delivery ledger");
    let states = events
        .iter()
        .filter(|record| {
            record.kind == "runtime.delivery"
                && record.data.get("source_event_id") == Some(&json!(event.id))
        })
        .filter_map(|record| record.data.get("state").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(states, ["claimed", "ambiguous"]);
    assert_eq!(
        crate::ops::runtime_delivery::claim(&home, &group, &actor, &event.id, "deepseek", false)
            .expect("claim fixture delivery"),
        crate::ops::runtime_delivery::ClaimResult::Terminal("ambiguous".into())
    );
}

/// A non-credential provider failure is retried, then the next delivery
/// gets its turn instead of waiting behind the failed one forever.
#[cfg(unix)]
#[test]
fn a_failed_provider_turn_releases_the_next_delivery_after_its_retry_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("failing provider", "").expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
else
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  echo "$rid" >> "$PROMPT_LOG"
  if printf '%s' "$line" | grep -q NEXT_READY; then
    printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$rid"
  else
    printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"Internal error: turn failed: upstream stream ended (HTTP 503)"}}\n' "$rid"
  fi
fi
done"#;
    let prompt_log = temp.path().join("prompts");
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    actor.env.insert(
        "PROMPT_LOG".into(),
        prompt_log.to_string_lossy().into_owned(),
    );
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    crate::ops::deepseek_runtime::start(&home, &group, &actor, temp.path()).expect("start");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["deepseek"],"text":"work","message_mode":"send"})
        .as_object()
        .expect("data")
        .clone();
    let mut followup = event.clone();
    followup.id = Event::new("chat.message", &group.group_id).id;
    followup.data.insert("text".into(), json!("NEXT_READY"));
    let path = store.ledger_path(&group.group_id).expect("ledger path");
    ledger::append(&path, &event).expect("first source");
    ledger::append(&path, &followup).expect("next source");
    let key = (group.group_id.clone(), actor.id.clone(), event.id.clone());
    let next_key = (
        group.group_id.clone(),
        actor.id.clone(),
        followup.id.clone(),
    );
    in_flight()
        .lock()
        .expect("claims")
        .extend([key.clone(), next_key.clone()]);
    let worker = spawn_worker(&(group.group_id.clone(), actor.id.clone()));
    worker
        .sender
        .as_ref()
        .expect("sender")
        .send(DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: event.clone(),
            claimed: true,
            transport: "deepseek",
        })
        .expect("enqueue");
    worker
        .sender
        .as_ref()
        .expect("sender")
        .send(DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: followup.clone(),
            claimed: true,
            transport: "deepseek",
        })
        .expect("enqueue follow-up behind failed delivery");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        drain_group(&home, &group.group_id);
        let state = crate::ops::runtime_delivery::latest_state(
            &home,
            &group.group_id,
            &actor.id,
            &event.id,
        )
        .expect("state");
        let next_state = crate::ops::runtime_delivery::latest_state(
            &home,
            &group.group_id,
            &actor.id,
            &followup.id,
        )
        .expect("next state");
        if state.is_some_and(|(state, _)| state == "failed")
            && next_state.is_some_and(|(state, _)| state == "accepted")
        {
            break;
        }
        let turns = std::fs::read_to_string(&prompt_log)
            .map(|log| log.lines().count())
            .unwrap_or_default();
        assert!(
            std::time::Instant::now() < deadline,
            "still retrying after {turns} failed turns"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Nothing more is sent once the delivery has failed.
    std::thread::sleep(std::time::Duration::from_secs(2));
    worker.shutdown();
    crate::ops::deepseek_runtime::stop(&group.group_id, &actor.id);
    let turns = std::fs::read_to_string(&prompt_log).expect("prompt log");
    // The failed turn and three retries, then the next task exactly once.
    assert_eq!(turns.lines().count(), 5);
    assert!(!in_flight().lock().expect("claims").contains(&key));
    assert!(!in_flight().lock().expect("claims").contains(&next_key));
    let events =
        ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger")).expect("events");
    let failures: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "runtime.delivery")
        .filter(|event| {
            event.data.get("state").and_then(serde_json::Value::as_str) == Some("failed")
        })
        .filter_map(|event| event.data.get("reason").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(failures.len(), 4);
    assert!(
        failures
            .iter()
            .all(|reason| reason.contains("upstream stream ended (HTTP 503)"))
    );
    let attempts = events
        .iter()
        .filter(|record| {
            record.kind == "runtime.delivery"
                && record.data.get("source_event_id") == Some(&json!(event.id))
        })
        .filter_map(|record| record.data.get("state").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(
        attempts,
        [
            "claimed", "failed", "claimed", "failed", "claimed", "failed", "claimed", "failed"
        ]
    );
    // Its retries are used up, so starting the Actor, resuming the Group or
    // restarting the daemon must not run it again; an explicit retry may.
    let still_pending = crate::ops::runtime_delivery::pending_sources(&home, &group, &actor, 10)
        .expect("pending sources")
        .iter()
        .any(|source| source.id == event.id);
    let automatic =
        crate::ops::runtime_delivery::claim(&home, &group, &actor, &event.id, "deepseek", false)
            .expect("automatic claim");
    assert!(
        !still_pending
            && automatic == crate::ops::runtime_delivery::ClaimResult::Terminal("failed".into()),
        "used-up delivery: pending={still_pending}, automatic claim={automatic:?}"
    );
    let exhausted = events
        .iter()
        .rfind(|event| {
            event.kind == "runtime.delivery"
                && event.data.get("state").and_then(serde_json::Value::as_str) == Some("failed")
        })
        .expect("failed record");
    assert_eq!(exhausted.data.get("retryable"), Some(&json!(false)));
    let (claimed, _) = crate::ops::runtime_delivery::claim_deliveries(
        &home,
        &group,
        &[(&actor, "deepseek")],
        &event.id,
        false,
    )
    .expect("explicit claim");
    assert!(claimed, "an explicit retry still claims a used-up delivery");
}

#[test]
fn configured_browser_transport_does_not_require_a_completed_pairing() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("transport", "").expect("group");
    cccc_core::web_model_connectors::configure(&home).expect("connector");
    let mut actor = Actor::new("a");
    actor.runtime = ActorRuntime::WebModel;
    assert_eq!(
        web_model_delivery_transport(&home, &group, &actor),
        "web_model_browser"
    );
    actor
        .env
        .insert("CCCC_WEB_MODEL_DELIVERY_MODE".into(), "pull".into());
    assert_eq!(
        web_model_delivery_transport(&home, &group, &actor),
        "web_model_pull"
    );
}

use super::*;
use cccc_core::{GroupStore, ledger};
use serde_json::json;

#[test]
fn mail_is_stored_without_runtime_queueing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("mail", "").expect("group");
    let actor = Actor::new("peer1");
    group.actors.push(actor);
    store.save(&group).expect("save actor");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["peer1"],"text":"read later","message_mode":"mail"})
        .as_object()
        .cloned()
        .expect("event data");
    let report = dispatch(&home, &group, &event);
    assert_eq!(report.state, "mail");
    assert_eq!(report.queued, 0);
    assert!(
        !in_flight()
            .lock()
            .expect("in flight")
            .iter()
            .any(|item| item.0 == group.group_id)
    );
}

#[test]
fn paused_direct_delivery_is_blocked_not_reported_as_mail() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("paused direct", "").expect("group");
    group.state = GroupState::Paused;
    group.actors.push(Actor::new("peer1"));
    store.save(&group).expect("save actor");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["peer1"],"text":"wait","message_mode":"send"})
        .as_object()
        .cloned()
        .expect("event data");

    let report = dispatch_to(&home, &group, &event, &group.actors, false);
    assert_eq!(report.state, "blocked");
    assert_eq!(report.queued, 0);
}

#[test]
fn deferred_retry_backoff_is_bounded() {
    assert_eq!(
        deferred_retry_delay(1),
        std::time::Duration::from_millis(250)
    );
    assert_eq!(deferred_retry_delay(u32::MAX), DEFERRED_RETRY_MAX);
}

#[test]
#[cfg(target_os = "linux")]
fn worker_shutdown_cancels_blocked_terminal_submission_before_runtime_stop() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_id = format!("g_blocked_delivery_{}", std::process::id());
    let mut actor = Actor::new("peer");
    actor.runtime = ActorRuntime::Custom;
    actor.submit = ActorSubmit::None;
    cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: group_id.clone(),
        actor_id: actor.id.clone(),
        runner: cccc_contracts::RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "stty raw -echo; touch ready; sleep 30".into(),
        ],
        cwd: temp.path().into(),
        env: Default::default(),
        cols: 80,
        rows: 24,
    })
    .expect("terminal");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !temp.path().join("ready").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = Arc::clone(&cancelled);
    let (result_tx, result_rx) = mpsc::channel();
    let expected = cccc_runtime::status(&group_id, &actor.id).expect("terminal status");
    let thread = std::thread::spawn(move || {
        let result =
            submit_terminal_text_for_session(&expected, &actor, &"x".repeat(1024 * 1024), &cancel);
        result_tx.send(result).expect("result receiver");
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    let blocked = !thread.is_finished();
    let worker = DeliveryWorker {
        sender: None,
        cancelled,
        replaced: Arc::new(AtomicBool::new(false)),
        thread: Some(thread),
    };
    let (done_tx, done_rx) = mpsc::channel();
    let shutdown = std::thread::spawn(move || {
        worker.shutdown();
        done_tx.send(()).expect("shutdown receiver");
    });
    let stopped = done_rx.recv_timeout(std::time::Duration::from_secs(1));
    cccc_runtime::stop(&group_id, "peer").expect("cleanup");
    shutdown.join().expect("shutdown thread");
    assert!(blocked, "fixture must block input before shutdown");
    stopped.expect("delivery shutdown must finish before stopping the runtime");
    assert!(matches!(
        result_rx.recv().expect("cancelled submission"),
        cccc_runtime::PtyHandoff::Uncertain(cccc_runtime::PtyHandoffReason::Cancelled)
            | cccc_runtime::PtyHandoff::NotWritten(cccc_runtime::PtyHandoffReason::Cancelled)
    ));
}

#[test]
fn worker_shutdown_releases_claims_as_retryable_failures() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("paused delivery", "").expect("group");
    group.state = GroupState::Paused;
    let actor = Actor::new("peer1");
    group.actors.push(actor.clone());
    store.save(&group).expect("save actor");

    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["peer1"],"text":"retry me","message_mode":"send"})
        .as_object()
        .cloned()
        .expect("event data");
    ledger::append(
        &store.ledger_path(&group.group_id).expect("ledger path"),
        &event,
    )
    .expect("append source");
    crate::ops::runtime_delivery::append_state(
        &home,
        &group.group_id,
        &actor.id,
        &actor.created_at,
        &event.id,
        delivery_transport(&home, &group, &actor).expect("owner"),
        crate::ops::runtime_delivery::DeliveryOutcome::Claimed,
    )
    .expect("claim");

    assert_eq!(
        offer_delivery(&home, &group, &actor, &event, "pty", true, None),
        Offer::Queued
    );
    shutdown_actor(&group.group_id, &actor.id);

    assert_eq!(
        crate::ops::runtime_delivery::latest_state(&home, &group.group_id, &actor.id, &event.id,)
            .expect("latest state")
            .expect("delivery state")
            .0,
        "failed"
    );
    assert!(
        !in_flight()
            .lock()
            .expect("in flight")
            .iter()
            .any(|item| item.0 == group.group_id)
    );
}
#[test]
fn work_a_switch_fence_refuses_is_given_back_before_the_fence_lifts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("fenced delivery", "").expect("group");
    let actor = Actor::new("peer1");
    group.actors.push(actor.clone());
    store.save(&group).expect("save actor");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["peer1"],"text":"after the switch","message_mode":"send"})
        .as_object()
        .cloned()
        .expect("event data");
    ledger::append(
        &store.ledger_path(&group.group_id).expect("ledger path"),
        &event,
    )
    .expect("append source");
    // Claimed by a dispatch that passed its check just before the fence.
    crate::ops::runtime_delivery::append_state(
        &home,
        &group.group_id,
        &actor.id,
        &actor.created_at,
        &event.id,
        delivery_transport(&home, &group, &actor).expect("owner"),
        crate::ops::runtime_delivery::DeliveryOutcome::Claimed,
    )
    .expect("claim");
    let fence = fence_actor(&group.group_id, &actor.id);

    let offered = offer_delivery(&home, &group, &actor, &event, "pty", true, None);
    drop(fence);

    assert_eq!(offered, Offer::Held);
    assert_eq!(
        crate::ops::runtime_delivery::latest_state(&home, &group.group_id, &actor.id, &event.id,)
            .expect("latest state")
            .expect("delivery state")
            .0,
        "failed",
        "unsubmitted is recorded as a retryable failure"
    );
    assert!(
        !workers()
            .lock()
            .expect("workers")
            .contains_key(&(group.group_id.clone(), actor.id.clone())),
        "a fenced member gets no worker"
    );
    assert!(
        !in_flight()
            .lock()
            .expect("in flight")
            .iter()
            .any(|item| item.0 == group.group_id)
    );
    let pending = crate::ops::runtime_delivery::pending_sources(&home, &group, &actor, 10)
        .expect("pending deliveries");
    assert_eq!(
        pending.iter().map(|event| &event.id).collect::<Vec<_>>(),
        [&event.id],
        "the switch's own dispatch offers it again"
    );
}

struct GroupGuard(String);
impl Drop for GroupGuard {
    fn drop(&mut self) {
        shutdown_group(&self.0);
    }
}

fn install_stub_worker(group_id: &str, actor_id: &str) -> mpsc::Receiver<DeliveryJob> {
    let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
    workers().lock().expect("workers").insert(
        (group_id.to_owned(), actor_id.to_owned()),
        DeliveryWorker {
            sender: Some(tx),
            cancelled: Arc::new(AtomicBool::new(false)),
            replaced: Arc::new(AtomicBool::new(false)),
            thread: None,
        },
    );
    rx
}

fn append_send(ledger_path: &std::path::Path, group_id: &str, to: &str, text: &str) -> Event {
    let mut event = Event::new("chat.message", group_id);
    event.by = "user".into();
    event.data = json!({"to":[to],"text":text,"message_mode":"send"})
        .as_object()
        .cloned()
        .expect("event data");
    ledger::append(ledger_path, &event).expect("append send");
    event
}

#[test]
fn non_external_queue_full_records_unsubmitted_and_recovers_in_order() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("queue full", "").expect("group");
    let _guard = GroupGuard(group.group_id.clone());
    let actor = Actor::new("peer1");
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let rx = install_stub_worker(&group.group_id, &actor.id);

    for index in 0..QUEUE_CAPACITY {
        let filler = append_send(&path, &group.group_id, &actor.id, &format!("fill-{index}"));
        let report = dispatch(&home, &group, &filler);
        assert_eq!(report.queued, 1);
    }

    let first_overflow = append_send(&path, &group.group_id, &actor.id, "overflow-1");
    let second_overflow = append_send(&path, &group.group_id, &actor.id, "overflow-2");
    assert_eq!(dispatch(&home, &group, &first_overflow).queued, 0);
    // Subsequent send joins the open gap and cannot overtake the rejected one.
    assert_eq!(dispatch(&home, &group, &second_overflow).queued, 0);

    let records = ledger::read_all(&path).expect("ledger");
    let rejected = records
        .iter()
        .rfind(|record| {
            record.kind == "runtime.delivery" && record.data["source_event_id"] == first_overflow.id
        })
        .expect("rejected delivery record");
    assert_eq!(rejected.data["state"], "failed");
    assert_eq!(rejected.data["retryable"], true);
    assert!(
        !records.iter().any(|record| {
            record.kind == "runtime.delivery"
                && record.data["source_event_id"] == second_overflow.id
        }),
        "second overflow stays behind the open gap without overtaking"
    );
    assert!(pending_group_ids().contains(&group.group_id));

    // Free two slots in the worker queue and drain the group.
    for _ in 0..2 {
        let job = rx.try_recv().expect("queued filler");
        complete_job(&job, None);
    }
    drain_group(&home, &group.group_id);

    let mut remaining: Vec<String> = rx.try_iter().map(|job| job.event.id).collect();
    let tail = remaining.split_off(remaining.len() - 2);
    assert_eq!(tail, vec![first_overflow.id, second_overflow.id]);
    // One more drain with capacity clears the completed gap.
    clear_in_flight(|item| item.0 == group.group_id);
    drain_group(&home, &group.group_id);
    assert!(!pending_group_ids().contains(&group.group_id));
}

#[test]
fn non_external_dispatch_unread_pages_beyond_queue_capacity_in_order() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("backlog paging", "").expect("group");
    let _guard = GroupGuard(group.group_id.clone());
    let actor = Actor::new("peer1");
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let rx = install_stub_worker(&group.group_id, &actor.id);

    let total = QUEUE_CAPACITY + 4;
    let ids: Vec<String> = (0..total)
        .map(|index| append_send(&path, &group.group_id, &actor.id, &format!("msg-{index}")).id)
        .collect();

    assert_eq!(
        dispatch_unread(&home, &group, &actor.id),
        QUEUE_CAPACITY,
        "first page fills the bounded queue"
    );
    assert!(
        pending_group_ids().contains(&group.group_id),
        "full page schedules catch-up for the tail"
    );

    // Draining while all slots are occupied does not duplicate or drop the tail.
    drain_group(&home, &group.group_id);
    let first_page: Vec<String> = rx
        .try_iter()
        .map(|job| {
            complete_job(&job, None);
            job.event.id
        })
        .collect();
    assert_eq!(first_page, ids[..QUEUE_CAPACITY]);

    // Once slots free up, maintenance delivers the tail in exact ledger order.
    drain_group(&home, &group.group_id);
    let second_page: Vec<String> = rx
        .try_iter()
        .map(|job| {
            complete_job(&job, None);
            job.event.id
        })
        .collect();
    assert_eq!(second_page, ids[QUEUE_CAPACITY..]);
    assert!(!pending_group_ids().contains(&group.group_id));
}

#[test]
fn dispatch_unread_pending_sources_error_records_catchup_and_recovers() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("pending read error", "").expect("group");
    let _guard = GroupGuard(group.group_id.clone());
    let pty_actor = Actor::new("pty_peer");
    let mut native_actor = Actor::new("native_peer");
    native_actor.external_session_id = "ext_session_1".into();
    group
        .actors
        .extend([pty_actor.clone(), native_actor.clone()]);
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger");

    let pty_msg = append_send(&path, &group.group_id, &pty_actor.id, "for pty");
    let native_msg = append_send(&path, &group.group_id, &native_actor.id, "for native");

    // Corrupt read_cursors.json so pending_sources fails before any worker exists.
    let cursor_path = store
        .state_dir(&group.group_id)
        .expect("state dir")
        .join("read_cursors.json");
    std::fs::write(&cursor_path, b"{not valid json").expect("corrupt cursors");

    assert_eq!(dispatch_unread(&home, &group, &pty_actor.id), 0);
    assert_eq!(dispatch_unread(&home, &group, &native_actor.id), 0);
    assert!(pending_group_ids().contains(&group.group_id));

    // Install stub workers and restore the cursor file, then drain.
    let pty_rx = install_stub_worker(&group.group_id, &pty_actor.id);
    let native_rx = install_stub_worker(&group.group_id, &native_actor.id);
    std::fs::remove_file(&cursor_path).expect("restore cursors");

    drain_group(&home, &group.group_id);
    assert_eq!(
        pty_rx
            .try_iter()
            .map(|job| job.event.id)
            .collect::<Vec<_>>(),
        vec![pty_msg.id]
    );
    assert_eq!(
        native_rx
            .try_iter()
            .map(|job| job.event.id)
            .collect::<Vec<_>>(),
        vec![native_msg.id]
    );
    assert!(!pending_group_ids().contains(&group.group_id));
}

#[test]
fn already_in_flight_offer_does_not_overwrite_claimed_receipt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("already in flight", "").expect("group");
    let _guard = GroupGuard(group.group_id.clone());
    let actor = Actor::new("peer1");
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let event = append_send(&path, &group.group_id, &actor.id, "in flight");

    crate::ops::runtime_delivery::append_state(
        &home,
        &group.group_id,
        &actor.id,
        &actor.created_at,
        &event.id,
        "pty",
        crate::ops::runtime_delivery::DeliveryOutcome::Claimed,
    )
    .expect("claim");
    in_flight().lock().expect("in flight").insert((
        group.group_id.clone(),
        actor.id.clone(),
        event.id.clone(),
    ));

    assert_eq!(
        offer_delivery(&home, &group, &actor, &event, "pty", true, None),
        Offer::AlreadyInFlight
    );
    let other = Actor::new("other");
    let targets = [(&other, "pty"), (&actor, "pty")];
    let Err(error) = reserve_explicit_deliveries(&group, &event, &targets) else {
        panic!("the existing worker owns the second slot");
    };
    assert_eq!(error.code, "delivery_in_progress");
    assert!(
        !in_flight().lock().expect("in flight").contains(&(
            group.group_id.clone(),
            other.id,
            event.id.clone()
        )),
        "rejecting the whole request releases earlier reservations"
    );
    let state =
        crate::ops::runtime_delivery::latest_state(&home, &group.group_id, &actor.id, &event.id)
            .expect("state");
    assert_eq!(state.map(|(s, _)| s).as_deref(), Some("claimed"));
    assert!(!pending_group_ids().contains(&group.group_id));
}

#[test]
fn queued_managed_retry_keeps_its_claim_when_recovery_or_send_now_runs() {
    use crate::ops::runtime_delivery::{self, ClaimResult, DeliveryOutcome};

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("queued retry", "").expect("group");
    let _guard = GroupGuard(group.group_id.clone());
    let mut actor = Actor::new("peer1");
    actor.runtime = ActorRuntime::Deepseek;
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger");

    for (outcome, gap_eligible) in [
        (DeliveryOutcome::Failed("turn failed"), false),
        (DeliveryOutcome::Unsubmitted("no input was sent"), true),
    ] {
        let event = append_send(&path, &group.group_id, &actor.id, "queued work");
        let _worker_slot = DeliverySlot::reserve(&group.group_id, &actor.id, &event.id)
            .expect("reserve the live worker's slot");
        let receipt = runtime_delivery::append_state(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            &event.id,
            "deepseek",
            outcome,
        )
        .expect("settle the preceding attempt");
        let latest = || {
            inbox::latest_delivery_receipt(&home, &group, &actor.id, &event.id)
                .expect("load latest receipt")
                .expect("delivery receipt")
                .id
        };
        let args = json!({
            "group_id": group.group_id,
            "actor_ids": [actor.id],
            "source_event_id": event.id,
        });
        let response = crate::dispatch::dispatch(
            &home,
            &cccc_contracts::DaemonRequest {
                v: 1,
                op: "message_deliver".into(),
                args: args.as_object().expect("delivery args").clone(),
            },
        );
        assert_eq!(
            response.error.expect("already in the worker").code,
            "delivery_in_progress"
        );
        assert_eq!(latest(), receipt.id, "Send now cannot take the live slot");
        assert_eq!(dispatch_unread(&home, &group, &actor.id), 0);
        assert_eq!(
            latest(),
            receipt.id,
            "unread recovery cannot claim it either"
        );

        if gap_eligible {
            gaps::record(&group, &actor, Some(&event));
            let prepared = gaps::prepare_group(
                &home,
                &group,
                &crate::session_owner::OwnerObservations::default(),
            );
            assert!(!prepared.is_empty(), "include the known no-handoff receipt");
            gaps::apply_group(&home, &group, prepared);
            assert_eq!(latest(), receipt.id, "gap recovery cannot claim it either");
        }
        assert_eq!(
            runtime_delivery::claim_current_attempt(&home, &group, &actor, &event.id, "deepseek")
                .expect("the live worker can reclaim its own work"),
            ClaimResult::Claimed
        );
    }
}

#[test]
fn confirmation_paused_backlog_does_not_schedule_refill_spin() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("paused confirmation", "").expect("group");
    let _guard = GroupGuard(group.group_id.clone());
    let mut actor = Actor::new("claude");
    actor.runtime = ActorRuntime::Claude;
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    for index in 0..QUEUE_CAPACITY {
        append_send(&path, &group.group_id, &actor.id, &format!("wait-{index}"));
    }
    crate::ops::local_headless::pause_confirmation(&group.group_id, &actor.id)
        .expect("pause confirmation");
    struct PauseGuard(String, String);
    impl Drop for PauseGuard {
        fn drop(&mut self) {
            let _ = crate::ops::local_headless::retry_confirmation(&self.0, &self.1);
        }
    }
    let _pause_guard = PauseGuard(group.group_id.clone(), actor.id.clone());

    assert_eq!(dispatch_unread(&home, &group, &actor.id), 0);
    assert!(
        !pending_group_ids().contains(&group.group_id),
        "paused confirmation backlog must not keep a maintenance refill spinning"
    );
}
