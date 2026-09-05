use super::block_on_managed;
use super::supervisor::{supports, uses_managed_session};
use cccc_contracts::{Actor, ActorRuntime, RunnerKind};

#[test]
fn admitted_runtimes_use_one_managed_session_and_always_expose_their_terminal() {
    let mut direct = Actor::new("codex-direct");
    direct.runtime = ActorRuntime::Codex;
    direct.runner = RunnerKind::Pty;
    direct.command = vec![
        "codex".into(),
        "-c".into(),
        "model_provider=\"ZAI\"".into(),
        "-m".into(),
        "glm-test".into(),
    ];
    assert!(supports(&direct));
    assert!(uses_managed_session(&direct));

    let mut legacy_runner = direct.clone();
    legacy_runner.id = "codex-legacy-runner".into();
    legacy_runner.runner = RunnerKind::Headless;
    assert!(supports(&legacy_runner));
    assert!(uses_managed_session(&legacy_runner));

    let mut claude = Actor::new("claude");
    claude.runtime = ActorRuntime::Claude;
    claude.runner = RunnerKind::Pty;
    claude.command = vec!["claude".into(), "--model".into(), "sonnet".into()];
    assert!(supports(&claude));
    assert!(uses_managed_session(&claude));
    claude.runner = RunnerKind::Headless;
    assert!(supports(&claude));
    assert!(uses_managed_session(&claude));

    let mut wrapped_claude = claude;
    wrapped_claude.runner = RunnerKind::Pty;
    wrapped_claude.command = vec!["sh".into(), "-lc".into(), "exec claude".into()];
    assert!(supports(&wrapped_claude));
    assert!(uses_managed_session(&wrapped_claude));

    let mut wrapped = direct.clone();
    wrapped.id = "codex-wrapper".into();
    wrapped.command = vec![
        "sh".into(),
        "-lc".into(),
        "exec codex --dangerously-bypass-approvals-and-sandbox".into(),
    ];
    assert!(supports(&wrapped));
    assert!(uses_managed_session(&wrapped));

    wrapped.runner = RunnerKind::Headless;
    assert!(supports(&wrapped));
    assert!(uses_managed_session(&wrapped));

    let mut unsupported_direct = direct;
    unsupported_direct.command = vec!["codex".into(), "exec".into()];
    assert!(supports(&unsupported_direct));
    assert!(uses_managed_session(&unsupported_direct));

    let mut grok = Actor::new("grok");
    grok.runtime = ActorRuntime::Grok;
    grok.runner = RunnerKind::Pty;
    grok.command = vec!["sh".into(), "-lc".into(), "exec grok".into()];
    assert!(supports(&grok));
    assert!(uses_managed_session(&grok));
    grok.runner = RunnerKind::Headless;
    assert!(supports(&grok));
    assert!(uses_managed_session(&grok));

    let mut opencode = Actor::new("opencode");
    opencode.runtime = ActorRuntime::Opencode;
    opencode.runner = RunnerKind::Pty;
    opencode.command = vec!["opencode".into(), "--auto".into()];
    assert!(supports(&opencode));
    assert!(uses_managed_session(&opencode));
    opencode.runner = RunnerKind::Headless;
    assert!(supports(&opencode));
    assert!(uses_managed_session(&opencode));

    let mut kilo = Actor::new("kilo");
    kilo.runtime = ActorRuntime::Kilo;
    kilo.command = vec!["kilo".into()];
    assert!(supports(&kilo));
    assert!(uses_managed_session(&kilo));
    assert!(super::provider_cli::uses_managed_provider_cli(&kilo));
}

#[tokio::test]
async fn managed_runtime_bridge_is_safe_inside_an_async_runtime() {
    assert_eq!(block_on_managed(async { 42 }), 42);
}
