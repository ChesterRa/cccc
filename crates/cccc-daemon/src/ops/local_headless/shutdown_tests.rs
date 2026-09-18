use super::*;
use crate::ops::codex_voice_analyst::AnalystSession;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[allow(clippy::await_holding_lock)] // Deliberately simulate an in-progress graceful stop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_requests_all_jobs_and_confirms_stops_concurrently() {
    // An isolated test process owns the supervisor's global registry.
    const CHILD: &str = "CCCC_TEST_CONCURRENT_MANAGED_STOP";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "ops::local_headless::supervisor::shutdown_tests::shutdown_requests_all_jobs_and_confirms_stops_concurrently", "--nocapture"])
            .env(CHILD, "1")
            .output().expect("isolated supervisor");
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
    let digest = format!("{:x}", Sha256::digest(config.to_string_lossy().as_bytes()));
    let directory = std::path::PathBuf::from("/tmp")
        .join(format!(
            "cc-daemon-{}",
            std::fs::metadata(&config).expect("metadata").uid()
        ))
        .join(&digest[..8]);
    std::fs::create_dir_all(&directory).expect("control directory");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .expect("permissions");
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _directory = Directory(directory.clone());
    let listener = tokio::net::UnixListener::bind(directory.join("control.sock")).expect("socket");
    let mode = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let stopped = Arc::new(Mutex::new(HashSet::<String>::new()));
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let parallel = Arc::new(AtomicBool::new(true));
    let short_ids = ["abcdef01", "abcdef02", "abcdef03"];
    let server = tokio::spawn({
        let (mode, requests, stopped, barrier, parallel) = (
            Arc::clone(&mode),
            Arc::clone(&requests),
            Arc::clone(&stopped),
            Arc::clone(&barrier),
            Arc::clone(&parallel),
        );
        async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.expect("accept");
                        let (mode, requests, stopped, barrier, parallel) = (Arc::clone(&mode), Arc::clone(&requests), Arc::clone(&stopped), Arc::clone(&barrier), Arc::clone(&parallel));
                        clients.spawn(async move {
                            let mut stream = BufReader::new(stream);
                            let mut line = String::new();
                            stream.read_line(&mut line).await.expect("request");
                            if line.is_empty() { return; }
                            let request: Value = serde_json::from_str(&line).expect("JSON");
                            let op = request["op"].as_str().expect("operation");
                            let response = match op {
                                "list" => {
                                    let stopped = stopped.lock().expect("stopped");
                                    let jobs = short_ids.iter().filter(|id| !stopped.contains(**id)).map(|id| json!({"short":id})).collect::<Vec<_>>();
                                    json!({"ok":true,"op":"list","jobs":jobs})
                                }
                                "kill" => {
                                    let short = request["short"].as_str().expect("short");
                                    assert!(short_ids.contains(&short));
                                    assert_eq!(request["signal"], "SIGTERM");
                                    requests.lock().expect("requests").push(short.into());
                                    match mode.load(Ordering::Acquire) {
                                        1 => {
                                            if tokio::time::timeout(Duration::from_secs(3), barrier.wait()).await.is_err() {
                                                parallel.store(false, Ordering::Release);
                                            }
                                            stopped.lock().expect("stopped").insert(short.into());
                                        }
                                        2 => return, // An unavailable provider must not block other stop requests.
                                        _ => {} // Acknowledged, but the provider job remains present.
                                    }
                                    json!({"ok":true,"op":"kill"})
                                }
                                other => panic!("unexpected operation {other}"),
                            };
                            stream.get_mut().write_all(format!("{response}\n").as_bytes()).await.expect("response");
                        });
                    }
                    Some(result) = clients.join_next(), if !clients.is_empty() => { result.expect("control handler"); }
                }
            }
        }
    });
    let home = HomeLayout::from_path(config.join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let group = store.create("shutdown fixture", "").expect("group");
    let blocked_cleanup = config.join("blocked-cleanup");
    std::fs::create_dir(&blocked_cleanup).expect("cleanup failure fixture");
    let items = short_ids
        .iter()
        .map(|short| {
            let item = Arc::new(Session {
                home: home.clone(),
                group_id: group.group_id.clone(),
                actor_id: (*short).into(),
                managed: Arc::new(AnalystSession::claude_for_shutdown_test(
                    &config,
                    short,
                    if *short == "abcdef02" {
                        vec![blocked_cleanup.clone()]
                    } else {
                        Vec::new()
                    },
                )),
                has_terminal: AtomicBool::new(false),
                status: Mutex::new(HeadlessStatus {
                    status: "idle".into(),
                    task_id: None,
                    updated_at: String::new(),
                    pid: None,
                }),
                stopped: AtomicBool::new(false),
                stop_lock: Mutex::new(()),
                startup_prompt: Mutex::new(None),
                active_turn: Mutex::new(None),
            });
            sessions()
                .write()
                .expect("registry")
                .insert((group.group_id.clone(), (*short).into()), Arc::clone(&item));
            item
        })
        .collect::<Vec<_>>();
    // Forced requests bypass a graceful stop's held owner lock; they do not
    // mark jobs stopped just because the provider accepted the request.
    let held = items[0].stop_lock.lock().expect("held stop lock");
    for phase in [0, 2] {
        mode.store(phase, Ordering::Release);
        requests.lock().expect("requests").clear();
        tokio::time::timeout(Duration::from_secs(2), kill_all_requests())
            .await
            .expect("bounded requests");
        let mut actual = requests.lock().expect("requests").clone();
        actual.sort();
        assert_eq!(actual, short_ids);
        assert_eq!(sessions().read().expect("registry").len(), 3);
        assert!(
            items
                .iter()
                .all(|item| !item.stopped.load(Ordering::Acquire))
        );
    }
    drop(held);
    mode.store(1, Ordering::Release);
    let error = tokio::task::spawn_blocking(stop_all)
        .await
        .expect("stop task")
        .expect_err("one cleanup failure");
    assert!(error.to_string().contains("abcdef02"));
    assert!(
        parallel.load(Ordering::Acquire),
        "all stop requests must arrive before the first job finishes"
    );
    assert_eq!(
        sessions().read().expect("registry").len(),
        1,
        "only the failed stop retains ownership"
    );
    assert!(!items[1].stopped.load(Ordering::Acquire));
    let mut switched_group = group.clone();
    let mut switched_actor = cccc_contracts::Actor::new("abcdef02");
    switched_actor.runtime = cccc_contracts::ActorRuntime::Custom;
    switched_actor.command = vec!["sleep".into(), "60".into()];
    switched_group.actors.push(switched_actor);
    store
        .mutate(&group.group_id, |doc| {
            doc.actors.clone_from(&switched_group.actors);
            Ok(())
        })
        .expect("save new backend config");
    struct TerminalCleanup(String);
    impl Drop for TerminalCleanup {
        fn drop(&mut self) {
            let _ = cccc_runtime::stop(&self.0, "abcdef02");
        }
    }
    let _terminal_cleanup = TerminalCleanup(group.group_id.clone());
    for attached in [false, true] {
        let terminal_pid = if attached {
            let terminal = cccc_runtime::start(cccc_runtime::LaunchSpec {
                group_id: group.group_id.clone(),
                actor_id: "abcdef02".into(),
                runner: cccc_contracts::RunnerKind::Pty,
                command: vec!["sleep".into(), "60".into()],
                cwd: config.clone(),
                env: Default::default(),
                cols: 80,
                rows: 24,
            })
            .expect("retained managed terminal");
            items[1].attach_terminal(terminal.pid);
            terminal.pid
        } else {
            None
        };
        assert!(!running(&group.group_id, "abcdef02"));
        for op in ["actor_start", "actor_restart", "actor_stop"] {
            let (home, group_id) = (home.clone(), group.group_id.clone());
            let response = tokio::task::spawn_blocking(move || {
                crate::handle_request(
                    &home,
                    &cccc_contracts::DaemonRequest {
                        v: 1,
                        op: op.into(),
                        args: json!({"group_id":group_id,"actor_id":"abcdef02","by":"user"})
                            .as_object()
                            .expect("args")
                            .clone(),
                    },
                )
            })
            .await
            .expect("lifecycle task");
            let error = response
                .error
                .expect("unconfirmed cleanup blocks backend switch");
            assert_eq!(error.details.get("lifecycle_stage"), Some(&json!("stop")));
            assert!(
                sessions()
                    .read()
                    .expect("registry")
                    .contains_key(&(group.group_id.clone(), "abcdef02".into()))
            );
            assert_eq!(
                cccc_runtime::status(&group.group_id, "abcdef02")
                    .is_ok_and(|status| status.running),
                attached,
                "a failed cleanup preserves the original terminal without starting another"
            );
            assert_eq!(
                cccc_runtime::status(&group.group_id, "abcdef02")
                    .ok()
                    .and_then(|status| status.pid),
                terminal_pid
            );
        }
    }
    std::fs::remove_dir(&blocked_cleanup).expect("unblock cleanup");
    let group_id = group.group_id.clone();
    tokio::task::spawn_blocking(move || stop(&group_id, "abcdef02"))
        .await
        .expect("retry task")
        .expect("retry stop");
    assert!(sessions().read().expect("registry").is_empty());
    assert!(!cccc_runtime::status(&group.group_id, "abcdef02").is_ok_and(|status| status.running));
    assert!(
        items
            .iter()
            .all(|item| item.stopped.load(Ordering::Acquire))
    );
    server.abort();
    let _ = server.await;
}
