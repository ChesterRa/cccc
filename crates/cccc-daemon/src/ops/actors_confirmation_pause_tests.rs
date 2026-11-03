use super::*;
use crate::ops::{actor_delivery::DeliveryJob, actor_delivery_worker};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn launches(fixture: &Fixture) -> usize {
    std::fs::read_to_string(fixture.config.join("launches"))
        .expect("fake provider launches")
        .lines()
        .count()
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
        std::fs::write(fixture.config.join("immediate_exit"), "").expect("decline prompt");
        let started = fixture.request("actor_start", Value::Null);
        assert!(started.ok, "{:?}", started.error);
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
        let before_ledger = std::fs::read(&ledger_path).expect("ledger before retries");
        let before_actor = json!(fixture.stored().actors[0]);
        let mut jobs = vec![DeliveryJob {
            home: fixture.home.clone(),
            group: fixture.group.clone(),
            actor: fixture.group.actors[0].clone(),
            event: event.clone(),
            claimed: true,
        }];
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
            assert_eq!(jobs[0].event.id, event.id);
            assert!(jobs[0].claimed);
        }
        assert_eq!(
            launches(&fixture),
            1,
            "automatic retries must not reopen a declined prompt"
        );
        assert_eq!(
            std::fs::read(&ledger_path).expect("ledger after retries"),
            before_ledger
        );
        assert_eq!(json!(fixture.stored().actors[0]), before_actor);

        if operation == "group_start" {
            assert_eq!(fixture.stored().state, cccc_contracts::GroupState::Active);
            store
                .mutate(&fixture.group.group_id, |group| {
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
            assert!(
                !ordinary.ok,
                "an ordinary active-group start must not clear confirmation pause"
            );
            assert!(
                ordinary
                    .error
                    .expect("paused start")
                    .message
                    .contains("confirmation")
            );
            assert_eq!(launches(&fixture), 1);
            let stopped = fixture.request("group_stop", Value::Null);
            assert!(stopped.ok, "{:?}", stopped.error);
        }
        std::fs::remove_file(fixture.config.join("immediate_exit"))
            .expect("keep the next prompt open");
        let retried = fixture.request(operation, json!({"enabled":true}));
        assert!(retried.ok, "{operation}: {:?}", retried.error);
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
