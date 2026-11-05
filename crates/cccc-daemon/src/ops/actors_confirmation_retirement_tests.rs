use super::*;
use crate::ops::{actor_delivery::DeliveryJob, actor_delivery_worker, local_headless};
use std::sync::atomic::AtomicBool;

fn automatic_start(fixture: &Fixture) {
    let result = actor_runtime::apply(&fixture.home, &fixture.stored(), "claude", "actor.start");
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn retired_actor_ids_and_explicit_new_actors_do_not_inherit_old_confirmation_pauses() {
    if isolated(
        "retirement_tests::retired_actor_ids_and_explicit_new_actors_do_not_inherit_old_confirmation_pauses",
    ) {
        return;
    }
    for cleanup in ["actor_remove", "actor_add"] {
        let fixture = Fixture::new();
        let original = fixture.group.actors[0].clone();
        let store = GroupStore::new(fixture.home.clone()).expect("store");
        if cleanup == "actor_remove" {
            local_headless::pause_confirmation(&fixture.group.group_id, "claude")
                .expect("old pause");
            let removed = fixture.request("actor_remove", Value::Null);
            assert!(removed.ok, "{:?}", removed.error);
            // Recreate through the store so this case proves successful removal cleaned up.
            store
                .mutate(&fixture.group.group_id, |group| {
                    actors::add(group, original.clone())
                })
                .expect("replacement Actor");
        } else {
            store
                .mutate(&fixture.group.group_id, |group| {
                    actors::remove(group, "claude")
                })
                .expect("remove old fixture Actor");
            local_headless::pause_confirmation(&fixture.group.group_id, "claude")
                .expect("stale pause");
            let mut actor = json!(original);
            actor
                .as_object_mut()
                .expect("Actor object")
                .remove("runner");
            let added = crate::handle_request(
                &fixture.home,
                &DaemonRequest {
                    v: 1,
                    op: "actor_add".into(),
                    args: json!({"group_id":fixture.group.group_id,"by":"user","actor":actor})
                        .as_object()
                        .expect("args")
                        .clone(),
                },
            );
            assert!(added.ok, "{:?}", added.error);
        }
        // Internal auto-start deliberately does not use the explicit retry hook.
        automatic_start(&fixture);
        assert!(actor_runtime::confirmation_required(
            &fixture.group.group_id,
            "claude"
        ));
    }
}

#[test]
fn a_saved_non_claude_runtime_is_not_blocked_by_an_old_claude_pause() {
    if isolated(
        "retirement_tests::a_saved_non_claude_runtime_is_not_blocked_by_an_old_claude_pause",
    ) {
        return;
    }
    let fixture = Fixture::new();
    local_headless::pause_confirmation(&fixture.group.group_id, "claude").expect("Claude pause");
    let program = fixture._temp.path().join("auggie");
    std::fs::write(
        &program,
        include_str!("codex_voice_analyst/native_acp/candidate_fixture.py"),
    )
    .expect("offline ACP provider");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("executable");
    let original = fixture.group.actors[0].clone();
    let mut environment = original.env.clone();
    environment.insert("ACP_FIXTURE_CLI".into(), "/bin/true".into());
    let changed = fixture.request(
        "actor_update",
        json!({
            "runtime":"auggie", "runtime_mode":"acp", "command":[program], "env":environment,
        }),
    );
    assert!(changed.ok, "{:?}", changed.error);
    automatic_start(&fixture);
    assert!(local_headless::running(&fixture.group.group_id, "claude"));
    actor_runtime::apply(&fixture.home, &fixture.stored(), "claude", "actor.stop")
        .expect("stop fake ACP");
    let restored = fixture.request("actor_update", json!({
        "runtime":"claude", "runtime_mode":original.runtime_mode, "command":original.command, "env":original.env,
    }));
    assert!(restored.ok, "{:?}", restored.error);
    automatic_start(&fixture);
    assert!(
        actor_runtime::confirmation_required(&fixture.group.group_id, "claude"),
        "old provider pause was retired, not just bypassed"
    );
}

#[test]
fn explicit_actor_and_group_stop_keep_queued_delivery_from_reopening_confirmation() {
    if isolated(
        "retirement_tests::explicit_actor_and_group_stop_keep_queued_delivery_from_reopening_confirmation",
    ) {
        return;
    }
    for operation in ["actor_stop", "group_stop"] {
        let fixture = Fixture::new();
        let started = fixture.request("actor_start", Value::Null);
        assert!(started.ok, "{:?}", started.error);
        let mut event = Event::new("chat.message", &fixture.group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["claude"],"text":"PENDING","message_mode":"send"})
            .as_object()
            .expect("data")
            .clone();
        let id = event.id.clone();
        let mut jobs = vec![DeliveryJob {
            home: fixture.home.clone(),
            group: fixture.group.clone(),
            actor: fixture.group.actors[0].clone(),
            event,
            claimed: true,
            transport: "managed_session",
        }];
        let stopped = fixture.request(operation, Value::Null);
        assert!(stopped.ok, "{:?}", stopped.error);
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
            assert!(jobs[0].claimed);
        }
        assert_eq!(
            std::fs::read_to_string(fixture.config.join("launches"))
                .expect("launch log")
                .lines()
                .count(),
            1
        );
    }
}

#[test]
fn pausing_group_retires_confirmation_but_keeps_other_terminal_running() {
    if isolated(
        "retirement_tests::pausing_group_retires_confirmation_but_keeps_other_terminal_running",
    ) {
        return;
    }
    let fixture = Fixture::new();
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    store
        .mutate(&fixture.group.group_id, |group| {
            let mut other = Actor::new("other");
            other.runtime = ActorRuntime::Custom;
            other.runner = cccc_contracts::RunnerKind::Pty;
            other.command = vec!["/bin/sleep".into(), "60".into()];
            group.actors.push(other);
            Ok(())
        })
        .expect("add ordinary member");
    cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: fixture.group.group_id.clone(),
        actor_id: "other".into(),
        runner: cccc_contracts::RunnerKind::Pty,
        command: vec!["/bin/sleep".into(), "60".into()],
        cwd: fixture._temp.path().to_owned(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    })
    .expect("start ordinary terminal");
    let started = fixture.request("actor_start", Value::Null);
    assert!(started.ok, "{:?}", started.error);
    assert!(actor_runtime::confirmation_required(
        &fixture.group.group_id,
        "claude"
    ));

    let set_state = |state: &str| {
        crate::handle_request(
            &fixture.home,
            &DaemonRequest {
                v: 1,
                op: "group_set_state".into(),
                args: json!({"group_id":fixture.group.group_id,"by":"user","state":state})
                    .as_object()
                    .expect("state arguments")
                    .clone(),
            },
        )
    };
    let paused = set_state("paused");
    assert!(paused.ok, "{:?}", paused.error);
    assert!(
        !cccc_runtime::status(&fixture.group.group_id, "claude").is_ok_and(|status| status.running)
    );
    assert!(
        cccc_runtime::status(&fixture.group.group_id, "other").is_ok_and(|status| status.running)
    );
    let resumed = set_state("active");
    assert!(resumed.ok, "{:?}", resumed.error);
    let retried = fixture.request("actor_start", Value::Null);
    assert!(retried.ok, "{:?}", retried.error);
    assert!(actor_runtime::confirmation_required(
        &fixture.group.group_id,
        "claude"
    ));
    assert_eq!(
        std::fs::read_to_string(fixture.config.join("launches"))
            .expect("launch log")
            .lines()
            .count(),
        2
    );
    cccc_runtime::stop(&fixture.group.group_id, "other").expect("stop ordinary fixture");
}
