use super::actor_runtime;
use cccc_contracts::{Actor, ActorRuntime, RunnerKind, RuntimeStateSource};

#[test]
fn runtime_source_follows_the_effective_managed_session_capability() {
    let mut actor = Actor::new("codex");
    actor.runtime = ActorRuntime::Codex;
    actor.runner = RunnerKind::Pty;
    actor_runtime::normalize_managed_session(&mut actor);
    assert_eq!(
        actor.runtime_state_source,
        RuntimeStateSource::ManagedSession
    );

    actor.runner = RunnerKind::Headless;
    actor_runtime::normalize_managed_session(&mut actor);
    assert_eq!(
        actor.runtime_state_source,
        RuntimeStateSource::ManagedSession
    );

    actor.runner = RunnerKind::Pty;
    actor.command = vec!["custom-codex-wrapper".into()];
    actor_runtime::normalize_managed_session(&mut actor);
    assert_eq!(actor.runtime_state_source, RuntimeStateSource::Terminal);

    let mut grok = Actor::new("grok");
    grok.runtime = ActorRuntime::Grok;
    grok.runner = RunnerKind::Pty;
    actor_runtime::normalize_managed_session(&mut grok);
    assert_eq!(
        grok.runtime_state_source,
        RuntimeStateSource::ManagedSession
    );

    let mut opencode = Actor::new("opencode");
    opencode.runtime = ActorRuntime::Opencode;
    opencode.runner = RunnerKind::Pty;
    actor_runtime::normalize_managed_session(&mut opencode);
    assert_eq!(
        opencode.runtime_state_source,
        RuntimeStateSource::ManagedSession
    );
}
