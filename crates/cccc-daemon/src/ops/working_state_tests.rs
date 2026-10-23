use super::working_state::fields;
use cccc_contracts::{
    Actor, ActorRuntime, DaemonRequest, RunnerKind, RuntimeMode, RuntimeStateSource,
};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::Map;

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
    assert_eq!(state["effective_working_state"], "idle");
    assert_eq!(state["effective_working_reason"], "headless_running");
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
