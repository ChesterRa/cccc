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
}
