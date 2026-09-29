use super::working_state::fields;
use cccc_contracts::{Actor, ActorRuntime, RunnerKind, RuntimeStateSource};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};

fn test_group(home: &HomeLayout, extra: serde_json::Value) -> GroupDoc {
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("working state tests", "").expect("group");
    group.extra = extra.as_object().cloned().unwrap_or_default();
    group
}

#[test]
fn claude_state_comes_only_from_its_managed_session() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = test_group(&home, serde_json::json!({}));
    let mut actor = Actor::new("peer1");
    actor.runtime = ActorRuntime::Claude;
    actor.runtime_state_source = RuntimeStateSource::ManagedSession;

    let state = fields(&home, &actor, &group, true, "pty");
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
    let group = test_group(&home, serde_json::json!({}));
    let mut claude = Actor::new("peer1");
    claude.runtime = ActorRuntime::Claude;
    let state = fields(&home, &claude, &group, true, "pty");
    assert_eq!(state["effective_working_state"], "waiting");
    assert_eq!(
        state["effective_working_reason"],
        "managed_agent_session_pending"
    );

    let mut custom = Actor::new("peer1");
    custom.runtime = ActorRuntime::Custom;
    custom.runner = RunnerKind::Headless;
    let state = fields(&home, &custom, &group, true, "headless");
    assert_eq!(state["effective_working_state"], "idle");
    assert_eq!(state["effective_working_reason"], "headless_running");
}

#[test]
#[cfg(target_os = "linux")]
fn interactive_prompt_reports_blocked_on_dialog() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = test_group(&home, serde_json::json!({}));
    let mut actor = Actor::new("peer1");
    actor.runtime = ActorRuntime::Custom;
    cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        runner: RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "printf 'Do you trust the files in this folder? [y/N]\\n'; touch ready; sleep 30"
                .into(),
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
    std::thread::sleep(std::time::Duration::from_millis(50));

    let state = fields(&home, &actor, &group, true, "pty");
    cccc_runtime::stop(&group.group_id, "peer1").expect("cleanup");
    assert_eq!(state["effective_working_state"], "blocked_on_dialog");
    assert_eq!(
        state["effective_working_reason"],
        "interactive_prompt:folder_trust"
    );
    assert_eq!(state["blocked_dialog_prompt"], "folder_trust");
}

#[test]
#[cfg(target_os = "linux")]
fn dialog_allowlist_suppresses_preapproved_prompts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = test_group(
        &home,
        serde_json::json!({"dialog_allowlist": ["folder_trust"]}),
    );
    let mut actor = Actor::new("peer1");
    actor.runtime = ActorRuntime::Custom;
    cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        runner: RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "printf 'Do you trust the files in this folder? [y/N]\\n'; touch ready; sleep 30"
                .into(),
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
    std::thread::sleep(std::time::Duration::from_millis(50));

    let state = fields(&home, &actor, &group, true, "pty");
    cccc_runtime::stop(&group.group_id, "peer1").expect("cleanup");
    assert_eq!(state["effective_working_state"], "waiting");
    assert_eq!(
        state["effective_working_reason"],
        "pty_running_state_unknown"
    );
    assert_eq!(state["blocked_dialog_prompt"], serde_json::Value::Null);
}
