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

#[test]
fn managed_launch_preserves_errors_and_reports_task_failure() {
    let error = super::run_managed_launch(async {
        Err::<(), _>(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "launch rejected",
        ))
    })
    .expect_err("provider launch failure");
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "launch rejected");

    let error = super::run_managed_launch::<_, ()>(async { panic!("fixture launch panic") })
        .expect_err("launch task failure");
    assert!(
        error
            .to_string()
            .contains("managed Agent launch task failed")
    );
    assert_eq!(
        super::run_managed_launch(async { Ok(42) }).expect("next launch"),
        42
    );
}

#[cfg(target_os = "linux")]
mod launch_lifetime {
    use super::super::run_managed_launch;
    use cccc_runtime::OwnedProcessTree;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    const FIXTURE_ENV: &str = "CCCC_TEST_MANAGED_PARENT_DEATH";
    const FIXTURE_TEST: &str =
        "ops::local_headless::supervisor_managed_tests::launch_lifetime::parent_bound_child";

    #[test]
    fn parent_bound_child() {
        if std::env::var_os(FIXTURE_ENV).is_none() {
            return;
        }
        nix::sys::prctl::set_pdeathsig(nix::sys::signal::Signal::SIGTERM)
            .expect("bind fixture to its spawning thread, like Grok ACP");
        println!("\nCCCC_CHILD_READY");
        for line in std::io::stdin().lock().lines() {
            match line.expect("fixture input").as_str() {
                "ping" => println!("CCCC_CHILD_PONG"),
                "stop" => return,
                other => panic!("unexpected fixture input: {other}"),
            }
        }
    }

    struct Probe {
        child: Child,
        tree: OwnedProcessTree,
        lines: Receiver<String>,
    }

    impl Probe {
        fn spawn() -> std::io::Result<Self> {
            let (mut child, tree) = OwnedProcessTree::spawn(
                Command::new(std::env::current_exe()?)
                    .args(["--exact", FIXTURE_TEST, "--nocapture"])
                    .env(FIXTURE_ENV, "1")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped()),
            )?;
            let stdout = child.stdout.take().expect("fixture stdout");
            let (sender, lines) = mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
            let probe = Self { child, tree, lines };
            probe.expect_line("CCCC_CHILD_READY");
            Ok(probe)
        }

        fn expect_line(&self, expected: &str) {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let line = self
                    .lines
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .expect("managed child must remain connected after the launch caller exits");
                if line == expected {
                    return;
                }
            }
        }

        fn send(&mut self, line: &str) {
            writeln!(self.child.stdin.as_mut().expect("fixture stdin"), "{line}")
                .expect("managed child must accept input after the launch caller exits");
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = self.tree.terminate();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn managed_process_survives_its_launch_caller_and_still_stops() {
        for inside_async_runtime in [false, true] {
            let mut probe = std::thread::spawn(move || {
                let launch = || {
                    run_managed_launch(async {
                        // Spawning after an await must also use a daemon-lifetime thread.
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        Probe::spawn()
                    })
                    .expect("launch managed child")
                };
                if inside_async_runtime {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("caller runtime")
                        .block_on(async { launch() })
                } else {
                    launch()
                }
            })
            .join()
            .expect("launch caller exits");
            probe.send("ping");
            probe.expect_line("CCCC_CHILD_PONG");
            probe.send("stop");
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(status) = probe
                    .tree
                    .try_wait(|| probe.child.try_wait())
                    .expect("poll stopped child")
                {
                    assert!(status.success(), "explicit stop must complete normally");
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "managed child failed to stop"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
