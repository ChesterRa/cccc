use super::super::*;
use crate::ops::codex_voice_lifecycle::{
    AnalystLifecycle, AnalystLifecycleEvent, VoiceDelegationAdmission,
};
use cccc_contracts::RuntimeMode;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tokio::sync::broadcast;

fn fixture() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    crate::antigravity_acp_setup::install_fixture(&home, include_str!("antigravity_fixture.py"));
    (temp, home)
}

async fn analyst(home: &HomeLayout, root: &Path) -> AnalystSession {
    AnalystSession::launch(
        home,
        LaunchConfig {
            workdir: root.into(),
            runtime: ActorRuntime::Antigravity,
            runtime_mode: RuntimeMode::Acp,
            command: vec!["agy".into(), "--model=gemini-fixture".into()],
            environment: BTreeMap::from([("AGY_FIXTURE_CLI".into(), "/bin/true".into())]),
            resume_thread_id: None,
        },
    )
    .await
    .expect("official adapter")
}

async fn completion(
    events: &mut broadcast::Receiver<crate::ops::codex_voice_lifecycle::AnalystLifecycleEvent>,
) -> (String, String) {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let AnalystLifecycleEvent::Completed {
                delegation_id,
                result,
                ..
            } = events.recv().await.expect("event")
            {
                return (delegation_id, result);
            }
        }
    })
    .await
    .expect("completion")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_voice_default_approves_and_drains_queue_on_new_and_resumed_sessions() {
    let (temp, home) = fixture();
    let settings = cccc_contracts::CodexVoiceAnalystSettings {
        runtime: ActorRuntime::Antigravity,
        runtime_mode: RuntimeMode::Acp,
        command: Vec::new(),
        ..Default::default()
    };
    cccc_core::codex_voice_settings::save(&home, &settings).expect("save default settings");
    let mut resume = None;
    for _ in 0..2 {
        let saved = cccc_core::codex_voice_settings::load(&home).expect("saved settings");
        assert!(saved.command.is_empty(), "retain the use-default sentinel");
        let resolved = cccc_core::codex_voice_settings::resolve(
            &home,
            &saved,
            &BTreeMap::from([("AGY_FIXTURE_CLI".into(), "/bin/true".into())]),
        )
        .expect("resolve default settings");
        let session = Arc::new(
            AnalystSession::launch(
                &home,
                LaunchConfig {
                    workdir: temp.path().into(),
                    runtime: resolved.runtime,
                    runtime_mode: resolved.runtime_mode,
                    command: resolved.command,
                    environment: resolved.environment,
                    resume_thread_id: resume.clone(),
                },
            )
            .await
            .expect("launch default Analyst"),
        );
        let requests = std::fs::read_to_string(temp.path().join("fixture_requests.jsonl"))
            .expect("initialization requests");
        let mode = requests
            .lines()
            .filter_map(|line| {
                let frame: Value = serde_json::from_str(line).expect("frame");
                (frame["method"] == "session/set_config_option"
                    && frame["params"]["configId"] == "mode")
                    .then(|| frame["params"]["value"].as_str().expect("mode").to_owned())
            })
            .next_back()
            .expect("permission mode");
        if mode != "yolo" {
            session
                .stop(session.generation())
                .await
                .expect("stop mismatched fixture");
        }
        assert_eq!(
            mode, "yolo",
            "default Voice launch must use the Actor permission default"
        );
        assert_eq!(session.thread_resumed, resume.is_some());
        if let Some(id) = &resume {
            assert_eq!(session.thread_id(), id);
        }
        resume = Some(session.thread_id().to_owned());
        let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
        let mut events = lifecycle.subscribe();
        lifecycle.admit_voice("hold", "HOLD").await.expect("hold");
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if matches!(
                    events.recv().await.expect("event"),
                    AnalystLifecycleEvent::Started { .. }
                ) {
                    break;
                }
            }
        })
        .await
        .expect("started");
        for (id, text) in [("permission", "PERMISSION"), ("after", "after permission")] {
            assert!(matches!(
                lifecycle.admit_voice(id, text).await.expect("queue"),
                VoiceDelegationAdmission::Queued { .. }
            ));
        }
        let ManagedProtocol::Acp(protocol) = &session.protocol else {
            panic!("ACP")
        };
        protocol
            .request("fixture/finish", json!({}), Duration::from_secs(2))
            .await
            .expect("finish held turn");
        assert_eq!(completion(&mut events).await.0, "hold");
        let (id, result) = completion(&mut events).await;
        assert_eq!(id, "permission");
        assert!(
            result.contains("once"),
            "automatic one-time approval: {result}"
        );
        assert!(!result.contains("never"));
        assert_eq!(completion(&mut events).await.0, "after");
        assert!(session.permissions().is_empty());
        assert_eq!(lifecycle.queued_inputs(), 0);
        session.stop(session.generation()).await.expect("stop");
    }
    let frames: Vec<Value> = std::fs::read_to_string(temp.path().join("fixture_requests.jsonl"))
        .expect("requests")
        .lines()
        .map(|line| serde_json::from_str(line).expect("frame"))
        .collect();
    let modes: Vec<_> = frames
        .iter()
        .filter(|frame| {
            frame["method"] == "session/set_config_option" && frame["params"]["configId"] == "mode"
        })
        .map(|frame| frame["params"]["value"].as_str().expect("mode"))
        .collect();
    assert_eq!(modes, ["yolo", "yolo"], "reapply after both new and load");
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["method"] == "session/new")
            .count(),
        1
    );
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["method"] == "session/load")
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_permissions_and_fifo_use_one_shared_adapter_without_terminal() {
    let (temp, home) = fixture();
    let session = Arc::new(analyst(&home, temp.path()).await);
    assert!(session.structured_only());
    assert!(!session.tui_ready());
    assert!(session.tui_command().is_empty());
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    assert!(matches!(
        lifecycle
            .admit_voice("first", "HOLD first")
            .await
            .expect("fixture operation"),
        VoiceDelegationAdmission::Queued { position: 1, .. }
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(
                events.recv().await.expect("fixture operation"),
                AnalystLifecycleEvent::Started { .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("fixture operation");
    assert!(
        lifecycle
            .admit_voice("first", "different content")
            .await
            .is_err()
    );
    assert!(matches!(
        lifecycle
            .admit_terminal("second", "second instruction")
            .await
            .expect("fixture operation"),
        VoiceDelegationAdmission::Queued { position: 1, .. }
    ));
    let ManagedProtocol::Acp(protocol) = &session.protocol else {
        panic!("ACP")
    };
    protocol
        .request("fixture/finish", json!({}), Duration::from_secs(2))
        .await
        .expect("fixture operation");
    assert_eq!(completion(&mut events).await.0, "first");
    let (id, text) = completion(&mut events).await;
    assert_eq!(id, "second");
    assert!(text.contains("second instruction"));
    assert!(matches!(
        lifecycle
            .admit_voice("first", "HOLD first")
            .await
            .expect("fixture operation"),
        VoiceDelegationAdmission::Turn(_)
    ));
    lifecycle
        .admit_voice("third", "PERMISSION")
        .await
        .expect("fixture operation");
    tokio::time::timeout(Duration::from_secs(3), async {
        while session.permissions().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture operation");
    let permission = session.permissions().pop().expect("fixture operation");
    let id = permission["request_id"]
        .as_str()
        .expect("fixture operation");
    assert!(
        session
            .respond_permission("wrong-generation", id, true)
            .await
            .is_err()
    );
    session
        .respond_permission(session.generation(), id, false)
        .await
        .expect("fixture operation");
    let (id, text) = completion(&mut events).await;
    assert_eq!(id, "third");
    assert!(text.contains("never"));
    assert!(session.permissions().is_empty());
    assert!(
        session
            .respond_permission(session.generation(), "\"permission-opaque-id\"", true)
            .await
            .is_err()
    );
    let requests = std::fs::read_to_string(temp.path().join("fixture_requests.jsonl"))
        .expect("fixture operation");
    assert_eq!(
        requests.matches("CCCC Realtime Voice").count(),
        1,
        "host instructions injected only once"
    );
    assert!(!text.contains("REPLAY"));
    assert!(!text.contains("FOREIGN"));
    session
        .stop(session.generation())
        .await
        .expect("fixture operation");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_cancel_clears_queued_input_and_pending_permission() {
    let (temp, home) = fixture();
    let session = Arc::new(analyst(&home, temp.path()).await);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    lifecycle
        .admit_voice("cancel", "PERMISSION")
        .await
        .expect("fixture operation");
    tokio::time::timeout(Duration::from_secs(3), async {
        while session.permissions().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture operation");
    lifecycle
        .admit_voice("never-submit", "must stay queued")
        .await
        .expect("fixture operation");
    assert!(lifecycle.cancel_current().await.expect("fixture operation"));
    assert_eq!(lifecycle.queued_inputs(), 0);
    assert!(session.permissions().is_empty());
    let requests = std::fs::read_to_string(temp.path().join("fixture_requests.jsonl"))
        .expect("fixture operation");
    assert!(!requests.contains("must stay queued"));
    let _ = completion(&mut events).await;
    assert!(!lifecycle.is_busy().await);
    session
        .stop(session.generation())
        .await
        .expect("fixture operation");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_queue_rejects_overflow_without_eviction_and_cancel_does_not_submit_it() {
    let (temp, home) = fixture();
    let session = Arc::new(analyst(&home, temp.path()).await);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    lifecycle
        .admit_voice("active", "HOLD active")
        .await
        .expect("first input");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(
                events.recv().await.expect("event"),
                AnalystLifecycleEvent::Started { .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("admitted input");
    for index in 0..32 {
        let input = format!("queued-{index}");
        lifecycle
            .admit_voice(&input, &input)
            .await
            .expect("queue input");
    }
    assert!(lifecycle.admit_voice("overflow", "overflow").await.is_err());
    assert_eq!(lifecycle.queued_inputs(), 32);
    assert!(
        lifecycle
            .cancel_current()
            .await
            .expect("cancel current and queue")
    );
    assert_eq!(lifecycle.queued_inputs(), 0);
    let requests =
        std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("requests");
    assert!(!requests.contains("queued-"));
    assert!(!requests.contains("overflow"));
    assert!(
        lifecycle.admit_voice("queued-0", "queued-0").await.is_err(),
        "cancelled input is never silently replayed"
    );
    session.stop(session.generation()).await.expect("stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_actor_receipts_isolate_sessions_and_reapply_model_on_resume() {
    let (temp, home) = fixture();
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("fixture operation")
        .create("ACP fixture", "")
        .expect("fixture operation");
    let launch = |actor: &str| ActorLaunchConfig {
        workdir: temp.path().into(),
        group_id: group.group_id.clone(),
        actor_id: actor.into(),
        runtime: ActorRuntime::Antigravity,
        runtime_mode: RuntimeMode::Acp,
        command: vec!["agy".into(), "--model=gemini-fixture".into()],
        environment: BTreeMap::from([("AGY_FIXTURE_CLI".into(), "/bin/true".into())]),
    };
    let first = AnalystSession::launch_actor(&home, launch("alpha"))
        .await
        .expect("fixture operation");
    let second = AnalystSession::launch_actor(&home, launch("beta"))
        .await
        .expect("fixture operation");
    assert_ne!(first.thread_id(), second.thread_id());
    let id = first.thread_id().to_owned();
    first
        .stop(first.generation())
        .await
        .expect("fixture operation");
    let resumed = AnalystSession::launch_actor(&home, launch("alpha"))
        .await
        .expect("fixture operation");
    assert_eq!(resumed.thread_id(), id);
    assert!(resumed.thread_resumed);
    let requests = std::fs::read_to_string(temp.path().join("fixture_requests.jsonl"))
        .expect("fixture operation");
    let frames: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).expect("fixture operation"))
        .collect();
    assert_eq!(
        frames
            .iter()
            .filter(|r| r["method"] == "session/set_config_option"
                && r["params"]["configId"] == "model")
            .count(),
        3
    );
    for actor in ["alpha", "beta"] {
        assert!(frames.iter().any(|r| {
            r["params"]["mcpServers"][0]["env"]
                .as_array()
                .is_some_and(|env| {
                    env.iter()
                        .any(|entry| entry["name"] == "CCCC_ACTOR_ID" && entry["value"] == actor)
                })
        }));
    }
    second
        .stop(second.generation())
        .await
        .expect("fixture operation");
    resumed
        .stop(resumed.generation())
        .await
        .expect("fixture operation");

    std::fs::write(temp.path().join("fixture_reject_load"), b"").expect("reject fixture");
    assert!(
        AnalystSession::launch_actor(&home, launch("alpha"))
            .await
            .is_err()
    );
    let requests =
        std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("requests");
    assert_eq!(
        requests.matches("\"method\": \"session/new\"").count(),
        2,
        "failed resume cannot silently create a new conversation"
    );
    std::fs::remove_file(temp.path().join("fixture_reject_load")).expect("remove flag");
    crate::ops::runtime_session::antigravity::remove(&home, &group.group_id, "alpha")
        .expect("new session");
    let fresh = AnalystSession::launch_actor(&home, launch("alpha"))
        .await
        .expect("fresh session");
    assert_ne!(fresh.thread_id(), id);
    fresh.stop(fresh.generation()).await.expect("stop");
}

#[test]
fn antigravity_supervisor_keeps_actual_acp_surface_after_a_pending_mode_edit() {
    use crate::ops::local_headless as supervisor;
    let (temp, home) = fixture();
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("ACP supervisor", "").expect("group");
    group.scopes.push(cccc_core::Scope {
        scope_key: "fixture".into(),
        url: temp.path().to_string_lossy().into_owned(),
        label: "fixture".into(),
        git_remote: String::new(),
    });
    group.active_scope_key = "fixture".into();
    let mut actor = cccc_contracts::Actor::new("alpha");
    actor.runtime = ActorRuntime::Antigravity;
    actor.runtime_mode = RuntimeMode::Acp;
    actor.command = vec!["agy".into()];
    actor
        .env
        .insert("AGY_FIXTURE_CLI".into(), "/bin/true".into());
    let actor = cccc_core::actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("persist");
    supervisor::start(&home, &group, &actor).expect("start ACP");
    struct Stop(String);
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = crate::ops::local_headless::stop(&self.0, "alpha");
        }
    }
    let _stop = Stop(group.group_id.clone());
    assert!(
        !cccc_runtime::status(&group.group_id, "alpha").is_ok_and(|state| state.running),
        "ACP must not create a dummy terminal"
    );
    let mut pending = actor.clone();
    pending.runtime_mode = RuntimeMode::Default;
    pending.normalize_runtime_constraints();
    group.actors[0] = pending.clone();
    store
        .save(&group)
        .expect("save next-launch native settings");
    let fields =
        crate::ops::working_state::runtime_actor_fields(&home, &pending, &group.group_id, true);
    assert_eq!(fields["runner_effective"], "headless");
    let state = supervisor::structured_state(&group.group_id, "alpha").expect("state");
    assert_eq!(state["runtime_mode"], "acp");
    let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"text":"HOLD supervisor","to":["alpha"],"message_mode":"send"})
        .as_object()
        .expect("message")
        .clone();
    let job = crate::ops::actor_delivery::DeliveryJob {
        home: home.clone(),
        group: group.clone(),
        actor: pending.clone(),
        event,
    };
    assert!(
        matches!(
            crate::ops::actor_delivery_worker::process_batch(
                &mut vec![job.clone()],
                &mut String::new(),
                &std::sync::atomic::AtomicBool::new(false)
            ),
            crate::ops::actor_delivery_worker::BatchOutcome::Delivered
        ),
        "delivery must continue through the actual ACP session"
    );
    assert_eq!(
        crate::ops::runtime_delivery::latest_state(&home, &group.group_id, "alpha", &job.event.id)
            .expect("delivery state")
            .expect("managed receipt"),
        ("accepted".into(), "managed_session".into())
    );
    assert!(supervisor::cancel_turn(&group.group_id, "alpha", "stale").is_err());
    let generation = state["generation"].as_str().expect("generation");
    let started = std::time::Instant::now();
    while !supervisor::structured_state(&group.group_id, "alpha").expect("state")["working"]
        .as_bool()
        .expect("working")
    {
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "turn must become active"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(supervisor::cancel_turn(&group.group_id, "alpha", generation).expect("cancel"));
    let journal = std::fs::read_to_string(
        store
            .state_dir(&group.group_id)
            .expect("state directory")
            .join("headless/events.jsonl"),
    )
    .expect("journal");
    assert!(journal.contains("headless.turn.started"));
    assert!(journal.contains("headless.message.delta"));
    assert!(!journal.contains("FOREIGN"));
    assert!(!journal.contains("REPLAY"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_cancel_before_admission_is_bounded_and_keeps_distinct_outcomes() {
    let (temp, home) = fixture();
    let session = Arc::new(analyst(&home, temp.path()).await);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    lifecycle
        .admit_voice("pending", "DELAY_RECEIPT")
        .await
        .expect("enqueue");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if std::fs::read_to_string(temp.path().join("fixture_requests.jsonl"))
                .unwrap_or_default()
                .contains("DELAY_RECEIPT")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("provider has prompt, no receipt");
    for id in ["source-a", "source-b"] {
        lifecycle
            .begin_actor_result(id, id, false)
            .await
            .expect("queue result");
    }
    let cancelled = tokio::time::timeout(Duration::from_secs(3), lifecycle.cancel_current()).await;
    // Stop our fixture even on the old deadlock, before asserting.
    if cancelled.is_err() {
        session
            .stop(session.generation())
            .await
            .expect("cleanup old deadlock");
    }
    assert!(
        cancelled
            .expect("cancel must not wait for admission")
            .expect("cancel")
    );
    assert_eq!(lifecycle.queued_inputs(), 0);
    let mut outcomes = std::collections::BTreeMap::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while outcomes.len() < 3 {
            if let AnalystLifecycleEvent::Completed {
                turn_id,
                delegation_id,
                status,
                ..
            } = events.recv().await.expect("event")
            {
                assert_eq!(status, "cancelled");
                assert!(!turn_id.is_empty());
                assert!(
                    outcomes.insert(delegation_id, turn_id).is_none(),
                    "one outcome per input"
                );
            }
        }
    })
    .await
    .expect("all cancellation outcomes");
    assert_eq!(
        outcomes
            .values()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    let requests =
        std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("trace");
    assert!(!requests.contains("source-a"));
    assert!(!requests.contains("source-b"));
    let prior = lifecycle
        .admit_voice("pending", "DELAY_RECEIPT")
        .await
        .expect("reuse prior receipt");
    assert!(
        matches!(prior, VoiceDelegationAdmission::Turn(ref receipt) if receipt.turn_id == outcomes["pending"])
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("fixture_requests.jsonl"))
            .expect("trace")
            .matches("DELAY_RECEIPT")
            .count(),
        1
    );
    assert!(!lifecycle.is_busy().await);
    assert!(
        session.process_running(),
        "confirmed cancellation keeps the warm session"
    );
    lifecycle
        .admit_voice("after-cancel", "continue safely")
        .await
        .expect("new input");
    assert_eq!(completion(&mut events).await.0, "after-cancel");
    session.stop(session.generation()).await.expect("stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_queued_cancellations_have_distinct_notification_keys() {
    let (temp, home) = fixture();
    let session = Arc::new(analyst(&home, temp.path()).await);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    lifecycle.admit_voice("held", "HOLD").await.expect("hold");
    tokio::time::timeout(Duration::from_secs(3), async {
        while !matches!(
            events.recv().await.expect("event"),
            AnalystLifecycleEvent::Started { .. }
        ) {}
    })
    .await
    .expect("admitted");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Voice sources", "").expect("group");
    group.actors.push(cccc_contracts::Actor::new("worker"));
    store.save(&group).expect("save");
    let mut prefs = cccc_core::voice_notifications::preferences(&home).expect("prefs");
    prefs.groups.insert(
        group.group_id.clone(),
        cccc_contracts::voice_notifications::NotificationScope::ToUser,
    );
    cccc_core::voice_notifications::save_preferences(&home, prefs).expect("subscribe");
    let mut source_ids = Vec::new();
    for text in ["source-a", "source-b"] {
        let mut source = cccc_contracts::Event::new("chat.message", &group.group_id);
        source.by = "worker".into();
        source.data = json!({"to":["user"],"text":text})
            .as_object()
            .expect("source")
            .clone();
        cccc_core::ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger"),
            &source,
        )
        .expect("append");
        let reference = cccc_contracts::voice_notifications::VoiceMessageRef {
            group_id: group.group_id.clone(),
            event_id: source.id,
        };
        cccc_core::voice_notifications::scan(&home).expect("scan");
        cccc_core::voice_notifications::reserve(&home, &reference, session.generation())
            .expect("reserve");
        let id = reference.correlation_id();
        lifecycle
            .begin_actor_result(&id, text, false)
            .await
            .expect("queue source");
        source_ids.push(id);
    }
    lifecycle.cancel_current().await.expect("cancel");
    let mut keys = std::collections::BTreeSet::new();
    for _ in 0..3 {
        let outcome = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let AnalystLifecycleEvent::Completed {
                    turn_id,
                    delegation_ids,
                    ..
                } = events.recv().await.expect("event")
                {
                    for _ in 0..2 {
                        cccc_core::voice_notifications::processed(
                            &home,
                            &delegation_ids,
                            session.generation(),
                            &turn_id,
                            "Cancelled",
                        )
                        .expect("both Web consumers");
                    }
                    break turn_id;
                }
            }
        })
        .await
        .expect("outcome");
        keys.insert(outcome);
    }
    session.stop(session.generation()).await.expect("stop");
    assert_eq!(keys.len(), 3);
    assert!(!keys.contains(""));
    let snapshot = cccc_core::voice_notifications::snapshot(&home).expect("persisted outcomes");
    assert_eq!(snapshot.results.len(), 2);
    assert!(snapshot.messages.iter().all(|item| item.processed));
    let retained: std::collections::BTreeSet<_> = snapshot
        .results
        .iter()
        .flat_map(|result| result.sources.iter().map(|source| source.correlation_id()))
        .collect();
    assert_eq!(retained, source_ids.into_iter().collect());
}

#[test]
fn antigravity_uncertain_actor_delivery_is_quarantined() {
    use crate::ops::{actor_delivery, local_headless as supervisor, runtime_delivery};
    let (temp, home) = fixture();
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("ACP delivery quarantine", "").expect("group");
    group.scopes.push(cccc_core::Scope {
        scope_key: "fixture".into(),
        url: temp.path().to_string_lossy().into_owned(),
        label: "fixture".into(),
        git_remote: String::new(),
    });
    group.active_scope_key = "fixture".into();
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = ActorRuntime::Antigravity;
    actor.runtime_mode = RuntimeMode::Acp;
    actor.enabled = true;
    actor.command = vec!["agy".into()];
    actor
        .env
        .insert("AGY_FIXTURE_CLI".into(), "/bin/true".into());
    let actor = cccc_core::actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            actor_delivery::shutdown_actor(&self.0, "worker");
            let _ = supervisor::stop(&self.0, "worker");
        }
    }
    let _cleanup = Cleanup(group.group_id.clone());
    supervisor::start(&home, &group, &actor).expect("start");
    let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"text":"DISCONNECT_BEFORE_RECEIPT","to":["worker"],"message_mode":"send"})
        .as_object()
        .expect("message")
        .clone();
    cccc_core::ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
        .expect("source");
    assert_eq!(actor_delivery::dispatch(&home, &group, &event).queued, 1);
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    let outcome = loop {
        let state = runtime_delivery::latest_state(&home, &group.group_id, "worker", &event.id)
            .expect("state");
        if state.as_ref().is_some_and(|(s, _)| s == "ambiguous")
            || std::time::Instant::now() >= deadline
        {
            break state;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    actor_delivery::shutdown_actor(&group.group_id, "worker");
    let requests =
        std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("requests");
    assert_eq!(
        requests.matches("DISCONNECT_BEFORE_RECEIPT").count(),
        1,
        "unconfirmed prompt must not be replayed in another process"
    );
    assert_eq!(outcome.expect("outcome").0, "ambiguous");
    assert_eq!(
        runtime_delivery::latest_state(&home, &group.group_id, "worker", &event.id)
            .expect("durable")
            .expect("state")
            .0,
        "ambiguous",
        "shutdown cannot overwrite uncertainty as retryable failure"
    );
    assert_eq!(actor_delivery::dispatch_unread(&home, &group, "worker"), 0);
}

#[test]
fn antigravity_admitted_failure_exposes_provider_error_in_headless_journal() {
    use crate::ops::{actor_delivery, local_headless as supervisor};
    let (temp, home) = fixture();
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("ACP delivery quarantine", "").expect("group");
    group.scopes.push(cccc_core::Scope {
        scope_key: "fixture".into(),
        url: temp.path().to_string_lossy().into_owned(),
        label: "fixture".into(),
        git_remote: String::new(),
    });
    group.active_scope_key = "fixture".into();
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = ActorRuntime::Antigravity;
    actor.runtime_mode = RuntimeMode::Acp;
    actor.enabled = true;
    actor.command = vec!["agy".into()];
    actor
        .env
        .insert("AGY_FIXTURE_CLI".into(), "/bin/true".into());
    let actor = cccc_core::actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            actor_delivery::shutdown_actor(&self.0, "worker");
            let _ = supervisor::stop(&self.0, "worker");
        }
    }
    let _cleanup = Cleanup(group.group_id.clone());
    supervisor::start(&home, &group, &actor).expect("start");
    let mut failed = cccc_contracts::Event::new("chat.message", &group.group_id);
    failed.data = json!({"text":"FAIL_AFTER_RECEIPT","to":["worker"],"message_mode":"send"})
        .as_object()
        .expect("message")
        .clone();
    assert_eq!(
        supervisor::submit_batch(
            &home,
            &group,
            &actor,
            &[failed],
            &std::sync::atomic::AtomicBool::new(false)
        ),
        supervisor::BatchSubmission::Accepted
    );
    let journal = store
        .state_dir(&group.group_id)
        .expect("state")
        .join("headless/events.jsonl");
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let projected = loop {
        let events = std::fs::read_to_string(&journal).expect("journal");
        if events.contains("Synthetic provider failure") || std::time::Instant::now() >= deadline {
            break events;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(projected.contains("headless.turn.failed"));
    assert!(projected.contains("Synthetic provider failure"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_cancel_before_protocol_dispatch_retires_only_the_named_input() {
    let (temp, home) = fixture();
    let session = analyst(&home, temp.path()).await;
    session
        .cancel_pending_input(session.generation(), "retired")
        .await
        .expect("cancel before prompt dispatch");
    let result = session
        .start_turn(session.generation(), "retired", "must not send")
        .await;
    assert_eq!(
        result.expect_err("cancelled input").kind(),
        std::io::ErrorKind::Interrupted
    );
    session
        .start_turn(session.generation(), "another", "different input")
        .await
        .expect("other input allowed");
    session.stop(session.generation()).await.expect("stop");
    let requests =
        std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("trace");
    assert!(!requests.contains("must not send"));
    assert!(requests.contains("different input"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn antigravity_pre_admission_failures_keep_distinct_outcomes_and_continue() {
    let (temp, home) = fixture();
    let session = Arc::new(analyst(&home, temp.path()).await);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    for id in ["failure-a", "failure-b"] {
        lifecycle
            .admit_voice(id, "REJECT_BEFORE_RECEIPT")
            .await
            .expect("queue");
    }
    let mut keys = std::collections::BTreeSet::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while keys.len() < 2 {
            if let AnalystLifecycleEvent::Completed {
                turn_id, status, ..
            } = events.recv().await.expect("event")
            {
                assert_eq!(status, "failed");
                assert!(!turn_id.is_empty());
                assert!(keys.insert(turn_id));
            }
        }
    })
    .await
    .expect("both errors");
    assert!(
        lifecycle
            .admit_voice("failure-a", "REJECT_BEFORE_RECEIPT")
            .await
            .is_err(),
        "do not silently replay"
    );
    lifecycle
        .admit_voice("after-failure", "continue")
        .await
        .expect("next input");
    assert_eq!(completion(&mut events).await.0, "after-failure");
    session.stop(session.generation()).await.expect("stop");
}

#[test]
fn antigravity_stalled_admission_can_stop_without_replay() {
    use crate::ops::{actor_delivery, local_headless as supervisor, runtime_delivery};
    let (temp, home) = fixture();
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("ACP delivery quarantine", "").expect("group");
    group.scopes.push(cccc_core::Scope {
        scope_key: "fixture".into(),
        url: temp.path().to_string_lossy().into_owned(),
        label: "fixture".into(),
        git_remote: String::new(),
    });
    group.active_scope_key = "fixture".into();
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = ActorRuntime::Antigravity;
    actor.runtime_mode = RuntimeMode::Acp;
    actor.enabled = true;
    actor.command = vec!["agy".into()];
    actor
        .env
        .insert("AGY_FIXTURE_CLI".into(), "/bin/true".into());
    let actor = cccc_core::actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            actor_delivery::shutdown_actor(&self.0, "worker");
            let _ = supervisor::stop(&self.0, "worker");
        }
    }
    let _cleanup = Cleanup(group.group_id.clone());
    supervisor::start(&home, &group, &actor).expect("start");
    let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"text":"DELAY_RECEIPT","to":["worker"],"message_mode":"send"})
        .as_object()
        .expect("message")
        .clone();
    cccc_core::ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
        .expect("source");
    assert_eq!(actor_delivery::dispatch(&home, &group, &event).queued, 1);
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    loop {
        let frames =
            std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).unwrap_or_default();
        if frames.contains("DELAY_RECEIPT") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "provider must receive prompt"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let (done, stopped) = std::sync::mpsc::channel();
    let group_id = group.group_id.clone();
    let stop = std::thread::spawn(move || {
        actor_delivery::shutdown_actor(&group_id, "worker");
        done.send(()).expect("notify stop");
    });
    let bounded = stopped.recv_timeout(Duration::from_secs(2)).is_ok();
    // Always release the fixture, even if admission regresses and blocks join.
    supervisor::stop(&group.group_id, "worker").expect("provider cleanup");
    stop.join().expect("worker cleanup");
    assert!(bounded, "Actor shutdown must not wait for an ACP receipt");
    let outcome = runtime_delivery::latest_state(&home, &group.group_id, "worker", &event.id)
        .expect("durable state");
    let requests =
        std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("requests");
    assert_eq!(
        requests.matches("DELAY_RECEIPT").count(),
        1,
        "unconfirmed prompt must not be replayed in another process"
    );
    assert_eq!(outcome.expect("outcome").0, "ambiguous");
    assert_eq!(
        runtime_delivery::latest_state(&home, &group.group_id, "worker", &event.id)
            .expect("durable")
            .expect("state")
            .0,
        "ambiguous",
        "shutdown cannot overwrite uncertainty as retryable failure"
    );
    assert_eq!(actor_delivery::dispatch_unread(&home, &group, "worker"), 0);
}
