use super::*;
use crate::ops::{actor_delivery::DeliveryJob, actor_delivery_worker};
use cccc_contracts::RunnerKind;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn launches(fixture: &Fixture) -> usize {
    std::fs::read_to_string(fixture.config.join("launches"))
        .expect("fake provider launches")
        .lines()
        .count()
}

#[test]
fn a_confirmation_the_user_is_answering_spends_no_automatic_start() {
    if isolated("pause_tests::a_confirmation_the_user_is_answering_spends_no_automatic_start") {
        return;
    }
    let fixture = Fixture::new();
    let started = fixture.request("actor_start", Value::Null);
    assert!(started.ok, "{:?}", started.error);
    assert_eq!(started.result["runtime_confirmation_required"], true);
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    let ledger_path = store
        .ledger_path(&fixture.group.group_id)
        .expect("ledger path");
    let mut event = Event::new("chat.message", &fixture.group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["claude"],"text":"waits for the answer","message_mode":"send"})
        .as_object()
        .expect("message")
        .clone();
    ledger::append(&ledger_path, &event).expect("queued message");
    let mut jobs = vec![DeliveryJob {
        home: fixture.home.clone(),
        group: fixture.group.clone(),
        actor: fixture.group.actors[0].clone(),
        event,
        claimed: true,
        transport: "managed_session",
    }];
    let mut wakes = actor_delivery_worker::WakeBudget::default();

    // More rounds than a batch has starts, each with its wait already over.
    for _ in 0..12 {
        wakes.elapse();
        assert_eq!(
            actor_delivery_worker::process_batch_tracked(
                &mut jobs,
                &mut String::new(),
                &AtomicBool::new(false),
                &mut std::collections::HashSet::new(),
                &mut wakes,
            ),
            actor_delivery_worker::BatchOutcome::Retry
        );
    }
}

#[test]
fn pause_projection_tracks_the_effective_runtime_and_is_not_configuration() {
    if isolated(
        "pause_tests::pause_projection_tracks_the_effective_runtime_and_is_not_configuration",
    ) {
        return;
    }
    let fixture = Fixture::new();
    assert_eq!(fixture.listed()["runtime_confirmation_paused"], false);
    crate::ops::local_headless::pause_confirmation(&fixture.group.group_id, "claude")
        .expect("pause confirmation");
    assert_eq!(fixture.listed()["runtime_confirmation_paused"], true);

    let updated = fixture.request(
        "actor_update",
        json!({"title":"Renamed", "runtime_confirmation_paused":false}),
    );
    assert!(updated.ok, "{:?}", updated.error);
    assert_eq!(updated.result["actor"]["runtime_confirmation_paused"], true);
    assert!(
        updated.result["event"]["data"]["patch"]
            .get("runtime_confirmation_paused")
            .is_none(),
        "observed state is not part of the persisted update event"
    );
    assert!(
        json!(fixture.stored().actors[0])
            .get("runtime_confirmation_paused")
            .is_none()
    );

    let store = GroupStore::new(fixture.home.clone()).expect("store");
    store
        .mutate(&fixture.group.group_id, |group| {
            group.actors[0].runtime = ActorRuntime::Codex;
            Ok(())
        })
        .expect("change configured runtime");
    assert!(
        crate::ops::local_headless::confirmation_paused(&fixture.group.group_id, "claude")
            .expect("pause key is retained")
    );
    assert_eq!(fixture.listed()["runtime_confirmation_paused"], false);

    store
        .mutate(&fixture.group.group_id, |group| {
            group.actors[0].runtime = ActorRuntime::Claude;
            group.actors[0].external_session_id = "claude:native".into();
            Ok(())
        })
        .expect("switch to a native session");
    assert_eq!(fixture.listed()["runtime_confirmation_paused"], false);

    store
        .mutate(&fixture.group.group_id, |group| {
            group.actors[0].runtime = ActorRuntime::Claude;
            group.actors[0].external_session_id.clear();
            group.actors[0].profile_id = "missing-profile".into();
            Ok(())
        })
        .expect("link missing profile");
    assert!(
        fixture
            .listed()
            .get("runtime_confirmation_paused")
            .is_none(),
        "an unresolved Profile must not turn an unknown pause into false"
    );
    let renamed = fixture.request("actor_update", json!({"title":"Still saved"}));
    assert!(
        renamed.ok,
        "projection failure must not fail an update: {:?}",
        renamed.error
    );
    assert_eq!(fixture.stored().actors[0].title, "Still saved");
    assert!(
        renamed.result["actor"]
            .get("runtime_confirmation_paused")
            .is_none()
    );
    assert!(
        !fixture.config.join("launches").exists(),
        "reading the projection must not launch a provider"
    );
}

#[test]
fn closed_confirmation_defers_queued_delivery_until_an_explicit_retry() {
    if isolated("pause_tests::closed_confirmation_defers_queued_delivery_until_an_explicit_retry") {
        return;
    }
    for operation in [
        "actor_start",
        "actor_restart",
        "actor_new_session",
        "actor_update",
        "group_start",
    ] {
        let fixture = Fixture::new();
        let started = fixture.request("actor_start", Value::Null);
        assert!(started.ok, "{:?}", started.error);
        // actor_update starts an enabled member only when the Group is running.
        // Let the prompt survive startup so this precondition is deterministic,
        // then close that fixture process as a user would close the prompt.
        assert!(
            fixture.stored().running,
            "confirmation keeps the Group running"
        );
        let confirmation = cccc_runtime::status(&fixture.group.group_id, "claude")
            .expect("fixture confirmation status");
        assert!(confirmation.running, "fixture confirmation is open");
        let pid = i32::try_from(confirmation.pid.expect("fixture confirmation pid"))
            .expect("fixture confirmation pid fits i32");
        assert!(pid > 1, "fixture confirmation has a child process id");
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        )
        .expect("close fixture confirmation");
        let deadline = Instant::now() + Duration::from_secs(5);
        while crate::ops::local_headless::waiting_for_workspace_trust(
            &fixture.group.group_id,
            "claude",
        )
        .expect("registry")
        {
            assert!(Instant::now() < deadline, "confirmation did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fixture.listed()["runtime_confirmation_paused"], true);
        assert_eq!(launches(&fixture), 1);
        let mut event = Event::new("chat.message", &fixture.group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["claude"],"text":"PENDING_TASK","message_mode":"send"})
            .as_object()
            .expect("event data")
            .clone();
        let store = GroupStore::new(fixture.home.clone()).expect("store");
        let ledger_path = store.ledger_path(&fixture.group.group_id).expect("ledger");
        ledger::append(&ledger_path, &event).expect("queued message");
        let before_actor = json!(fixture.stored().actors[0]);
        let mut jobs = vec![DeliveryJob {
            home: fixture.home.clone(),
            group: fixture.group.clone(),
            actor: fixture.group.actors[0].clone(),
            event: event.clone(),
            claimed: true,
            transport: "managed_session",
        }];
        assert_eq!(
            actor_delivery_worker::process_batch(
                &mut jobs,
                &mut String::new(),
                &AtomicBool::new(false)
            ),
            actor_delivery_worker::BatchOutcome::Delivered
        );
        assert_eq!(
            launches(&fixture),
            1,
            "automatic retries must not reopen a declined prompt"
        );
        let records = ledger::read_all(&ledger_path).expect("delivery records");
        assert!(records.iter().any(|record| {
            record.kind == "runtime.delivery"
                && record.data["source_event_id"] == event.id
                && record.data["state"] == "failed"
                && record.data["retryable"] == true
        }));
        assert!(
            crate::ops::runtime_delivery::pending_sources(
                &fixture.home,
                &fixture.stored(),
                &fixture.group.actors[0],
                20,
            )
            .expect("pending work")
            .iter()
            .any(|pending| pending.id == event.id)
        );
        assert_eq!(json!(fixture.stored().actors[0]), before_actor);

        if operation == "group_start" {
            assert_eq!(fixture.stored().state, cccc_contracts::GroupState::Active);
            store
                .mutate(&fixture.group.group_id, |group| {
                    let mut other = Actor::new("other");
                    other.runtime = ActorRuntime::Custom;
                    other.runner = RunnerKind::Pty;
                    other.command = vec!["/bin/sleep".into(), "60".into()];
                    group.actors.push(other);
                    group.automation = json!({"version":1,"rules":[{
                        "id":"confirmation-retry", "enabled":true, "scope":"group",
                        "trigger":{"kind":"at","at":"2020-01-01T00:00:00Z"},
                        "action":{"kind":"group_state","state":"active"}
                    }]})
                    .as_object()
                    .expect("automation rule")
                    .clone();
                    Ok(())
                })
                .expect("schedule an active-group wake");
            crate::ops::automation_runtime::tick_group(
                &fixture.home,
                &fixture.group.group_id,
                false,
                &AtomicBool::new(false),
            );
            assert_eq!(
                launches(&fixture),
                1,
                "the real scheduled group_start must retain the pause"
            );
            let ordinary = crate::handle_request(
                &fixture.home,
                &DaemonRequest {
                    v: 1,
                    op: "group_start".into(),
                    args: json!({"group_id":fixture.group.group_id,"by":"user","_cccc_automatic_lifecycle":true})
                        .as_object().expect("automatic request").clone(),
                },
            );
            assert!(ordinary.ok, "paused Claude must not block the other member");
            assert!(
                cccc_runtime::status(&fixture.group.group_id, "other")
                    .is_ok_and(|status| status.running)
            );
            assert!(
                crate::ops::local_headless::confirmation_paused(&fixture.group.group_id, "claude")
                    .expect("pause remains")
            );
            assert_eq!(launches(&fixture), 1);
            let stopped = fixture.request("group_stop", Value::Null);
            assert!(stopped.ok, "{:?}", stopped.error);
        }
        let retried = fixture.request(operation, json!({"enabled":true}));
        assert!(retried.ok, "{operation}: {:?}", retried.error);
        assert_eq!(fixture.listed()["runtime_confirmation_paused"], false);
        assert_eq!(
            launches(&fixture),
            2,
            "{operation} must permit a new confirmation attempt"
        );
        if operation == "group_start" {
            assert!(actor_runtime::confirmation_required(
                &fixture.group.group_id,
                "claude"
            ));
        } else {
            assert_eq!(retried.result["runtime_confirmation_required"], true);
        }
    }
}

#[test]
fn paused_confirmation_keeps_new_and_preclaimed_work_retryable() {
    if isolated("pause_tests::paused_confirmation_keeps_new_and_preclaimed_work_retryable") {
        return;
    }
    let fixture = Fixture::new();
    let actor = &fixture.group.actors[0];
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    let ledger_path = store.ledger_path(&fixture.group.group_id).expect("ledger");
    crate::ops::local_headless::pause_confirmation(&fixture.group.group_id, &actor.id)
        .expect("pause");

    let mut events = Vec::new();
    for mode in ["send", "request_reply", "mail"] {
        let mut event = Event::new("chat.message", &fixture.group.group_id);
        event.by = "user".into();
        event.data = json!({"to":[actor.id],"text":mode,"message_mode":mode})
            .as_object()
            .expect("message")
            .clone();
        ledger::append(&ledger_path, &event).expect("append message");
        events.push(event);
    }
    let mut notice = Event::new("system.notify", &fixture.group.group_id);
    notice.by = "system".into();
    notice.data = json!({"kind":"mail_notice","target_actor_id":actor.id,
        "context":{"actor_id":actor.id,"source_event_ids":[events[2].id]}})
    .as_object()
    .expect("notice")
    .clone();
    ledger::append(&ledger_path, &notice).expect("append notice");
    events.push(notice);

    // New messages and notices stay in the ledger, without repeatedly
    // claiming work that cannot be submitted while the confirmation is closed.
    for event in [&events[0], &events[1], &events[3]] {
        assert_eq!(
            actor_delivery::dispatch(&fixture.home, &fixture.group, event).queued,
            0
        );
        assert!(
            crate::ops::runtime_delivery::latest_state(
                &fixture.home,
                &fixture.group.group_id,
                &actor.id,
                &event.id,
            )
            .expect("delivery state")
            .is_none()
        );
    }

    // A claim can already exist when the prompt closes. It is known not to
    // have crossed the provider boundary, including for an explicitly
    // redelivered Mail event, and remains eligible after the pause.
    let mut jobs = Vec::new();
    for event in &events {
        crate::ops::runtime_delivery::claim(
            &fixture.home,
            &fixture.group,
            actor,
            &event.id,
            "pty",
            false,
        )
        .expect("claim");
        jobs.push(DeliveryJob {
            home: fixture.home.clone(),
            group: fixture.group.clone(),
            actor: actor.clone(),
            event: event.clone(),
            claimed: true,
            transport: "pty",
        });
    }
    assert_eq!(
        actor_delivery_worker::process_batch(
            &mut jobs,
            &mut String::new(),
            &AtomicBool::new(false)
        ),
        actor_delivery_worker::BatchOutcome::Delivered
    );
    let records = ledger::read_all(&ledger_path).expect("delivery records");
    for event in &events {
        assert!(
            records.iter().any(|record| {
                record.kind == "runtime.delivery"
                    && record.data["source_event_id"] == event.id
                    && record.data["state"] == "failed"
                    && record.data["retryable"] == true
            }),
            "{} remains retryable",
            event.id
        );
    }
    let pending =
        crate::ops::runtime_delivery::pending_sources(&fixture.home, &fixture.group, actor, 20)
            .expect("pending work");
    for event in &events {
        assert!(
            pending.iter().any(|item| item.id == event.id),
            "{} was lost",
            event.id
        );
    }

    crate::ops::local_headless::retry_confirmation(&fixture.group.group_id, &actor.id)
        .expect("explicit retry");
    assert_eq!(
        actor_delivery::dispatch_unread(&fixture.home, &fixture.group, &actor.id),
        events.len(),
        "the explicit retry rediscovers each event once"
    );
}

#[test]
fn paused_claude_does_not_block_a_profile_switched_to_another_runtime() {
    if isolated("pause_tests::paused_claude_does_not_block_a_profile_switched_to_another_runtime") {
        return;
    }
    let fixture = Fixture::new();
    let profile = cccc_core::profiles::ProfileStore::new(fixture.home.clone())
        .expect("profile store")
        .upsert(
            json!({"name":"Custom fixture","runtime":"custom","command":["/bin/sleep","60"]})
                .as_object()
                .expect("profile")
                .clone(),
            None,
        )
        .expect("profile");
    let store = GroupStore::new(fixture.home.clone()).expect("group store");
    let group = store
        .mutate(&fixture.group.group_id, |group| {
            group.actors[0].profile_id = profile["id"].as_str().expect("profile id").into();
            Ok(group.clone())
        })
        .expect("link profile");
    assert_eq!(group.actors[0].runtime, ActorRuntime::Claude);
    assert_eq!(
        actor_profile_runtime::resolve(&fixture.home, &group.actors[0])
            .expect("effective profile")
            .runtime,
        ActorRuntime::Custom
    );
    crate::ops::local_headless::pause_confirmation(&group.group_id, "claude").expect("old pause");
    assert_eq!(
        fixture.listed()["runtime_confirmation_paused"],
        false,
        "a retained pause does not describe the effective Custom Profile"
    );
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["claude"],"text":"PENDING_TASK","message_mode":"send"})
        .as_object()
        .expect("message")
        .clone();
    ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
        .expect("append message");
    assert_eq!(
        actor_delivery::dispatch(&fixture.home, &group, &event).queued,
        1,
        "an old Claude pause must not suppress the effective Custom runtime"
    );
}

#[test]
fn queued_retry_during_confirmation_exit_never_opens_a_second_prompt() {
    if isolated("pause_tests::queued_retry_during_confirmation_exit_never_opens_a_second_prompt") {
        return;
    }
    let fixture = Fixture::new();
    let started = fixture.request("actor_start", Value::Null);
    assert!(started.ok, "{:?}", started.error);
    let mut event = Event::new("chat.message", &fixture.group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["claude"],"text":"PENDING_TASK","message_mode":"send"})
        .as_object()
        .expect("event data")
        .clone();
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    ledger::append(
        &store.ledger_path(&fixture.group.group_id).expect("ledger"),
        &event,
    )
    .expect("queue before closing confirmation");
    let id = event.id.clone();
    let mut jobs = vec![DeliveryJob {
        home: fixture.home.clone(),
        group: fixture.group.clone(),
        actor: fixture.group.actors[0].clone(),
        event,
        claimed: true,
        transport: "managed_session",
    }];
    assert_eq!(
        actor_delivery_worker::process_batch(
            &mut jobs,
            &mut String::new(),
            &AtomicBool::new(false)
        ),
        actor_delivery_worker::BatchOutcome::Retry
    );
    cccc_runtime::stop(&fixture.group.group_id, "claude")
        .expect("user closes only the confirmation PTY");
    // Retry immediately, without waiting for the recovery watcher's next poll.
    for _ in 0..3 {
        assert_eq!(
            actor_delivery_worker::process_batch(
                &mut jobs,
                &mut String::new(),
                &AtomicBool::new(false)
            ),
            actor_delivery_worker::BatchOutcome::Retry
        );
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].event.id, id);
        assert_eq!(launches(&fixture), 1);
    }
}
