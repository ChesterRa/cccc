use super::*;
use cccc_client::DaemonClient;
use cccc_contracts::RuntimeMode;
use std::os::unix::fs::PermissionsExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_structured_adapters_run_fixed_tasks_without_analyst_authority() {
    const CHILD: &str = "CCCC_SECRETARY_STRUCTURED_FIXTURE";
    if std::env::var_os(CHILD).is_none() {
        let temp = tempfile::tempdir().expect("launcher");
        let launcher = temp.path().join("cccc");
        std::fs::write(&launcher, "#!/bin/sh\nexit 0\n").expect("launcher");
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).expect("mode");
        for runtime in [
            "antigravity",
            "copilot",
            "devin",
            "cursor",
            "claude",
            "grok",
            "opencode",
            "kilo",
        ] {
            let output = std::process::Command::new(std::env::current_exe().expect("test"))
            .args(["--exact", "ops::voice_secretary::tests::structured_fixture_tests::all_structured_adapters_run_fixed_tasks_without_analyst_authority", "--nocapture"])
            .env(CHILD, runtime).env("CCCC_LAUNCHER_PATH", &launcher)
            .output().expect("isolated fixture");
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    }
    for (runtime, program) in [
        (ActorRuntime::Antigravity, "agy"),
        (ActorRuntime::Copilot, "copilot"),
        (ActorRuntime::Devin, "devin"),
        (ActorRuntime::Cursor, "cursor-agent"),
        (ActorRuntime::Claude, "claude"),
        (ActorRuntime::Grok, "grok"),
        (ActorRuntime::Opencode, "opencode"),
        (ActorRuntime::Kilo, "kilo"),
    ] {
        if serde_json::to_value(runtime).expect("runtime").as_str()
            != std::env::var(CHILD).ok().as_deref()
        {
            continue;
        }
        let temp = tempfile::Builder::new()
            .prefix("cccc-s-")
            .tempdir_in("/tmp")
            .expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
        home.initialize().expect("init");
        let executable = temp.path().join(program);
        let script = include_str!("structured_fixture.py");
        std::fs::write(&executable, script).expect("provider fixture");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("mode");
        if runtime == ActorRuntime::Antigravity {
            crate::antigravity_acp_setup::install_fixture(&home, script);
        }
        let events = temp.path().join("events");
        std::fs::create_dir(&events).expect("events");
        let provider = temp.path().join("provider");
        std::fs::create_dir(&provider).expect("provider");
        let mode = if runtime.supports_acp_mode() {
            RuntimeMode::Acp
        } else {
            RuntimeMode::Default
        };
        let command = if runtime == ActorRuntime::Antigravity {
            vec!["agy".to_owned()]
        } else {
            vec![executable.to_string_lossy().into_owned()]
        };
        let profiles = ProfileStore::new(home.clone()).expect("profiles");
        let profile=profiles.upsert(json!({"name":"Offline structured Secretary","runtime":runtime,"runtime_mode":mode,"command":command}).as_object().expect("profile").clone(),None).expect("profile");
        let environment = json!({"HOME":provider,"CODEX_HOME":provider.join("codex"),"CLAUDE_CONFIG_DIR":provider.join("claude"),"GROK_HOME":provider.join("grok"),"XDG_CONFIG_HOME":provider.join("config"),"XDG_DATA_HOME":provider.join("data"),"FIXTURE_HOME":home.root(),"FIXTURE_EVENTS":events,"SECRETARY_FIXTURE_RUNTIME":serde_json::to_value(runtime).expect("runtime")});
        profiles
            .update_secrets(
                profile["id"].as_str().expect("id"),
                environment.as_object().expect("env"),
                &[],
                false,
            )
            .expect("isolated environment");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let a = groups.create("A", "").expect("A").group_id;
        let b = groups.create("B", "").expect("B").group_id;
        for (group, text) in [(&a, "# Alice 42\n"), (&b, "# Bob 75\n")] {
            request(
                &home,
                "assistant_voice_document_save",
                json!({"group_id":group,"document_path":"notes.md","content":text}),
            )
            .expect("document");
        }
        // An unrelated Analyst receipt must survive every Secretary session.
        let analyst_path = home.daemon_dir().join("codex_voice_analyst.json");
        let analyst_receipt = json!({"thread_id":"unrelated-analyst","materialized":false});
        cccc_core::fs::write_json(&analyst_path, &analyst_receipt).expect("Analyst baseline");
        request(
            &home,
            "voice_secretary_settings_update",
            json!({"settings":{"profile_id":profile["id"]}}),
        )
        .expect("configure");
        let daemon = tokio::spawn(crate::server::run(home.clone()));
        wait_until(|| crate::DaemonPaths::new(home.clone()).address.exists()).await;
        let client = DaemonClient::new(home.clone());
        let jobs = SecretaryTaskStore::new(home.clone());
        for (index, (group, text, path, kind)) in [
            (&a, "A source", "notes.md", "document"),
            (&b, "B question", "", "ask"),
            (&b, "B source", "notes.md", "document"),
            (&a, "A return", "notes.md", "document"),
            (&a, "Improve this", "", "prompt"),
            (&a, "HOLD pending admission", "", "ask"),
            (&a, "DISCONNECT uncertain", "", "ask"),
        ]
        .into_iter()
        .enumerate()
        {
            let operation = if kind == "prompt" {
                "assistant_voice_input_append"
            } else {
                "assistant_voice_document_instruction"
            };
            let args = if kind == "prompt" {
                json!({"group_id":group,"kind":"prompt_refine","request_id":"fixture-prompt","input_append_id":format!("source-{index}"),"composer_text":text,"composer_snapshot_hash":"fixture-hash"})
            } else {
                json!({"group_id":group,"instruction":text,"document_path":path,"input_append_id":format!("source-{index}")})
            };
            let reply = client
                .call(&DaemonRequest {
                    v: 1,
                    op: operation.into(),
                    args: args.as_object().expect("args").clone(),
                })
                .await
                .expect("source");
            assert!(reply.ok, "{runtime:?}: source rejected");
            let id = reply.result["secretary_task_id"].as_str().expect("task");
            let waiting_since = tokio::time::Instant::now();
            let mut traced = false;
            wait_until(|| {
                if waiting_since.elapsed() > Duration::from_secs(10) && !traced {
                    traced = true;
                    eprintln!(
                        "startup trace: {}",
                        std::fs::read_to_string(events.join("startup-trace.jsonl"))
                            .unwrap_or_default()
                    );
                    if let Ok(t) = jobs.load(id) {
                        eprintln!("task phase {:?}, diagnostic {}", t.phase, t.diagnostic);
                    }
                }
                if let Ok(task) = jobs.load(id) {
                    assert!(
                        task.phase != SecretaryTaskPhase::Failed,
                        "{runtime:?} startup failed: {}",
                        task.diagnostic
                    );
                }
                events.join(id).exists()
            })
            .await;
            if text.contains("HOLD") {
                let cancelled = client
                    .call(&DaemonRequest {
                        v: 1,
                        op: "voice_secretary_task_cancel".into(),
                        args: json!({"group_id":group,"task_id":id})
                            .as_object()
                            .expect("args")
                            .clone(),
                    })
                    .await
                    .expect("cancel");
                assert!(cancelled.ok);
                wait_until(|| {
                    if let Ok(t) = jobs.load(id) {
                        assert!(
                            !matches!(
                                t.phase,
                                SecretaryTaskPhase::Failed | SecretaryTaskPhase::Unconfirmed
                            ),
                            "{runtime:?} task {index} failed as {:?}: {}",
                            t.phase,
                            t.diagnostic
                        );
                    }
                    jobs.load(id).is_ok_and(|t| {
                        t.phase == SecretaryTaskPhase::Cancelled && t.cleanup_confirmed
                    })
                })
                .await;
            } else if text.contains("DISCONNECT") && runtime != ActorRuntime::Claude {
                wait_until(|| {
                    jobs.load(id).is_ok_and(|t| {
                        t.phase == SecretaryTaskPhase::Unconfirmed && t.cleanup_confirmed
                    })
                })
                .await;
                assert!(jobs.successor(id).expect("no replay").is_none());
            } else {
                let waiting_since = tokio::time::Instant::now();
                let mut traced = false;
                wait_until(|| {
                    if waiting_since.elapsed() > Duration::from_secs(10) && !traced {
                        traced = true;
                        eprintln!(
                            "completion trace: {}",
                            std::fs::read_to_string(events.join("startup-trace.jsonl"))
                                .unwrap_or_default()
                        );
                        if let Ok(t) = jobs.load(id) {
                            eprintln!(
                                "task phase {:?}, receipt {:?}, cleanup {}, diagnostic {}",
                                t.phase,
                                t.receipt.as_ref().map(|r| r.status),
                                t.cleanup_confirmed,
                                t.diagnostic
                            );
                        }
                    }
                    jobs.load(id).is_ok_and(|t| {
                        t.phase == SecretaryTaskPhase::Done
                            && t.cleanup_confirmed
                            && !t.projected_at.is_empty()
                    })
                })
                .await;
            }
            let task = jobs.load(id).expect("task");
            assert!(
                !task.isolated_document_writes,
                "trusted native Runtime cannot imply sandbox proof"
            );
            let evidence: Value =
                cccc_core::fs::read_json(&events.join(id)).expect("identity receipt");
            assert_eq!(evidence["group_id"], *group);
            assert!(
                evidence["cwd"]
                    .as_str()
                    .expect("cwd")
                    .ends_with("voice-secretary/workspace")
            );
            assert_eq!(
                cccc_core::fs::read_json::<Value>(&analyst_path).expect("Analyst unchanged"),
                analyst_receipt
            );
        }
        let completed = jobs.list().expect("tasks");
        let sessions: std::collections::HashSet<_> =
            completed.iter().map(|t| t.thread_id.clone()).collect();
        assert_eq!(
            sessions.len(),
            2,
            "{runtime:?}: normal tasks share one session; cancellation replaces it"
        );
        for group in [&a, &b] {
            assert!(
                groups
                    .load(group)
                    .expect("no secretary Actor")
                    .actors
                    .is_empty()
            );
            let file = completed
                .iter()
                .find(|t| t.target.group_id == *group && t.document_file.is_some())
                .expect("doc task")
                .document_file
                .as_ref()
                .expect("file");
            let content = std::fs::read_to_string(file).expect("document");
            assert!(content.contains(if group == &a { "Alice 42" } else { "Bob 75" }));
            assert!(!content.contains(if group == &a { "Bob 75" } else { "Alice 42" }));
        }
        if runtime == ActorRuntime::Claude {
            profiles
                .update_secrets(
                    profile["id"].as_str().expect("id"),
                    json!({"SECRETARY_FIXTURE_REJECT_LAUNCH":"1"})
                        .as_object()
                        .expect("patch"),
                    &[],
                    false,
                )
                .expect("reject fixture launch");
            let result=client.call(&DaemonRequest{v:1,op:"assistant_voice_document_instruction".into(),args:json!({"group_id":a,"instruction":"Rejected launch fixture","input_append_id":"rejected-launch"}).as_object().expect("args").clone()}).await.expect("accepted source");
            assert!(result.ok);
            let id = result.result["secretary_task_id"].as_str().expect("task");
            wait_until(|| {
                jobs.load(id)
                    .is_ok_and(|t| t.phase == SecretaryTaskPhase::Failed && t.cleanup_confirmed)
            })
            .await;
        }
        client
            .call(&DaemonRequest {
                v: 1,
                op: "shutdown".into(),
                args: serde_json::Map::new(),
            })
            .await
            .expect("shutdown");
        tokio::time::timeout(Duration::from_secs(10), daemon)
            .await
            .expect("bounded shutdown")
            .expect("daemon task")
            .expect("daemon");
        if runtime == ActorRuntime::Claude {
            // Fixture control services allow straggling kill/list readers, then exit.
            tokio::time::sleep(Duration::from_millis(700)).await;
            let settings_dir = home.daemon_dir().join("claude-managed");
            assert_eq!(
                std::fs::read_dir(settings_dir)
                    .expect("settings directory")
                    .count(),
                0,
                "task settings are retired after cleanup"
            );
        }
        eprintln!(
            "offline Secretary {runtime:?}: Doc/Ask/Prompt, A-B-A, cancel, isolated receipts passed"
        );
    }
}

pub(super) async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "structured Secretary fixture timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restored_claude_task_requires_native_cleanup_and_preserves_unrelated_jobs() {
    const CHILD: &str = "CCCC_SECRETARY_CLAUDE_RECOVERY_FIXTURE";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test"))
            .args(["--exact", "ops::voice_secretary::tests::structured_fixture_tests::restored_claude_task_requires_native_cleanup_and_preserves_unrelated_jobs", "--nocapture"])
            .env(CHILD, "1").output().expect("fixture child");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::Builder::new()
        .prefix("cccc-s-recovery-")
        .tempdir_in("/tmp")
        .expect("temp");
    let state = temp.path().join("state");
    std::fs::create_dir(&state).expect("state");
    let alias = temp.path().join("cccc");
    std::os::unix::fs::symlink(state, &alias).expect("home alias");
    let home = HomeLayout::from_path(alias).expect("home");
    home.initialize().expect("init");
    let group_id = GroupStore::new(home.clone())
        .expect("groups")
        .create("Recovery", "")
        .expect("group")
        .group_id;
    let store = SecretaryTaskStore::new(home.clone());
    let mut task = SecretaryTask::new(
        cccc_contracts::voice_secretary::SecretaryTaskTarget {
            group_id,
            scope_key: String::new(),
            kind: SecretaryTaskKind::Ask,
            document_path: String::new(),
            request_id: String::new(),
            document_id: String::new(),
            composer_snapshot_hash: String::new(),
        },
        vec![json!({"input_id":"orphan","text":"Recovery fixture"})],
    );
    task.phase = SecretaryTaskPhase::Unconfirmed;
    task.cleanup_confirmed = false;
    store.create(&task).expect("task");
    // Restore a task produced by the preceding per-task process layout.
    std::fs::remove_dir(
        store
            .workspace(&task.task_id)
            .expect("isolated fixture invariant"),
    )
    .expect("isolated fixture invariant");
    let workspace = store
        .directory(&task.task_id)
        .expect("isolated fixture invariant")
        .join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace directory");
    let config_dir = temp.path().join("provider");
    std::fs::create_dir_all(config_dir.join("jobs/abcdef12")).expect("provider");
    cccc_core::fs::write_json(
        &config_dir.join("jobs/abcdef12/state.json"),
        &json!({"cwd":workspace}),
    )
    .expect("durable live job");
    let settings_path = home.daemon_dir().join("claude-managed/fixture.json");
    cccc_core::fs::write_secret_json(&settings_path, &json!({"env":{"CCCC_SECRETARY_TASK_TOKEN":"f".repeat(64),"CCCC_MCP_TOOL_PROFILE":"secretary-task","CLAUDE_CONFIG_DIR":config_dir,"FIXTURE_UNRELATED_JOB":"1"}})).expect("private settings");
    let owner_path = store
        .directory(&task.task_id)
        .expect("directory")
        .join("claude-owner.json");
    cccc_core::fs::write_json(
        &owner_path,
        &json!({"config_dir":config_dir,"settings_path":settings_path}),
    )
    .expect("owner");
    start(&home, None).expect("start without native control");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!store.load(&task.task_id).expect("task").cleanup_confirmed);
    assert!(
        owner_path.exists() && settings_path.exists(),
        "unresolved ownership must remain"
    );
    let executable = temp.path().join("claude");
    std::fs::write(&executable, include_str!("structured_fixture.py")).expect("fixture");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let (mut child, owner) = cccc_runtime::OwnedProcessTree::spawn(
        std::process::Command::new(&executable)
            .args(["--fixture-control", "--settings"])
            .arg(&settings_path)
            .current_dir(&workspace)
            .env("SECRETARY_FIXTURE_RUNTIME", "claude")
            .env("HOME", &config_dir)
            .env("CODEX_HOME", config_dir.join("codex"))
            .env_remove("CCCC_GROUP_ID")
            .env_remove("CCCC_ACTOR_ID"),
    )
    .expect("native control fixture");
    wait_until(|| config_dir.join("fixture-ready").exists()).await;
    wait_until(|| {
        store
            .load(&task.task_id)
            .is_ok_and(|task| task.cleanup_confirmed)
    })
    .await;
    assert!(store.load(&task.task_id).expect("task").cleanup_confirmed);
    assert!(!owner_path.exists() && !settings_path.exists());
    stop(&home).expect("stop");
    wait_until(|| owner.try_wait(|| child.try_wait()).expect("poll").is_some()).await;
}
