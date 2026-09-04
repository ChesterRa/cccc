use super::working_state::fields;
use cccc_contracts::{Actor, ActorRuntime, RunnerKind, RuntimeStateSource};
use cccc_core::HomeLayout;

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
