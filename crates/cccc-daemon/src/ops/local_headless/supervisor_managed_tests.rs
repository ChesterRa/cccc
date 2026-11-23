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

#[test]
fn precise_headless_runtimes_share_the_managed_owner_without_an_optional_pi_mode() {
    use cccc_contracts::RuntimeMode;
    let mut pi = Actor::new("pi");
    pi.runtime = ActorRuntime::Pi;
    pi.normalize_runtime_constraints();
    assert!(supports(&pi));
    assert!(super::provider_cli::uses_managed_provider_cli(&pi));
    assert_eq!(pi.runner, RunnerKind::Headless);
    for runtime in [
        ActorRuntime::Auggie,
        ActorRuntime::Kimi,
        ActorRuntime::Kiro,
        ActorRuntime::Droid,
    ] {
        let mut actor = Actor::new("native");
        actor.runtime = runtime;
        assert!(!supports(&actor));
        actor.runtime_mode = RuntimeMode::Acp;
        assert!(supports(&actor));
    }
    // Hermes's terminal views the member's ACP session, so both modes are
    // managed sessions.
    let mut hermes = Actor::new("hermes");
    hermes.runtime = ActorRuntime::Hermes;
    assert!(supports(&hermes));
    hermes.runtime_mode = RuntimeMode::Acp;
    assert!(supports(&hermes));
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

#[cfg(unix)]
mod structured_admission {
    use crate::ops::{actor_delivery, local_headless, runtime_delivery};
    use cccc_contracts::{Actor, ActorRuntime, Event, RuntimeMode};
    use cccc_core::{GroupDoc, GroupStore, HomeLayout};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    const SECRET: &str = "synthetic-admission-credential";

    struct Fixture {
        temp: tempfile::TempDir,
        home: HomeLayout,
        group: GroupDoc,
    }

    impl Fixture {
        fn new() -> Self {
            Self::with_runtime(ActorRuntime::Kiro)
        }

        fn with_runtime(runtime: ActorRuntime) -> Self {
            Self::with_markers(runtime, &[])
        }

        /// Marker files in the workspace switch on fixture behaviours that
        /// must be in place before the provider starts.
        fn with_markers(runtime: ActorRuntime, markers: &[&str]) -> Self {
            let mode = if runtime == ActorRuntime::Opencode {
                RuntimeMode::Default
            } else {
                RuntimeMode::Acp
            };
            Self::build(runtime, mode, markers)
        }

        fn build(runtime: ActorRuntime, mode: RuntimeMode, markers: &[&str]) -> Self {
            let (fixture, actor) = Self::unstarted(runtime, mode, markers);
            local_headless::start(&fixture.home, &fixture.group, &actor)
                .expect("start owned fixture");
            fixture
        }

        fn unstarted(runtime: ActorRuntime, mode: RuntimeMode, markers: &[&str]) -> (Self, Actor) {
            let temp = tempfile::tempdir().expect("isolated admission fixture");
            for marker in markers {
                std::fs::write(temp.path().join(marker), "").expect("fixture marker");
            }
            let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
            home.initialize().expect("initialize home");
            std::fs::write(temp.path().join("AGENTS.md"), "initial instructions")
                .expect("initial instruction file");
            let store = GroupStore::new(home.clone()).expect("store");
            let group = store.create("ACP admission diagnostic", "").expect("group");
            let mut group = cccc_core::group_scope::attach(
                &store,
                &group.group_id,
                cccc_core::scope::detect(temp.path()).expect("fixture scope"),
            )
            .expect("attach scope");
            let opencode = runtime == ActorRuntime::Opencode;
            let executable = temp.path().join(match runtime {
                ActorRuntime::Opencode => "opencode",
                ActorRuntime::Hermes => "hermes",
                _ => "kiro-cli",
            });
            std::fs::write(
                &executable,
                if opencode {
                    include_str!("../codex_voice_analyst/tests/opencode_v2_fixture.py")
                } else {
                    include_str!("../codex_voice_analyst/native_acp/v3_fixture.py")
                },
            )
            .expect("write ACP fixture");
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                .expect("fixture permissions");
            let mut actor = Actor::new("worker");
            actor.runtime = runtime;
            actor.runtime_mode = mode;
            actor.command = vec![executable.to_string_lossy().into_owned()];
            actor.env = BTreeMap::from([
                ("AAAA_OVERLAPPING_SECRET".into(), "synthetic".into()),
                (
                    "HOME".into(),
                    temp.path().join("provider").to_string_lossy().into_owned(),
                ),
                (
                    "CODEX_HOME".into(),
                    temp.path().join("codex").to_string_lossy().into_owned(),
                ),
                ("ACP_FIXTURE_CLI".into(), "/bin/true".into()),
                ("CCCC_TEST_ADMISSION_SECRET".into(), SECRET.into()),
                (
                    "OPENCODE_FIXTURE_DIR".into(),
                    temp.path()
                        .join("opencode-events")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ]);
            group.actors.push(actor.clone());
            store.save(&group).expect("save actor");
            (Self { temp, home, group }, actor)
        }

        fn requests(&self) -> Vec<Value> {
            std::fs::read_to_string(self.temp.path().join("v3_requests.jsonl"))
                .expect("fixture requests")
                .lines()
                .map(|line| serde_json::from_str(line).expect("request"))
                .collect()
        }

        /// The arguments of each setup the member's terminal ran.
        fn setup_runs(&self) -> Vec<Value> {
            std::fs::read_to_string(self.temp.path().join("v3_setup_runs.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).expect("setup run"))
                .collect()
        }

        fn send(&self, text: &str) -> Event {
            let mut event = Event::new("chat.message", &self.group.group_id);
            event.by = "user".into();
            event.data = json!({"text":text,"to":["worker"],"message_mode":"send"})
                .as_object()
                .expect("message fields")
                .clone();
            let store = GroupStore::new(self.home.clone()).expect("store");
            cccc_core::ledger::append(
                &store.ledger_path(&self.group.group_id).expect("ledger"),
                &event,
            )
            .expect("source event");
            assert_eq!(
                actor_delivery::dispatch(&self.home, &self.group, &event).queued,
                1
            );
            event
        }

        fn wait(&self, ready: impl Fn() -> bool) {
            let deadline = Instant::now() + Duration::from_secs(4);
            while !ready() {
                assert!(Instant::now() < deadline, "fixture event deadline");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn journal(&self) -> Vec<Value> {
            let path = GroupStore::new(self.home.clone())
                .expect("store")
                .state_dir(&self.group.group_id)
                .expect("state")
                .join("headless/events.jsonl");
            std::fs::read_to_string(path)
                .expect("headless journal")
                .lines()
                .map(|line| serde_json::from_str(line).expect("journal event"))
                .collect()
        }

        fn outcome(&self, event: &Event) -> Option<String> {
            runtime_delivery::latest_state(&self.home, &self.group.group_id, "worker", &event.id)
                .expect("delivery state")
                .map(|(state, _)| state)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            actor_delivery::shutdown_actor(&self.group.group_id, "worker");
            let _ = local_headless::stop(&self.group.group_id, "worker");
        }
    }

    #[test]
    fn prepared_managed_input_stays_bound_to_its_validated_generation() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let fixture = Fixture::new();
        let actor = &fixture.group.actors[0];
        let generation = || {
            local_headless::registered_generation(&fixture.group.group_id, &actor.id)
                .expect("registered managed holder")
                .0
        };
        let old = generation();
        local_headless::stop(&fixture.group.group_id, &actor.id).expect("stop old holder");
        local_headless::start(&fixture.home, &fixture.group, actor).expect("start replacement");
        let current = generation();
        assert_ne!(current, old);
        let cancelled = AtomicBool::new(false);
        let started = AtomicBool::new(false);
        let event = Event::new("chat.message", &fixture.group.group_id);
        let submit = |generation| {
            local_headless::submit_prepared(
                &fixture.home,
                &fixture.group,
                actor,
                generation,
                std::slice::from_ref(&event),
                "GENERATION_BOUND_INPUT",
                &cancelled,
                &started,
            )
        };
        assert!(matches!(
            submit(&old),
            local_headless::BatchSubmission::Deferred
        ));
        assert!(!started.load(Ordering::Acquire));
        assert!(
            !fixture
                .requests()
                .iter()
                .any(|request| request.to_string().contains("GENERATION_BOUND_INPUT"))
        );

        assert!(matches!(
            submit(&current),
            local_headless::BatchSubmission::Accepted(_)
        ));
        assert!(started.load(Ordering::Acquire));
        assert_eq!(
            fixture
                .requests()
                .iter()
                .filter(|request| request.to_string().contains("GENERATION_BOUND_INPUT"))
                .count(),
            1
        );
    }

    #[test]
    fn opencode_actor_delivery_uses_ordinary_input_on_its_bound_session() {
        if std::env::var_os("OPENCODE_ACTOR_FIXTURE_CHILD").is_none() {
            let temp = tempfile::tempdir().expect("isolated Actor launcher");
            let launcher = temp.path().join("cccc");
            std::fs::write(&launcher, "#!/bin/sh\nexit 1\n").expect("offline MCP launcher");
            std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))
                .expect("launcher executable");
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args(["--exact", "ops::local_headless::supervisor_managed_tests::structured_admission::opencode_actor_delivery_uses_ordinary_input_on_its_bound_session", "--nocapture"])
                .env("OPENCODE_ACTOR_FIXTURE_CHILD", "1")
                .env("CCCC_LAUNCHER_PATH", launcher)
                .env("HOME", temp.path().join("home"))
                .env("CCCC_HOME", temp.path().join("cccc-home"))
                .env("CODEX_HOME", temp.path().join("codex"))
                .output().expect("Actor fixture child");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success()
                    && stdout
                        .lines()
                        .any(|line| line.starts_with("test result: ok. 1 passed;")),
                "Actor fixture failed or did not run:\n{stdout}\n{stderr}"
            );
            return;
        }
        let fixture = Fixture::with_runtime(ActorRuntime::Opencode);
        let event = fixture.send("OPENCODE_ACTOR_DELIVERY");
        fixture.wait(|| fixture.outcome(&event).as_deref() == Some("accepted"));
        let root = fixture.temp.path().join("opencode-events");
        let requests = std::fs::read_to_string(root.join("requests.jsonl"))
            .expect("HTTP admission trace")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("request"))
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["sessionID"], "ses_chet_fixture");
        assert!(requests[0].get("delivery").is_none());
        assert!(
            requests[0]["text"]
                .as_str()
                .expect("Actor HTTP request text")
                .contains("OPENCODE_ACTOR_DELIVERY")
        );
        assert!(
            !root.join("terminal-inputs.jsonl").exists(),
            "Actor input uses HTTP, not its viewer"
        );
    }

    #[test]
    fn rejected_actor_admission_projects_a_safe_diagnostic_without_claiming_a_turn() {
        let fixture = Fixture::new();
        let event = fixture.send("REJECT_BEFORE_RECEIPT");
        fixture.wait(|| fixture.outcome(&event).as_deref() == Some("ambiguous"));
        let journal = fixture.journal();
        let failed = journal
            .iter()
            .find(|item| item["type"] == "headless.control.failed")
            .expect("pre-admission rejection must be visible in the existing execution journal");
        assert_eq!(failed["data"]["control_kind"], "input");
        assert_eq!(failed["data"]["status"], "unconfirmed");
        assert_eq!(failed["data"]["event_id"], event.id);
        assert_eq!(failed["data"]["source_event_ids"], json!([event.id]));
        let diagnostic = failed["data"]["error"]["message"]
            .as_str()
            .expect("safe diagnostic");
        assert!(
            diagnostic.contains("-32001"),
            "numeric RPC code must remain visible"
        );
        assert!(diagnostic.contains("Synthetic admission failure"));
        assert!(diagnostic.contains("[redacted]"));
        assert!(
            diagnostic.chars().count() <= 1600,
            "diagnostic remains bounded"
        );
        let projected = serde_json::to_string(&journal).expect("projected journal");
        assert!(
            !projected.contains(SECRET),
            "private launch value must not be published"
        );
        assert!(
            !projected.contains("-admission-credential"),
            "shorter private values must not expose a longer value's suffix"
        );
        assert!(!projected.contains("DO_NOT_PUBLISH_PROVIDER_DATA"));
        assert!(!projected.contains("jsonrpc"));
        assert!(!journal.iter().any(|item| matches!(
            item["type"].as_str(),
            Some("headless.turn.started" | "headless.turn.failed")
        )));
        let state = local_headless::structured_state(&fixture.group.group_id, "worker")
            .expect("retained owner");
        assert_eq!(state["running"], true);
        assert_eq!(state["working"], false);
        assert_eq!(
            actor_delivery::dispatch_unread(&fixture.home, &fixture.group, "worker"),
            0
        );
        assert_eq!(fixture.outcome(&event).as_deref(), Some("ambiguous"));
    }

    #[test]
    fn stopping_pending_actor_admission_does_not_report_provider_rejection() {
        let fixture = Fixture::new();
        let event = fixture.send("DELAY_RECEIPT");
        fixture.wait(|| {
            std::fs::read_to_string(fixture.temp.path().join("v3_requests.jsonl"))
                .is_ok_and(|frames| frames.contains("DELAY_RECEIPT"))
        });
        actor_delivery::shutdown_actor(&fixture.group.group_id, "worker");
        assert_eq!(fixture.outcome(&event).as_deref(), Some("ambiguous"));
        assert!(
            !fixture
                .journal()
                .iter()
                .any(|item| item["type"] == "headless.control.failed")
        );
        assert_eq!(
            actor_delivery::dispatch_unread(&fixture.home, &fixture.group, "worker"),
            0
        );
    }
    #[test]
    fn cancel_reaches_a_written_prompt_and_an_idle_cancel_keeps_the_session() {
        let fixture = Fixture::new();
        let group = fixture.group.group_id.clone();
        let generation =
            local_headless::structured_state(&group, "worker").expect("running")["generation"]
                .as_str()
                .expect("generation")
                .to_owned();
        let requests = || {
            std::fs::read_to_string(fixture.temp.path().join("v3_requests.jsonl"))
                .unwrap_or_default()
        };
        let working = || {
            local_headless::structured_state(&group, "worker").expect("running")["working"]
                .as_bool()
                .expect("working")
        };
        assert!(!working(), "an idle session offers no cancel");
        assert!(
            !local_headless::cancel_turn(&group, "worker", &generation).expect("idle cancel"),
            "an idle session has nothing to cancel"
        );
        let delayed = fixture.send("DELAY_RECEIPT");
        fixture.wait(|| requests().contains("DELAY_RECEIPT"));
        // The provider has reported nothing, yet the written prompt is shown as
        // work the cancel control can stop.
        fixture.wait(working);
        assert!(
            !fixture
                .journal()
                .iter()
                .any(|item| item["type"] == "headless.turn.started"),
            "the provider has not started the turn"
        );
        assert!(
            local_headless::cancel_turn(&group, "worker", &generation).expect("cancel"),
            "a written prompt is cancellable before the provider reports it started"
        );
        fixture.wait(|| requests().contains("session/cancel"));
        // The provider ends it as cancelled: delivered, then cancelled, as a
        // started turn's cancel is recorded.
        fixture.wait(|| fixture.outcome(&delayed).as_deref() == Some("accepted"));
        fixture.wait(|| {
            fixture
                .journal()
                .iter()
                .any(|item| item["type"] == "headless.turn.cancelled")
        });
        fixture.wait(|| !working());
        let next = fixture.send("AFTER_CANCEL");
        fixture.wait(|| fixture.outcome(&next).as_deref() == Some("accepted"));
        let state = local_headless::structured_state(&group, "worker").expect("same owner");
        assert_eq!(state["running"], true);
        assert_eq!(state["generation"], generation.as_str());
    }

    /// A requested cancel alone does not confirm that a turn was cancelled. An
    /// error or a missing stop reason after it fails the turn with that reason;
    /// an end_turn answer stays cancelled, because some agents answer a cancel
    /// that way.
    #[test]
    fn a_cancel_records_an_error_or_missing_stop_reason_as_a_failure() {
        for (prompt, kind, reason) in [
            (
                "KEEP_OPEN UNKNOWN_AFTER_CANCEL",
                "headless.turn.failed",
                Some("unknown_execution"),
            ),
            (
                "KEEP_OPEN EMPTY_AFTER_CANCEL",
                "headless.turn.failed",
                Some("without a stop reason"),
            ),
            (
                "KEEP_OPEN END_AFTER_CANCEL",
                "headless.turn.cancelled",
                None,
            ),
        ] {
            let fixture = Fixture::new();
            let group = fixture.group.group_id.clone();
            let generation =
                local_headless::structured_state(&group, "worker").expect("running")["generation"]
                    .as_str()
                    .expect("generation")
                    .to_owned();
            let event = fixture.send(prompt);
            fixture.wait(|| fixture.outcome(&event).as_deref() == Some("accepted"));
            assert!(
                local_headless::cancel_turn(&group, "worker", &generation).expect("cancel"),
                "{prompt}: the open turn is cancellable"
            );
            fixture.wait(|| fixture.journal().iter().any(|item| item["type"] == kind));
            let journal = fixture.journal();
            let settled: Vec<_> = journal
                .iter()
                .filter_map(|item| item["type"].as_str())
                .filter(|kind| {
                    matches!(
                        *kind,
                        "headless.turn.failed"
                            | "headless.turn.cancelled"
                            | "headless.turn.completed"
                    )
                })
                .collect();
            assert_eq!(settled, [kind], "{prompt}: settled once, as {kind}");
            if let Some(reason) = reason {
                let failed = journal
                    .iter()
                    .find(|item| item["type"] == kind)
                    .expect("failed turn");
                assert!(
                    failed["data"]["error"]
                        .as_str()
                        .is_some_and(|error| error.contains(reason)),
                    "{prompt}: the provider's reason is kept: {failed}"
                );
            }
            let next = fixture.send("AFTER_CANCEL");
            fixture.wait(|| fixture.outcome(&next).as_deref() == Some("accepted"));
        }
    }

    /// Hermes finishes a turn in its shared gateway after the client closes,
    /// so stop must cancel it first and record what it observed. Only the
    /// prompt's own stopReason proves it ended; an error leaves it unknown.
    #[test]
    fn stopping_hermes_cancels_its_open_turn_before_closing_the_client() {
        for (prompt, expected) in [
            ("DELAY_RECEIPT", "settled"),
            ("KEEP_OPEN", "settled"),
            ("DELAY_RECEIPT IGNORE_CANCEL", "interrupt_requested"),
            ("DELAY_RECEIPT UNKNOWN_AFTER_CANCEL", "interrupt_requested"),
            ("KEEP_OPEN UNKNOWN_AFTER_CANCEL", "interrupt_requested"),
        ] {
            let fixture = Fixture::with_runtime(ActorRuntime::Hermes);
            let group = fixture.group.group_id.clone();
            let requests = || {
                std::fs::read_to_string(fixture.temp.path().join("v3_requests.jsonl"))
                    .unwrap_or_default()
            };
            fixture.send(prompt);
            fixture.wait(|| requests().contains(prompt));
            actor_delivery::shutdown_actor(&group, "worker");
            let started = Instant::now();
            local_headless::stop(&group, "worker").expect("stop Hermes");
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{prompt}: stop is bounded"
            );
            assert!(
                requests().contains("session/cancel"),
                "{prompt}: cancel before close"
            );
            let journal = fixture.journal();
            let stopped = journal
                .iter()
                .rfind(|item| item["type"] == "headless.session.stopped")
                .expect("stopped event");
            assert_eq!(stopped["data"]["interrupt"]["acp"], expected, "{prompt}");
            assert!(
                stopped["data"]["interrupt"].get("terminal").is_none(),
                "{prompt}: an ACP-mode member has no terminal"
            );
        }
    }

    /// A Hermes profile without a provider advertises only its own setup. The
    /// member's terminal runs it while messages wait; when it exits the member
    /// connects once, opens its own terminal and gets them.
    #[test]
    fn hermes_first_setup_runs_in_the_member_terminal_and_connects_once_it_exits() {
        let fixture = Fixture::build(
            ActorRuntime::Hermes,
            RuntimeMode::Default,
            &["v3_setup_needed"],
        );
        let group = fixture.group.group_id.clone();
        let root = fixture.temp.path().to_path_buf();
        fixture.wait(|| !fixture.setup_runs().is_empty());
        assert_eq!(fixture.setup_runs(), [json!(["acp", "--setup"])]);
        assert!(local_headless::confirmation_terminal_owns(&group, "worker").expect("owner"));
        assert!(local_headless::confirmation_pending_active(&group, "worker").expect("recovery"));
        assert!(!local_headless::running(&group, "worker"));
        let event = fixture.send("AFTER_SETUP");
        std::fs::write(root.join("v3_setup_finish"), "").expect("finish the setup");
        fixture.wait(|| fixture.outcome(&event).as_deref() == Some("accepted"));
        assert!(local_headless::running(&group, "worker"));
        fixture.wait(|| root.join("v3_viewer_ready").exists());
        let requests = fixture.requests();
        // One session refused before the setup, one created after it.
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "session/new")
                .count(),
            2
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "session/prompt"
                    && request.to_string().contains("AFTER_SETUP"))
                .count(),
            1
        );
        assert_eq!(fixture.setup_runs().len(), 1);
        assert!(!local_headless::confirmation_paused(&group, "worker").expect("pause"));
    }

    /// A setup left without a provider gets one connection attempt and no
    /// second setup; automatic starts then wait for the user's own start.
    #[test]
    fn a_hermes_setup_left_without_a_provider_waits_for_the_user_to_start_again() {
        let fixture = Fixture::build(
            ActorRuntime::Hermes,
            RuntimeMode::Default,
            &["v3_setup_needed", "v3_setup_quit"],
        );
        let group = fixture.group.group_id.clone();
        fixture.wait(|| !fixture.setup_runs().is_empty());
        std::fs::write(fixture.temp.path().join("v3_setup_finish"), "").expect("leave setup");
        fixture.wait(|| local_headless::confirmation_paused(&group, "worker").expect("pause"));
        fixture.wait(|| {
            !local_headless::confirmation_pending_active(&group, "worker").expect("recovery")
        });
        assert!(!local_headless::running(&group, "worker"));
        assert_eq!(
            fixture
                .requests()
                .iter()
                .filter(|request| request["method"] == "session/new")
                .count(),
            2
        );
        assert_eq!(fixture.setup_runs().len(), 1);
        assert!(fixture.journal().iter().any(|item| {
            item["type"] == "headless.session.exited"
                && item["data"]["reason"] == "interactive_confirmation_ended"
        }));
        let actor = fixture.group.actors[0].clone();
        let paused = local_headless::start(&fixture.home, &fixture.group, &actor)
            .expect_err("automatic starts wait");
        assert_eq!(paused.kind(), std::io::ErrorKind::WouldBlock);
        local_headless::retry_confirmation(&group, "worker").expect("the user's start");
        local_headless::start(&fixture.home, &fixture.group, &actor).expect("setup again");
        fixture.wait(|| fixture.setup_runs().len() == 2);
    }

    /// Without a terminal there is nowhere to run the setup: the start fails
    /// and names it.
    #[test]
    fn hermes_in_acp_mode_names_its_setup_without_opening_a_terminal() {
        let (fixture, actor) =
            Fixture::unstarted(ActorRuntime::Hermes, RuntimeMode::Acp, &["v3_setup_needed"]);
        let error = local_headless::start(&fixture.home, &fixture.group, &actor)
            .expect_err("no provider yet");
        let message = error.to_string();
        assert!(
            message.contains("acp --setup") && message.contains("storage_unavailable"),
            "{message}"
        );
        assert!(
            cccc_runtime::status(&fixture.group.group_id, "worker")
                .map_or(true, |status| !status.running)
        );
        assert!(fixture.setup_runs().is_empty());
    }

    /// Hermes's default mode keeps its own terminal, attached to the member's
    /// ACP session; CCCC still delivers through ACP and never types into it.
    #[test]
    fn hermes_default_mode_keeps_its_terminal_as_a_viewer_of_the_acp_session() {
        let fixture = Fixture::build(ActorRuntime::Hermes, RuntimeMode::Default, &[]);
        let group = fixture.group.group_id.clone();
        let state = local_headless::structured_state(&group, "worker").expect("managed Hermes");
        assert_eq!(state["runner"], "pty");
        assert_eq!(state["runtime_mode"], "default");
        let viewer = cccc_runtime::status(&group, "worker").expect("Hermes terminal");
        assert!(viewer.running);
        fixture.wait(|| fixture.temp.path().join("v3_viewer_ready").exists());
        let process: Value = serde_json::from_slice(
            &std::fs::read(fixture.temp.path().join("v3_viewer_process.json"))
                .expect("viewer process observation"),
        )
        .expect("viewer process record");
        assert_eq!(
            process["pid"].as_u64(),
            Some(u64::from(viewer.pid.expect("viewer pid")))
        );
        let args: Vec<String> =
            serde_json::from_value(process["argv"].clone()).expect("actual viewer arguments");
        assert!(args.iter().any(|arg| arg == "--tui"), "{args:?}");
        assert!(!args.iter().any(|arg| arg == "--yolo"), "{args:?}");
        let resume = args
            .iter()
            .position(|arg| arg == "--resume")
            .expect("the terminal resumes the member session");

        let event = fixture.send("THROUGH_ACP");
        fixture.wait(|| fixture.outcome(&event).as_deref() == Some("accepted"));
        let prompt = std::fs::read_to_string(fixture.temp.path().join("v3_requests.jsonl"))
            .expect("fixture requests")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("request"))
            .find(|request| request["method"] == "session/prompt")
            .expect("delivered through ACP");
        assert_eq!(
            args[resume + 1],
            prompt["params"]["sessionId"].as_str().expect("id")
        );
        assert!(prompt.to_string().contains("THROUGH_ACP"));

        actor_delivery::shutdown_actor(&group, "worker");
        local_headless::stop(&group, "worker").expect("stop");
        assert!(
            cccc_runtime::status(&group, "worker").map_or(true, |status| !status.running),
            "stop closes the terminal"
        );
        // The ACP prompt had finished; the terminal gets Ctrl+C for any turn
        // typed there, and CCCC records only that it asked.
        let journal = fixture.journal();
        let stopped = journal
            .iter()
            .rfind(|item| item["type"] == "headless.session.stopped")
            .expect("stopped event");
        assert_eq!(
            stopped["data"]["interrupt"],
            serde_json::json!({"terminal":"requested"})
        );
    }

    /// A Hermes that stopped reading leaves the protocol loop blocked on its
    /// write; stop must still end within its deadlines and say the cancel was
    /// never confirmed.
    #[test]
    fn stopping_a_hermes_that_stopped_reading_is_bounded_and_unconfirmed() {
        let fixture = Fixture::with_markers(ActorRuntime::Hermes, &["v3_stall_stdin"]);
        let group = fixture.group.group_id.clone();
        // 480 KiB exceeds Darwin's 64 KiB pipe while leaving room for the ACP
        // JSON envelope below 1 MiB instead of relying on an oversized frame.
        let event = fixture.send(&"STALLED PROMPT ".repeat(32 * 1024));
        fixture.wait(|| fixture.outcome(&event).as_deref() == Some("claimed"));
        // The prompt write cannot finish; give it time to fill the pipe.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(fixture.outcome(&event).as_deref(), Some("claimed"));
        // A prompt still being written is nothing a cancel could reach yet.
        assert_eq!(
            local_headless::structured_state(&group, "worker").expect("running")["working"],
            false
        );
        actor_delivery::shutdown_actor(&group, "worker");
        let started = Instant::now();
        local_headless::stop(&group, "worker").expect("stop a stalled Hermes");
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "stop is bounded"
        );
        let journal = fixture.journal();
        let stopped = journal
            .iter()
            .rfind(|item| item["type"] == "headless.session.stopped")
            .expect("stopped event");
        assert_eq!(stopped["data"]["interrupt"]["acp"], "unconfirmed");
    }

    #[test]
    fn stopping_an_idle_or_other_acp_session_records_no_interrupt() {
        for runtime in [ActorRuntime::Hermes, ActorRuntime::Kiro] {
            let fixture = Fixture::with_runtime(runtime);
            let group = fixture.group.group_id.clone();
            if runtime == ActorRuntime::Kiro {
                fixture.send("DELAY_RECEIPT");
                fixture.wait(|| {
                    std::fs::read_to_string(fixture.temp.path().join("v3_requests.jsonl"))
                        .unwrap_or_default()
                        .contains("DELAY_RECEIPT")
                });
                actor_delivery::shutdown_actor(&group, "worker");
            }
            local_headless::stop(&group, "worker").expect("stop");
            let requests = std::fs::read_to_string(fixture.temp.path().join("v3_requests.jsonl"))
                .unwrap_or_default();
            assert!(!requests.contains("session/cancel"), "{runtime:?}");
            let journal = fixture.journal();
            let stopped = journal
                .iter()
                .rfind(|item| item["type"] == "headless.session.stopped")
                .expect("stopped event");
            assert!(stopped["data"].get("interrupt").is_none(), "{runtime:?}");
        }
    }

    /// A Hermes terminal that stopped reading, with input already blocked in
    /// it, cannot hold up stop: the Ctrl+C gets a bounded time, is recorded as
    /// unconfirmed, and the terminal is closed anyway.
    #[test]
    fn stopping_hermes_is_bounded_when_its_terminal_stopped_reading() {
        let fixture = Fixture::build(ActorRuntime::Hermes, RuntimeMode::Default, &[]);
        let group = fixture.group.group_id.clone();
        assert!(
            cccc_runtime::status(&group, "worker")
                .expect("terminal")
                .running
        );
        fixture.wait(|| fixture.temp.path().join("v3_viewer_ready").exists());
        let filler = {
            let group = group.clone();
            std::thread::spawn(move || cccc_runtime::write(&group, "worker", &vec![b'x'; 1 << 20]))
        };
        // The terminal reads nothing: the queue fills and the filler keeps
        // holding the terminal's input.
        std::thread::sleep(Duration::from_millis(300));
        assert!(!filler.is_finished(), "input is blocked before stop");
        actor_delivery::shutdown_actor(&group, "worker");
        let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
        let stopping = group.clone();
        std::thread::spawn(move || {
            let _ = stopped_tx.send(local_headless::stop(&stopping, "worker"));
        });
        stopped_rx
            .recv_timeout(Duration::from_secs(8))
            .expect("stop is bounded")
            .expect("stop");
        assert!(
            cccc_runtime::status(&group, "worker").map_or(true, |status| !status.running),
            "stop closes the terminal"
        );
        let journal = fixture.journal();
        let stopped = journal
            .iter()
            .rfind(|item| item["type"] == "headless.session.stopped")
            .expect("stopped event");
        assert_eq!(
            stopped["data"]["interrupt"],
            serde_json::json!({"terminal":"unconfirmed"})
        );
    }

    /// A turn typed in Hermes's own terminal reaches this client only as
    /// session activity. Hermes cancels only this client's prompts, so stop
    /// leaves that turn to the terminal and records no ACP evidence for it.
    #[test]
    fn stopping_hermes_leaves_a_terminal_typed_turn_to_the_terminal() {
        let fixture = Fixture::with_markers(ActorRuntime::Hermes, &["v3_terminal_turns"]);
        let group = fixture.group.group_id.clone();
        std::fs::write(fixture.temp.path().join("v3_type_in_terminal"), "")
            .expect("type a turn in the terminal");
        fixture.wait(|| {
            fixture
                .journal()
                .iter()
                .any(|item| item["type"] == "headless.turn.started")
        });
        local_headless::stop(&group, "worker").expect("stop");
        let requests = std::fs::read_to_string(fixture.temp.path().join("v3_requests.jsonl"))
            .unwrap_or_default();
        assert!(!requests.contains("session/cancel"));
        let journal = fixture.journal();
        let stopped = journal
            .iter()
            .rfind(|item| item["type"] == "headless.session.stopped")
            .expect("stopped event");
        assert!(stopped["data"].get("interrupt").is_none());
    }

    #[test]
    fn instruction_change_before_first_delivery_is_retried_after_unconfirmed_admission() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.temp.path().join("AGENTS.md"),
            "updated instructions",
        )
        .expect("update instruction file");

        let uncertain = fixture.send("REJECT_BEFORE_RECEIPT");
        fixture.wait(|| fixture.outcome(&uncertain).as_deref() == Some("ambiguous"));
        let accepted = fixture.send("AFTER_UNCONFIRMED_ADMISSION");
        fixture.wait(|| fixture.outcome(&accepted).as_deref() == Some("accepted"));
        let later = fixture.send("AFTER_CONFIRMED_ADMISSION");
        fixture.wait(|| fixture.outcome(&later).as_deref() == Some("accepted"));

        let requests = std::fs::read_to_string(fixture.temp.path().join("v3_requests.jsonl"))
            .expect("fixture requests");
        let prompts = requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("request"))
            .filter(|request| request["method"] == "session/prompt")
            .filter_map(|request| {
                request["params"]["prompt"][0]["text"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        let prompt = |needle: &str| {
            prompts
                .iter()
                .find(|text| text.contains(needle))
                .unwrap_or_else(|| panic!("missing prompt: {needle}"))
        };
        assert!(prompt("REJECT_BEFORE_RECEIPT").contains("INSTRUCTION FILES CHANGED"));
        assert!(prompt("AFTER_UNCONFIRMED_ADMISSION").contains("INSTRUCTION FILES CHANGED"));
        assert!(!prompt("AFTER_CONFIRMED_ADMISSION").contains("INSTRUCTION FILES CHANGED"));
    }
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

#[test]
fn terminal_handoff_evidence_distinguishes_zero_byte_deferral_from_partial_write_uncertainty() {
    use super::BatchSubmission;
    use super::supervisor::{classify_terminal_handoff, reconcile_owner_submission};
    use cccc_runtime::{PtyHandoff, PtyHandoffReason, RuntimeError};
    use std::sync::atomic::{AtomicBool, Ordering};

    let io_err = || {
        PtyHandoffReason::Runtime(RuntimeError::Io(std::io::Error::from(
            std::io::ErrorKind::BrokenPipe,
        )))
    };

    // 1. Non-owner, >=1 byte written before I/O failure (cancelled stays false):
    //    must settle Unconfirmed with started=true and never trigger a second write.
    let started = AtomicBool::new(false);
    let mut attempts = 0;
    let mut final_outcome = BatchSubmission::Deferred;
    for _ in 0..3 {
        attempts += 1;
        final_outcome = classify_terminal_handoff(&started, || PtyHandoff::Uncertain(io_err()));
        if final_outcome != BatchSubmission::Deferred {
            break;
        }
    }
    assert_eq!(attempts, 1, "partial write must never auto-retry");
    assert_eq!(final_outcome, BatchSubmission::Unconfirmed);
    assert!(started.load(Ordering::Acquire));
    assert_eq!(
        reconcile_owner_submission(Some(final_outcome), started.load(Ordering::Acquire)),
        BatchSubmission::Unconfirmed
    );

    // 2. 0-byte NotWritten: resets started=false and stays Deferred on both non-owner
    //    and Unix owner reconciliation, allowing the next attempt to deliver once.
    let started = AtomicBool::new(false);
    let first = classify_terminal_handoff(&started, || PtyHandoff::NotWritten(io_err()));
    assert_eq!(first, BatchSubmission::Deferred);
    assert!(
        !started.load(Ordering::Acquire),
        "0-byte NotWritten must reset started so Unix owner does not misclassify it as Unconfirmed"
    );
    assert_eq!(
        reconcile_owner_submission(Some(first), started.load(Ordering::Acquire)),
        BatchSubmission::Deferred
    );

    let second = classify_terminal_handoff(&started, || PtyHandoff::Accepted);
    assert_eq!(
        second,
        BatchSubmission::Accepted(Some(
            cccc_contracts::event::DeliveryEvidence::TerminalWritten
        ))
    );
    assert!(started.load(Ordering::Acquire));
    assert_eq!(
        reconcile_owner_submission(Some(second), started.load(Ordering::Acquire)),
        BatchSubmission::Accepted(Some(
            cccc_contracts::event::DeliveryEvidence::TerminalWritten
        ))
    );

    // 3. Conservative owner guard: Deferred paired with started=true still upgrades to Unconfirmed.
    assert_eq!(
        reconcile_owner_submission(Some(BatchSubmission::Deferred), true),
        BatchSubmission::Unconfirmed
    );
}

#[cfg(unix)]
#[test]
fn managed_terminal_submit_key_io_error_after_payload_is_unconfirmed_without_retry() {
    use super::BatchSubmission;
    use super::supervisor::{reconcile_owner_submission, submit_managed_terminal_input};
    use cccc_contracts::ActorSubmit;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    let temp = tempfile::tempdir().expect("isolated pty fixture");
    let ready = temp.path().join("ready");
    let out = temp.path().join("out");
    let group_id = format!("g_partial_pty_{}", std::process::id());
    let mut actor = Actor::new("managed-viewer");
    actor.runtime = ActorRuntime::Custom;
    actor.submit = ActorSubmit::Enter;

    // 0-byte control: before any PTY exists, submission defers with started=false.
    let cancelled = AtomicBool::new(false);
    let started = AtomicBool::new(false);
    let absent = submit_managed_terminal_input(&group_id, &actor, "hello", &cancelled, &started);
    assert_eq!(absent, BatchSubmission::Deferred);
    assert!(!started.load(Ordering::Acquire));
    assert_eq!(
        reconcile_owner_submission(Some(absent), started.load(Ordering::Acquire)),
        BatchSubmission::Deferred
    );

    // Real PTY that reads the 5-byte payload then exits before the submit key is written,
    // forcing a post-payload I/O failure while cancelled stays false.
    let mut env = std::collections::BTreeMap::new();
    env.insert("READY".into(), ready.to_string_lossy().into_owned());
    env.insert("OUT".into(), out.to_string_lossy().into_owned());
    cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: group_id.clone(),
        actor_id: actor.id.clone(),
        runner: RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "stty raw -echo; touch \"$READY\"; dd bs=1 count=5 of=\"$OUT\" 2>/dev/null; exit 0"
                .into(),
        ],
        cwd: temp.path().into(),
        env,
        cols: 80,
        rows: 24,
    })
    .expect("start real pty");
    struct Cleanup(String, String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = cccc_runtime::stop(&self.0, &self.1);
        }
    }
    let _cleanup = Cleanup(group_id.clone(), actor.id.clone());

    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(ready.exists(), "pty fixture did not become ready");

    // 0-byte pre-write rejection on a live PTY (empty trimmed input -> NotWritten):
    // started resets to false so owner reconciliation keeps Deferred.
    let pre_write = submit_managed_terminal_input(&group_id, &actor, "\r\n", &cancelled, &started);
    assert_eq!(pre_write, BatchSubmission::Deferred);
    assert!(!started.load(Ordering::Acquire));
    assert_eq!(
        reconcile_owner_submission(Some(pre_write), started.load(Ordering::Acquire)),
        BatchSubmission::Deferred
    );

    // Payload is written and received, then submit-key write fails after process exit.
    let outcome = submit_managed_terminal_input(&group_id, &actor, "hello", &cancelled, &started);
    assert!(!cancelled.load(Ordering::Acquire));
    assert_eq!(
        std::fs::read_to_string(&out).expect("payload reached pty"),
        "hello"
    );
    assert_eq!(
        outcome,
        BatchSubmission::Unconfirmed,
        "post-payload submit-key failure must not collapse into Deferred retry"
    );
    assert!(started.load(Ordering::Acquire));
}
#[cfg(unix)]
#[test]
fn managed_owner_acceptance_keeps_the_wire_outcome_and_optional_evidence() {
    use super::BatchSubmission;
    use cccc_contracts::event::DeliveryEvidence;
    use serde_json::json;

    let old = json!({"backend":"managed","outcome":"Accepted","started":true});
    assert_eq!(
        BatchSubmission::from_owner_result(&old),
        Some(BatchSubmission::Accepted(None))
    );
    for evidence in [
        DeliveryEvidence::TerminalWritten,
        DeliveryEvidence::ChannelAccepted,
    ] {
        let accepted = BatchSubmission::Accepted(Some(evidence));
        let result = accepted.owner_result(true);
        assert_eq!(result["outcome"], "Accepted");
        assert_eq!(result["delivery_evidence"], json!(evidence));
        assert_eq!(BatchSubmission::from_owner_result(&result), Some(accepted));
    }
}
