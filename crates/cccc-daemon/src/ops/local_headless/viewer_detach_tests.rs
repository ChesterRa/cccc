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
    let provider_alive = Arc::new(AtomicBool::new(true));
    let server = tokio::spawn({
        let provider_alive = Arc::clone(&provider_alive);
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
                    "list" => {
                        json!({"ok":true,"op":"list","jobs":if provider_alive.load(std::sync::atomic::Ordering::Acquire) { vec![json!({"short":"abcdef01"})] } else { vec![] }})
                    }
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
        command: vec![
            "sh".into(),
            "-c".into(),
            "stty -echo; printf '\\033[?2004h'; exec cat".into(),
        ],
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
            true,
        )),
        has_terminal: AtomicBool::new(true),
        viewer: Mutex::new(Some(viewer.clone())),
        status: Mutex::new(HeadlessStatus {
            status: "idle".into(),
            task_id: None,
            updated_at: String::new(),
            pid: attached.pid,
            reason: None,
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
    assert!(
        item.running(),
        "live provider must remain running before viewer reconciliation"
    );
    // A lifecycle start before the reaper runs must reconnect the same provider.
    let current_group = store.load(&group.group_id).expect("current group");
    crate::ops::actor_runtime::apply(&home, &current_group, "claude-1", "actor.start")
        .expect("start reconnects viewer");
    crate::ops::actor_runtime::reconcile_exited(&home, vec![exited])
        .expect("late exit must not detach the replacement");
    assert!(item.has_terminal());
    assert!(item.running());
    let exited = cccc_runtime::stop(&group.group_id, "claude-1").expect("second viewer exit");
    // Saved settings may already target a different backend; the live owner wins.
    store
        .mutate(&group.group_id, |document| {
            document.actors[0].runtime = ActorRuntime::Custom;
            Ok(())
        })
        .expect("change next-launch runtime");
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

    // A passive viewer must not launch work. A control attach can reconnect.
    let passive = attach_request(&home, &group.group_id, "viewer").await;
    assert!(!passive.ok);
    assert!(!item.has_terminal());
    let control = attach_request(&home, &group.group_id, "control").await;
    assert!(control.ok, "{control:?}");
    assert!(item.has_terminal());
    assert!(
        cccc_runtime::status(&group.group_id, "claude-1")
            .expect("reattached viewer")
            .running
    );
    assert!(operations.lock().expect("operations").is_empty());

    // Delivery must also reopen an exited TUI before the reaper has seen it.
    let exited = cccc_runtime::stop(&group.group_id, "claude-1").expect("exit before delivery");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"text":"ISOLATED_VIEWER_DELIVERY","to":["claude-1"]})
        .as_object()
        .expect("message")
        .clone();
    let delivered = tokio::task::spawn_blocking({
        let home = home.clone();
        let group = current_group.clone();
        move || {
            submit_batch(
                &home,
                &group,
                &group.actors[0],
                &[event],
                &AtomicBool::new(false),
            )
        }
    })
    .await
    .expect("delivery task");
    assert_eq!(
        delivered,
        super::super::BatchSubmission::Accepted,
        "delivery reconnects the same provider's input channel"
    );
    crate::ops::actor_runtime::reconcile_exited(&home, vec![exited]).expect("late delivery reap");
    assert!(item.has_terminal());
    assert!(operations.lock().expect("operations").is_empty());

    // Losing the provider process must preserve the durable conversation receipt.
    let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
    let command = vec!["claude".into()];
    let environment = BTreeMap::new();
    crate::ops::runtime_session::record_claude_managed_session(
        &home,
        &group.group_id,
        "claude-1",
        &config,
        &command,
        &environment,
        session_id,
        false,
    )
    .expect("record durable session");
    provider_alive.store(false, std::sync::atomic::Ordering::Release);
    tokio::task::spawn_blocking({
        let item = Arc::clone(&item);
        move || super::super::managed_reader::stop_after_provider_exit(&item)
    })
    .await
    .expect("reconcile provider exit");
    assert!(item.stopped.load(std::sync::atomic::Ordering::Acquire));
    assert!(!item.running());
    let resumed = crate::ops::runtime_session::prepare_claude_managed_session(
        &home,
        &group.group_id,
        "claude-1",
        &config,
        &command,
        &environment,
    )
    .expect("prepare durable resume");
    assert_eq!(
        resumed.as_ref().map(|attempt| attempt.session_id.as_str()),
        Some(session_id)
    );
    assert_eq!(operations.lock().expect("operations").as_slice(), ["list"]);
    let events =
        cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
            .expect("ledger after provider exit");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "actor.stop")
            .count(),
        1
    );
    let closed = attach_request(&home, &group.group_id, "control").await;
    assert!(
        !closed.ok,
        "control attach cannot restart a stopped provider"
    );
    server.abort();
}

async fn attach_request(
    home: &HomeLayout,
    group_id: &str,
    mode: &str,
) -> cccc_contracts::DaemonResponse {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (_shutdown, receiver) = tokio::sync::watch::channel(false);
    let request = cccc_contracts::DaemonRequest {
        v: 1,
        op: "term_attach".into(),
        args: json!({"group_id":group_id,"actor_id":"claude-1","mode":mode})
            .as_object()
            .expect("args")
            .clone(),
    };
    let server = tokio::spawn(crate::server_terminal_attach::handle(
        BufReader::new(server),
        home.clone(),
        request,
        receiver,
    ));
    let mut client = BufReader::new(client);
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.read_line(&mut line),
    )
    .await
    .expect("attach timeout")
    .expect("response");
    let response = serde_json::from_str(&line).expect("attachment response");
    // End input while retaining the read half until the server finishes its
    // initial terminal output. Dropping both halves races that output write.
    client
        .get_mut()
        .shutdown()
        .await
        .expect("end fixture input");
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .expect("detach timeout")
        .expect("server task")
        .expect("server stream");
    response
}
