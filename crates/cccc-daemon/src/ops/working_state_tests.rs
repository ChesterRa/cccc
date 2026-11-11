use super::working_state::fields;
use cccc_contracts::{
    Actor, ActorRuntime, DaemonRequest, RunnerKind, RuntimeMode, RuntimeStateSource,
};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

#[test]
fn stopped_actor_listing_uses_the_configured_surface_without_starting_work() {
    let temp = tempfile::tempdir().expect("fixture directory");
    let home = HomeLayout::from_path(temp.path()).expect("fixture home");
    home.initialize().expect("initialize fixture home");
    let store = GroupStore::new(home.clone()).expect("fixture store");
    let mut group = store.create("Stopped surfaces", "").expect("fixture group");
    let cases = [
        (ActorRuntime::Pi, RuntimeMode::Default, "headless"),
        (ActorRuntime::Antigravity, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Auggie, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Copilot, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Cursor, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Devin, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Kimi, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Kiro, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Hermes, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Droid, RuntimeMode::Acp, "headless"),
        (ActorRuntime::Kiro, RuntimeMode::Default, "pty"),
        (ActorRuntime::Hermes, RuntimeMode::Default, "pty"),
        (ActorRuntime::Droid, RuntimeMode::Default, "pty"),
        (ActorRuntime::Kimi, RuntimeMode::Default, "pty"),
        (ActorRuntime::Auggie, RuntimeMode::Default, "pty"),
        (ActorRuntime::Claude, RuntimeMode::Default, "pty"),
        (ActorRuntime::Codex, RuntimeMode::Default, "pty"),
        (ActorRuntime::Deepseek, RuntimeMode::Default, "headless"),
        (ActorRuntime::WebModel, RuntimeMode::Default, "headless"),
    ];
    for (index, (runtime, mode, _)) in cases.iter().enumerate() {
        let mut actor = Actor::new(format!("stopped-{index}"));
        actor.runtime = *runtime;
        actor.runtime_mode = *mode;
        group.actors.push(actor);
    }
    store.save(&group).expect("save stopped actors");
    let request = DaemonRequest {
        v: 1,
        op: "actor_list".into(),
        args: Map::new(),
    };
    let values = super::actor_listing::list(&home, &group, &request)
        .expect("list configured stopped surfaces");
    assert_eq!(values.len(), cases.len());
    for (value, (_, _, runner)) in values.iter().zip(cases) {
        assert_eq!(value["runner"], runner, "{}", value["id"]);
        assert_eq!(value["runner_effective"], runner, "{}", value["id"]);
        assert_eq!(value["running"], false);
        assert_eq!(value["pid"], serde_json::Value::Null);
        assert_eq!(value["effective_working_state"], "stopped");
        assert!(value["pty_last_output_at"].is_null());
        assert!(value["pty_quiet_seconds"].is_null());
        assert!(
            super::local_headless::structured_state(
                &group.group_id,
                value["id"].as_str().expect("actor ID")
            )
            .is_none()
        );
    }
}

#[test]
fn claude_state_comes_only_from_its_managed_session() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group_id = "g_claude_projection";
    let mut actor = Actor::new("peer1");
    actor.runtime = ActorRuntime::Claude;
    actor.runtime_state_source = RuntimeStateSource::ManagedSession;

    let state = fields(&home, &actor, group_id, true, "pty");
    assert_eq!(state["effective_working_state"], "waiting");
    assert_eq!(
        state["effective_working_reason"],
        "managed_agent_session_pending"
    );
}

#[test]
fn pending_managed_session_and_structured_runtime_have_distinct_states() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let mut claude = Actor::new("peer1");
    claude.runtime = ActorRuntime::Claude;
    let state = fields(&home, &claude, "g_test", true, "pty");
    assert_eq!(state["effective_working_state"], "waiting");
    assert_eq!(
        state["effective_working_reason"],
        "managed_agent_session_pending"
    );

    let mut custom = Actor::new("peer1");
    custom.runtime = ActorRuntime::Custom;
    custom.runner = RunnerKind::Headless;
    let state = fields(&home, &custom, "g_test", true, "headless");
    // No turn record exists: the state cannot be known, never bare idle.
    assert_eq!(state["effective_working_state"], "waiting");
    assert_eq!(state["effective_working_reason"], "headless_state_unknown");
    assert!(state["pty_last_output_at"].is_null());
    assert!(state["pty_quiet_seconds"].is_null());
}

/// Terminal output is an observation for display. It never decides the working
/// state, because output does not establish whether a process is working.
#[cfg(unix)]
mod terminal_output {
    use super::*;
    use serde_json::Value;
    use std::path::Path;
    use std::time::{Duration, Instant};

    const ACTOR_ID: &str = "peer1";
    /// Prints once, then computes without writing anything.
    const QUIET_WORK: &str = "printf 'computing\\n'; while :; do :; done";
    /// A prompt that keeps redrawing while it waits for an answer.
    const NOISY_PROMPT: &str = "while :; do printf 'Continue? [y/N] \\r'; sleep 0.2; done";
    const PRINT_THEN_SLEEP: &str = "printf 'done\\n'; exec sleep 30";

    struct StopOnDrop(String);

    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            let _ = cccc_runtime::stop(&self.0, ACTOR_ID);
        }
    }

    fn start_terminal(group_id: &str, cwd: &Path, script: &str) -> StopOnDrop {
        cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group_id.into(),
            actor_id: ACTOR_ID.into(),
            runner: RunnerKind::Pty,
            command: vec!["sh".into(), "-c".into(), script.into()],
            cwd: cwd.into(),
            env: Default::default(),
            cols: 80,
            rows: 24,
        })
        .expect("start terminal process");
        StopOnDrop(group_id.into())
    }

    fn custom_actor() -> Actor {
        let mut actor = Actor::new(ACTOR_ID);
        actor.runtime = ActorRuntime::Custom;
        actor
    }

    fn still_running(group_id: &str) -> bool {
        cccc_runtime::status(group_id, ACTOR_ID).is_ok_and(|status| status.running)
    }

    fn quiet_seconds(state: &Map<String, Value>) -> u64 {
        state["pty_quiet_seconds"].as_u64().unwrap_or(0)
    }

    /// Polls until the daemon has seen terminal output, or until the working
    /// state leaves "unknown" so that the caller's assertion reports it.
    fn observe_output(home: &HomeLayout, actor: &Actor, group_id: &str) -> Map<String, Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = fields(home, actor, group_id, true, "pty");
            let last_output = state.get("pty_last_output_at");
            if last_output.is_some_and(Value::is_string)
                || state["effective_working_reason"] != "pty_running_state_unknown"
                || Instant::now() >= deadline
            {
                return state;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn assert_unknown_state(state: &Map<String, Value>) {
        assert_eq!(state["effective_working_state"], "waiting", "{state:?}");
        assert_eq!(
            state["effective_working_reason"], "pty_running_state_unknown",
            "{state:?}"
        );
        assert!(state["effective_working_updated_at"].is_null());
        assert!(state["idle_seconds"].is_null());
    }

    #[test]
    fn actor_list_observations_follow_only_the_current_terminal_session() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Output observations", "").expect("group");
        group.actors.push(custom_actor());
        store.save(&group).expect("save actor");
        let request = DaemonRequest {
            v: 1,
            op: "actor_list".into(),
            args: serde_json::json!({"group_id":group.group_id,"by":"user"})
                .as_object()
                .expect("args")
                .clone(),
        };
        let list = || {
            let response = crate::handle_request(&home, &request);
            assert!(response.ok, "{:?}", response.error);
            response.result["actors"][0].clone()
        };
        let stopped = list();
        assert_eq!(stopped["running"], false);
        assert!(stopped["pty_last_output_at"].is_null());
        assert!(stopped["pty_quiet_seconds"].is_null());
        assert!(!still_running(&group.group_id));

        let terminal = start_terminal(&group.group_id, temp.path(), PRINT_THEN_SLEEP);
        let deadline = Instant::now() + Duration::from_secs(10);
        let observed = loop {
            let value = list();
            if value["pty_last_output_at"].is_string() || Instant::now() >= deadline {
                break value;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        chrono::DateTime::parse_from_rfc3339(
            observed["pty_last_output_at"]
                .as_str()
                .expect("last output"),
        )
        .expect("RFC 3339 timestamp");
        assert!(observed["pty_quiet_seconds"].is_u64());
        assert_unknown_state(observed.as_object().expect("actor"));

        drop(terminal);
        let stopped = list();
        assert_eq!(stopped["running"], false);
        assert!(stopped["pty_last_output_at"].is_null());
        assert!(stopped["pty_quiet_seconds"].is_null());

        let _silent = start_terminal(&group.group_id, temp.path(), "exec sleep 30");
        let fresh = list();
        assert_eq!(fresh["running"], true);
        assert!(fresh["pty_last_output_at"].is_null());
        assert!(fresh["pty_quiet_seconds"].is_null());
        assert_unknown_state(fresh.as_object().expect("actor"));
    }

    #[test]
    fn quiet_ongoing_work_keeps_the_working_state_unknown() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let group_id = "g_pty_quiet_work";
        let _stop = start_terminal(group_id, temp.path(), QUIET_WORK);
        let actor = custom_actor();

        let observed = observe_output(&home, &actor, group_id);
        assert_unknown_state(&observed);
        assert!(observed["pty_last_output_at"].is_string(), "{observed:?}");

        // The process keeps computing but writes nothing more.
        let deadline = Instant::now() + Duration::from_secs(10);
        let quiet = loop {
            let state = fields(&home, &actor, group_id, true, "pty");
            assert_unknown_state(&state);
            if quiet_seconds(&state) >= 1 || Instant::now() >= deadline {
                break state;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(quiet_seconds(&quiet) >= 1, "{quiet:?}");
        assert!(still_running(group_id));
    }

    #[test]
    fn noisy_input_waiting_keeps_the_working_state_unknown() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let group_id = "g_pty_noisy_prompt";
        let _stop = start_terminal(group_id, temp.path(), NOISY_PROMPT);
        let actor = custom_actor();

        let observed = observe_output(&home, &actor, group_id);
        assert_unknown_state(&observed);
        let first_output = observed["pty_last_output_at"].clone();
        assert!(first_output.is_string(), "{observed:?}");

        // The prompt keeps printing while it waits; the state stays unknown.
        let deadline = Instant::now() + Duration::from_secs(10);
        let later = loop {
            let state = fields(&home, &actor, group_id, true, "pty");
            assert_unknown_state(&state);
            if state["pty_last_output_at"] != first_output || Instant::now() >= deadline {
                break state;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_ne!(later["pty_last_output_at"], first_output, "{later:?}");
        assert!(still_running(group_id));
    }

    #[test]
    fn print_then_sleep_reports_observations_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let group_id = "g_pty_print_then_sleep";
        let _stop = start_terminal(group_id, temp.path(), PRINT_THEN_SLEEP);
        let actor = custom_actor();

        let observed = observe_output(&home, &actor, group_id);
        assert_unknown_state(&observed);
        let last_output_at = observed["pty_last_output_at"].clone();
        assert!(last_output_at.is_string(), "{observed:?}");
        assert!(observed["pty_quiet_seconds"].is_u64(), "{observed:?}");

        let deadline = Instant::now() + Duration::from_secs(10);
        let quiet = loop {
            let state = fields(&home, &actor, group_id, true, "pty");
            if quiet_seconds(&state) >= 1 || Instant::now() >= deadline {
                break state;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_unknown_state(&quiet);
        assert!(quiet_seconds(&quiet) >= 1, "{quiet:?}");
        assert_eq!(quiet["pty_last_output_at"], last_output_at);
        assert!(still_running(group_id));
    }

    #[test]
    fn a_new_terminal_session_starts_without_observations() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let group_id = "g_pty_session_reset";
        let actor = custom_actor();
        let first = start_terminal(group_id, temp.path(), PRINT_THEN_SLEEP);
        let observed = observe_output(&home, &actor, group_id);
        assert_unknown_state(&observed);
        assert!(observed["pty_last_output_at"].is_string(), "{observed:?}");
        drop(first);

        let _stop = start_terminal(group_id, temp.path(), "exec sleep 30");
        let fresh = fields(&home, &actor, group_id, true, "pty");
        assert_unknown_state(&fresh);
        assert!(fresh["pty_last_output_at"].is_null(), "{fresh:?}");
        assert!(fresh["pty_quiet_seconds"].is_null(), "{fresh:?}");
    }

    #[test]
    fn a_managed_session_keeps_precedence_over_terminal_output() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let group_id = "g_pty_managed_session";
        let mut actor = Actor::new(ACTOR_ID);
        actor.runtime = ActorRuntime::Claude;
        actor.runtime_state_source = RuntimeStateSource::ManagedSession;
        let _stop = start_terminal(group_id, temp.path(), PRINT_THEN_SLEEP);

        let deadline = Instant::now() + Duration::from_secs(10);
        let state = loop {
            let state = fields(&home, &actor, group_id, true, "pty");
            let last_output = state.get("pty_last_output_at");
            if last_output.is_some_and(Value::is_string) || Instant::now() >= deadline {
                break state;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(state["effective_working_state"], "waiting", "{state:?}");
        assert_eq!(
            state["effective_working_reason"],
            "managed_agent_session_pending"
        );
        assert!(state["pty_last_output_at"].is_string(), "{state:?}");
    }
}

fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_default(),
    }
}

fn headless_group(store: &GroupStore, actor: &Actor) -> cccc_core::GroupDoc {
    let mut group = store.create("Headless record", "").expect("group");
    group.running = true;
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    group
}

#[test]
fn headless_actor_reports_working_while_its_turn_runs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Headless turn", "").expect("group");
    let mut actor = Actor::new("hs-actor");
    actor.runtime = ActorRuntime::Deepseek;
    // The runner's own record: a live turn, its task, and the last write time.
    cccc_core::integration_state::group_update(
        &store,
        &group.group_id,
        "runtime_states",
        |value| {
            *value = json!({
                "hs-actor": {
                    "status": "working",
                    "active_turn_id": "turn-1",
                    "task_id": "task-7",
                    "updated_at": "2026-11-09T12:00:00Z"
                }
            });
            Ok(())
        },
    )
    .expect("seed turn record");
    let state = fields(&home, &actor, &group.group_id, true, "headless");
    assert_eq!(state["effective_working_state"], "working");
    assert_eq!(state["effective_working_reason"], "headless_turn_record");
    assert_eq!(
        state["effective_working_updated_at"],
        "2026-11-09T12:00:00Z"
    );
    assert_eq!(state["effective_active_task_id"], "task-7");
}

#[test]
fn headless_actor_reports_idle_with_the_last_turn_end() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Headless idle", "").expect("group");
    let mut actor = Actor::new("hs-actor");
    actor.runtime = ActorRuntime::Deepseek;
    cccc_core::integration_state::group_update(
        &store,
        &group.group_id,
        "runtime_states",
        |value| {
            *value = json!({
                "hs-actor": {
                    "status": "idle",
                    "last_turn_end": "2026-11-09T12:30:00Z",
                    "updated_at": "2026-11-09T12:31:00Z"
                }
            });
            Ok(())
        },
    )
    .expect("seed idle record");
    let state = fields(&home, &actor, &group.group_id, true, "headless");
    assert_eq!(state["effective_working_state"], "idle");
    assert_eq!(state["effective_working_reason"], "headless_turn_record");
    assert_eq!(
        state["effective_working_updated_at"],
        "2026-11-09T12:30:00Z"
    );
    assert!(state["effective_active_task_id"].is_null());
}

#[test]
fn headless_actor_with_no_record_is_not_reported_idle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Headless unknown", "").expect("group");
    let mut actor = Actor::new("hs-actor");
    actor.runtime = ActorRuntime::Deepseek;
    // No runtime_states record: no evidence for idle or for a last turn end.
    let state = fields(&home, &actor, &group.group_id, true, "headless");
    assert_eq!(state["effective_working_state"], "waiting");
    assert_eq!(state["effective_working_reason"], "headless_state_unknown");
    assert!(state["effective_working_updated_at"].is_null());
    assert!(state["effective_active_task_id"].is_null());
}

#[test]
fn an_unreadable_headless_record_is_unknown_not_idle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("not-a-home");
    std::fs::write(&path, b"not a directory").expect("fixture file");
    let home = HomeLayout::from_path(&path).expect("home path");
    let mut actor = Actor::new("hs-actor");
    actor.runtime = ActorRuntime::Deepseek;
    let state = fields(&home, &actor, "g_headless_unreadable", true, "headless");
    assert_eq!(state["effective_working_state"], "waiting");
    assert_eq!(state["effective_working_reason"], "headless_state_unknown");
    assert!(state["effective_working_updated_at"].is_null());
    assert!(state["effective_active_task_id"].is_null());
}

#[test]
fn headless_record_status_wins_over_a_stale_turn_id() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut actor = Actor::new("hs-actor");
    actor.runtime = ActorRuntime::Deepseek;
    actor.runner = RunnerKind::Headless;
    let group = headless_group(&store, &actor);
    cccc_core::integration_state::group_update(
        &store,
        &group.group_id,
        "runtime_states",
        |value| {
            *value = json!({
                "hs-actor": {
                    "active_turn_id": "turn-stale",
                    "task_id": "task-stale",
                    "updated_at": "2026-11-09T12:00:00Z"
                }
            });
            Ok(())
        },
    )
    .expect("seed stale turn");
    for (status, expected) in [
        ("idle", "idle"),
        ("waiting", "waiting"),
        ("stopped", "stopped"),
    ] {
        let response = crate::dispatch::dispatch(
            &home,
            &request(
                "headless_set_status",
                json!({
                    "group_id": group.group_id,
                    "actor_id": actor.id,
                    "by": actor.id,
                    "status": status,
                    "task_id": "task-stale"
                }),
            ),
        );
        assert!(response.ok, "{:?}", response.error);
        let state = fields(&home, &actor, &group.group_id, true, "headless");
        assert_eq!(state["effective_working_state"], expected, "{status}");
        assert!(state["effective_active_task_id"].is_null(), "{status}");
    }
}

#[test]
fn headless_turn_end_is_not_rewritten_by_an_empty_poll() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut actor = Actor::new("hs-actor");
    actor.runtime = ActorRuntime::Deepseek;
    actor.runner = RunnerKind::Headless;
    let group = headless_group(&store, &actor);
    cccc_core::integration_state::group_update(
        &store,
        &group.group_id,
        "runtime_states",
        |value| {
            *value = json!({
                "hs-actor": {
                    "status": "working",
                    "active_turn_id": "turn-1",
                    "task_id": "task-7",
                    "updated_at": "2026-11-09T12:00:00Z"
                }
            });
            Ok(())
        },
    )
    .expect("seed turn record");
    let completion = super::runtime_completion::Completion {
        turn_id: "turn-1".into(),
        event_ids: vec!["event-1".into()],
        status: "done".into(),
        delivery_id: "delivery-1".into(),
    };
    let receipt = super::runtime_completion::append(&home, &group.group_id, &actor.id, &completion)
        .expect("append completion");
    let ended_at = receipt.ts.clone();
    super::runtime_state::finish_completion(
        &home,
        &group,
        &actor.id,
        &completion,
        receipt,
        &request("runtime_complete_turn", json!({})),
    )
    .expect("finish turn");
    let completed = fields(&home, &actor, &group.group_id, true, "headless");
    // Completion leaves the runner waiting for its next turn, with the turn end
    // as the timestamp clients read.
    assert_eq!(completed["effective_working_state"], "waiting");
    assert_eq!(completed["effective_working_updated_at"], ended_at);
    assert!(completed["effective_active_task_id"].is_null());

    let polled = super::runtime_state::wait_next_turn(
        &home,
        &request(
            "runtime_wait_next_turn",
            json!({
                "group_id": group.group_id,
                "actor_id": actor.id,
                "by": actor.id
            }),
        ),
    )
    .expect("empty poll");
    assert_eq!(polled["status"], "idle");
    let after_poll = fields(&home, &actor, &group.group_id, true, "headless");
    assert_eq!(after_poll["effective_working_state"], "waiting");
    assert_eq!(after_poll["effective_working_updated_at"], ended_at);
    assert!(after_poll["effective_active_task_id"].is_null());
}

#[test]
fn the_headless_snapshot_never_waits_on_the_group_lock_a_writer_holds() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Headless lock", "").expect("group");
    let mut actor = Actor::new("hs-actor");
    actor.runtime = ActorRuntime::Deepseek;
    cccc_core::integration_state::group_update(
        &store,
        &group.group_id,
        "runtime_states",
        |value| {
            *value = json!({
                "hs-actor": {
                    "status": "working",
                    "active_turn_id": "turn-9",
                    "updated_at": "2026-11-09T12:00:00Z"
                }
            });
            Ok(())
        },
    )
    .expect("seed turn record");
    // A runner mutating its record holds the group lock for the whole write;
    // hold it here so the snapshot proves it does not wait on that lock.
    let lock_path = store
        .group_dir(&group.group_id)
        .expect("group dir")
        .join("group.yaml.lock");
    std::fs::create_dir_all(lock_path.parent().expect("lock parent")).expect("lock dir");
    let lock_file = std::fs::File::create(&lock_path).expect("lock file");
    fs2::FileExt::lock_exclusive(&lock_file).expect("exclusive lock");
    let state = fields(&home, &actor, &group.group_id, true, "headless");
    fs2::FileExt::unlock(&lock_file).expect("unlock");
    assert_eq!(state["effective_working_state"], "working");
}
