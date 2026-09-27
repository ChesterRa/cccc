use super::*;
use crate::ops::codex_voice_analyst::AnalystSession;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reaped_viewer_detaches_without_stopping_the_provider_job() {
    // An isolated test process owns the supervisor and runtime registries.
    const CHILD: &str = "CCCC_TEST_VIEWER_DETACH";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "ops::local_headless::supervisor::viewer_detach_tests::a_reaped_viewer_detaches_without_stopping_the_provider_job", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .expect("isolated supervisor");
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("fixture home");
    let config = temp.path().canonicalize().expect("config");
    let (listener, _directory) = super::control_fixture::bind(&config);
    // The provider job stays listed; record every control operation it receives.
    let operations = Arc::new(Mutex::new(Vec::<String>::new()));
    let server = tokio::spawn({
        let operations = Arc::clone(&operations);
        async move {
            loop {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).await.expect("request");
                let request: Value = serde_json::from_str(&line).expect("JSON");
                let op = request["op"].as_str().expect("operation").to_owned();
                operations.lock().expect("operations").push(op.clone());
                let response = match op.as_str() {
                    "list" => json!({"ok":true,"op":"list","jobs":[{"short":"abcdef01"}]}),
                    other => json!({"ok":true,"op":other}),
                };
                stream
                    .get_mut()
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .expect("response");
            }
        }
    });

    let home = HomeLayout::from_path(config.join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let group = store.create("viewer detach", "").expect("group");
    store
        .mutate(&group.group_id, |document| {
            let mut actor = Actor::new("claude-1");
            actor.runtime = ActorRuntime::Claude;
            document.actors.push(actor);
            Ok(())
        })
        .expect("actor");
    let viewer = super::super::ViewerLaunch {
        command: vec!["sleep".into(), "30".into()],
        env: BTreeMap::from([("PATH".into(), std::env::var("PATH").unwrap_or_default())]),
        cwd: config.clone(),
    };
    let viewer_spec = || cccc_runtime::LaunchSpec {
        group_id: group.group_id.clone(),
        actor_id: "claude-1".into(),
        runner: RunnerKind::Pty,
        command: viewer.command.clone(),
        cwd: viewer.cwd.clone(),
        env: viewer.env.clone(),
        cols: 120,
        rows: 40,
    };
    let attached = cccc_runtime::start(viewer_spec()).expect("viewer attach");
    let item = Arc::new(Session {
        home: home.clone(),
        group_id: group.group_id.clone(),
        actor_id: "claude-1".into(),
        managed: Arc::new(AnalystSession::claude_for_shutdown_test(
            &config,
            "abcdef01",
            Vec::new(),
        )),
        has_terminal: AtomicBool::new(true),
        viewer: Mutex::new(Some(viewer.clone())),
        status: Mutex::new(HeadlessStatus {
            status: "idle".into(),
            task_id: None,
            updated_at: String::new(),
            pid: attached.pid,
        }),
        stopped: AtomicBool::new(false),
        stop_lock: Mutex::new(()),
        startup_prompt: Mutex::new(None),
        active_turn: Mutex::new(None),
    });
    sessions().write().expect("registry").insert(
        (group.group_id.clone(), "claude-1".into()),
        Arc::clone(&item),
    );

    // The `claude attach` viewer exits and the runtime reaper reconciles it.
    let exited = cccc_runtime::stop(&group.group_id, "claude-1").expect("viewer exit");
    crate::ops::actor_runtime::reconcile_exited(&home, vec![exited]).expect("reconcile");

    assert!(
        operations.lock().expect("operations").is_empty(),
        "a viewer exit must not ask Agent View to stop the provider job"
    );
    assert!(!item.stopped.load(std::sync::atomic::Ordering::Acquire));
    assert!(!item.has_terminal());
    let ledger =
        cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
            .expect("ledger");
    assert!(
        ledger.iter().all(|event| event.kind != "actor.stop"),
        "a viewer exit is not a provider exit"
    );

    // The next delivery re-opens the viewer on the same live job.
    item.reattach_viewer().expect("reattach");
    assert!(item.has_terminal());
    assert!(
        cccc_runtime::status(&group.group_id, "claude-1")
            .expect("reattached viewer")
            .running
    );
    cccc_runtime::stop(&group.group_id, "claude-1").expect("stop viewer");
    server.abort();
}
