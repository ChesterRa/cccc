use super::*;
use cccc_client::DaemonClient;
use futures_util::{SinkExt, StreamExt};
use std::path::Path;
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resident_session_serializes_tasks_and_quarantines_uncertain_admission() {
    // Re-execution isolates the process registries and launcher environment.
    const KEY: &str = "CCCC_SECRETARY_OFFLINE_OWNER_TEST";
    if std::env::var_os(KEY).is_none() {
        let temp = tempfile::tempdir().expect("launcher");
        let launcher = temp.path().join("cccc");
        std::fs::write(&launcher, "#!/bin/sh\nexit 0\n").expect("launcher");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).expect("mode");
        let result=std::process::Command::new(std::env::current_exe().expect("test exe"))
            .args(["--exact","ops::voice_secretary::tests::fixture_tests::resident_session_serializes_tasks_and_quarantines_uncertain_admission","--nocapture"])
            .env(KEY,"1").env("CCCC_LAUNCHER_PATH",launcher).output().expect("fixture process");
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let events = temp.path().join("events");
    std::fs::create_dir(&events).expect("events");
    let provider = temp.path().join("provider");
    std::fs::create_dir(&provider).expect("provider state");
    let executable = temp.path().join("codex");
    std::fs::write(&executable,format!("#!/bin/sh\ncase \" $* \" in *' --remote '*) stty raw -echo; printf 'SECRETARY NATIVE VIEWER\\n'; touch ../viewer-started; cat; exit 0;; esac\nexec {} --ignored --exact ops::voice_secretary::tests::fixture_tests::offline_provider_child --nocapture\n",
        shell_quote(&std::env::current_exe().expect("test exe").to_string_lossy()))).expect("wrapper");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let store = GroupStore::new(home.clone()).expect("groups");
    let a = store.create("A", "").expect("A").group_id;
    let b = store.create("B", "").expect("B").group_id;
    for (group, text) in [(&a, "# A\nAlice owes 42.\n"), (&b, "# B\nBob owes 75.\n")] {
        request(
            &home,
            "assistant_voice_document_save",
            json!({"group_id":group,"document_path":"notes.md","content":text}),
        )
        .expect("document");
    }
    let profiles = ProfileStore::new(home.clone()).expect("profiles");
    let profile = profiles
        .upsert(
            json!({"name":"Offline secretary","runtime":"codex","command":[executable]})
                .as_object()
                .expect("profile")
                .clone(),
            None,
        )
        .expect("profile");
    let environment =
        json!({"CODEX_HOME":provider,"FIXTURE_HOME":home.root(),"FIXTURE_EVENTS":events});
    profiles
        .update_secrets(
            profile["id"].as_str().expect("id"),
            environment.as_object().expect("env"),
            &[],
            false,
        )
        .expect("isolated env");
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
    let call = |group: &str, text: &str, key: &str, path: &str| DaemonRequest {
        v: 1,
        op: "assistant_voice_document_instruction".into(),
        args:
            json!({"group_id":group,"instruction":text,"input_append_id":key,"document_path":path})
                .as_object()
                .expect("args")
                .clone(),
    };
    let hold = client
        .call(&call(&a, "HOLD admission", "hold", "notes.md"))
        .await
        .expect("hold");
    assert!(hold.ok);
    let hold_id = hold.result["secretary_task_id"]
        .as_str()
        .expect("id")
        .to_owned();
    wait_until(|| events.join(&hold_id).exists()).await;
    let active = jobs.load(&hold_id).expect("active task");
    let grant = jobs.session_grant_file();
    assert!(!grant.starts_with(jobs.runtime_workspace()));
    assert_eq!(
        std::fs::metadata(&grant)
            .expect("isolated fixture invariant")
            .permissions()
            .mode()
            & 0o077,
        0
    );

    let reset_request = |generation: &str| DaemonRequest {
        v: 1,
        op: "voice_secretary_runtime_reset".into(),
        args: json!({"generation":generation})
            .as_object()
            .expect("isolated fixture invariant")
            .clone(),
    };
    let rejected = client
        .call(&reset_request(&active.generation))
        .await
        .expect("isolated fixture invariant");
    assert!(!rejected.ok, "reset must not interrupt an active task");
    let view_request = |group: &str, generation: &str, by: &str| {
        DaemonRequest {
        v: 1,
        op: "voice_secretary_terminal_attach".into(),
        args: json!({"group_id":group,"task_id":hold_id,"generation":generation,"by":by,"bootstrap":"snapshot_v1","cols":80,"rows":20})
            .as_object().expect("terminal args").clone(),
    }
    };
    let viewer_marker = jobs
        .runtime_workspace()
        .parent()
        .expect("secretary")
        .join("viewer-started");
    let status_request = DaemonRequest {
        v: 1,
        op: "voice_secretary_tasks".into(),
        args: json!({"group_id":a}).as_object().expect("args").clone(),
    };
    let status = client.call(&status_request).await.expect("status");
    assert_eq!(
        status.result["tasks"][0]["execution"]["native_terminal"],
        true
    );
    assert_eq!(
        status.result["tasks"][0]["execution"]["generation"],
        active.generation
    );
    assert!(
        !viewer_marker.exists(),
        "status polling must not open a terminal"
    );
    for denied in [
        view_request(&a, "stale-generation", "user"),
        view_request(&a, &active.generation, "peer"),
    ] {
        let (response, _stream) = client.upgrade(&denied).await.expect("denied attachment");
        assert!(!response.ok);
    }
    let (attached, mut terminal_stream) = client
        .upgrade(&view_request(&a, &active.generation, "user"))
        .await
        .expect("terminal upgrade");
    assert!(attached.ok, "{:?}", attached.error);
    assert_eq!(attached.result["terminal_mode"], "control");
    assert_eq!(attached.result["terminal_writable"], true);
    wait_until(|| viewer_marker.exists()).await;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut output = [0; 1024];
        let mut received = Vec::new();
        loop {
            let bytes = terminal_stream
                .read(&mut output)
                .await
                .expect("viewer output");
            assert!(bytes > 0, "native viewer closed before its output");
            received.extend_from_slice(&output[..bytes]);
            if String::from_utf8_lossy(&received).contains("SECRETARY NATIVE VIEWER") {
                break;
            }
        }
    })
    .await
    .expect("viewer output deadline");
    terminal_stream
        .write_all(b"native keys\r")
        .await
        .expect("native input");
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut bytes = [0; 1024];
        let mut received = Vec::new();
        while !String::from_utf8_lossy(&received).contains("native keys") {
            let count = terminal_stream.read(&mut bytes).await.expect("echo");
            assert!(count > 0);
            received.extend_from_slice(&bytes[..count]);
        }
    })
    .await
    .expect("native input reaches the PTY");
    let resized = client.call(&DaemonRequest { v:1, op:"voice_secretary_terminal_resize".into(), args:json!({"group_id":a,"task_id":hold_id,"generation":active.generation,"attachment_id":attached.result["attachment_id"],"cols":50,"rows":12}).as_object().expect("resize args").clone() }).await.expect("resize");
    assert!(resized.ok, "{:?}", resized.error);
    drop(terminal_stream);
    assert!(
        jobs.load(&hold_id).expect("still active").phase.executing(),
        "closing a viewer must not cancel its task"
    );
    let ask = client
        .call(&call(&b, "B question", "ask", ""))
        .await
        .expect("ask");
    assert!(ask.ok);
    let ask_id = ask.result["secretary_task_id"]
        .as_str()
        .expect("id")
        .to_owned();
    assert_eq!(
        jobs.load(&ask_id).expect("queued Ask").phase,
        SecretaryTaskPhase::Queued,
        "all task kinds share one execution position"
    );
    let next = client
        .call(&call(&b, "B source", "doc-b", "notes.md"))
        .await
        .expect("B doc");
    assert!(next.ok);
    let next_id = next.result["secretary_task_id"]
        .as_str()
        .expect("id")
        .to_owned();
    assert_eq!(
        jobs.load(&next_id).expect("queued B").phase,
        SecretaryTaskPhase::Queued
    );
    let cancel = client
        .call(&DaemonRequest {
            v: 1,
            op: "voice_secretary_task_cancel".into(),
            args: json!({"group_id":a,"task_id":hold_id})
                .as_object()
                .expect("args")
                .clone(),
        })
        .await
        .expect("cancel");
    assert!(cancel.ok);
    wait_until(|| {
        jobs.load(&hold_id)
            .is_ok_and(|t| t.phase == SecretaryTaskPhase::Cancelled && t.cleanup_confirmed)
    })
    .await;
    assert!(
        !cccc_runtime::status("voice-secretary-terminal", &active.generation)
            .is_ok_and(|status| status.running),
        "task cancellation must clean up its native viewer"
    );
    let (ended, _stream) = client
        .upgrade(&view_request(&a, &active.generation, "user"))
        .await
        .expect("ended task");
    assert!(!ended.ok, "a stale viewer cannot restart a completed task");
    wait_until(|| {
        jobs.load(&next_id)
            .is_ok_and(|t| t.phase == SecretaryTaskPhase::Done && t.cleanup_confirmed)
    })
    .await;
    wait_until(|| {
        jobs.load(&ask_id)
            .is_ok_and(|t| t.phase == SecretaryTaskPhase::Done && t.cleanup_confirmed)
    })
    .await;
    let again = client
        .call(&call(&a, "A follow-up", "doc-a", "notes.md"))
        .await
        .expect("A again");
    assert!(again.ok);
    let again_id = again.result["secretary_task_id"]
        .as_str()
        .expect("id")
        .to_owned();
    wait_until(|| {
        jobs.load(&again_id)
            .is_ok_and(|t| t.phase == SecretaryTaskPhase::Done && t.cleanup_confirmed)
    })
    .await;
    for id in [&hold_id, &next_id, &again_id] {
        let record: Value = cccc_core::fs::read_json(&events.join(id)).expect("safe receipt");
        assert_eq!(
            record["group_id"],
            jobs.load(id).expect("job").target.group_id
        );
        assert!(
            record["cwd"]
                .as_str()
                .expect("cwd")
                .ends_with("voice-secretary/workspace")
        );
    }
    assert_eq!(
        jobs.load(&next_id)
            .expect("isolated fixture invariant")
            .generation,
        jobs.load(&again_id)
            .expect("isolated fixture invariant")
            .generation,
        "A and B tasks reuse the same resident process/session"
    );
    let idle = resident::status(
        &home,
        &DaemonRequest {
            v: 1,
            op: "voice_secretary_runtime".into(),
            args: serde_json::Map::new(),
        },
    )
    .expect("resident status");
    assert_eq!(idle["phase"], "ready");
    let generation = jobs
        .load(&again_id)
        .expect("isolated fixture invariant")
        .generation;
    let (idle_attached, stream) = client
        .upgrade(&view_request(&a, &generation, "user"))
        .await
        .expect("idle viewer");
    assert!(idle_attached.ok, "idle Secretary keeps its native terminal");
    drop(stream);
    // The runtime owns native/manual turns even with the viewer detached.
    let owner = lookup(&home).expect("owner");
    let resident = owner.resident().expect("resident");
    let emit = |method: &str, turn: &str, thread: &str| {
        resident
            .session
            .publish_event_for_test(json!({"method":method,
            "params":{"threadId":thread,"turn":{"id":turn}}}));
    };
    let status_request = DaemonRequest {
        v: 1,
        op: "voice_secretary_runtime".into(),
        args: Default::default(),
    };
    emit("turn/started", "manual", resident.session.thread_id());
    wait_until(|| resident.busy()).await;
    let manual = resident::status(&home, &status_request).expect("manual status");
    assert_eq!(manual["phase"], "working");
    assert_eq!(manual["manual_turn"], true);
    assert!(
        manual["task"].is_null(),
        "manual input has no borrowed Group target"
    );
    assert!(
        !client
            .call(&reset_request(&generation))
            .await
            .expect("busy reset")
            .ok
    );
    let queued = client
        .call(&call(&b, "After manual turn", "after-manual", ""))
        .await
        .expect("queued task");
    let queued_id = queued.result["secretary_task_id"].as_str().expect("id");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        jobs.load(queued_id).expect("waiting").phase,
        SecretaryTaskPhase::Queued
    );
    emit("turn/completed", "unrelated", resident.session.thread_id());
    emit("turn/completed", "manual", "other-thread");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(
        resident.busy(),
        "unrelated completions do not release the native turn"
    );
    emit("turn/completed", "manual", resident.session.thread_id());
    wait_until(|| {
        jobs.load(queued_id)
            .is_ok_and(|t| t.phase == SecretaryTaskPhase::Done && t.cleanup_confirmed)
    })
    .await;
    assert_eq!(
        jobs.load(queued_id).expect("completed").generation,
        generation
    );
    wait_until(|| !resident.busy()).await;
    let original_a = jobs
        .load(&again_id)
        .expect("A")
        .document_file
        .expect("A file");
    let original_b = jobs
        .load(&next_id)
        .expect("B")
        .document_file
        .expect("B file");
    let text_a = std::fs::read_to_string(original_a).expect("A content");
    let text_b = std::fs::read_to_string(original_b).expect("B content");
    assert!(text_a.contains("Alice owes 42") && !text_a.contains("Bob owes 75"));
    assert!(text_b.contains("Bob owes 75") && !text_b.contains("Alice owes 42"));
    let stale = client
        .call(&reset_request("old-generation"))
        .await
        .expect("isolated fixture invariant");
    assert!(!stale.ok, "stale reset cannot stop the replacement session");
    assert!(
        client
            .call(&reset_request(&generation))
            .await
            .expect("isolated fixture invariant")
            .ok
    );
    wait_until(|| {
        lookup(&home)
            .expect("isolated fixture invariant")
            .resident()
            .is_none()
    })
    .await;
    for _ in 0..3 {
        let status = resident::status(
            &home,
            &DaemonRequest {
                v: 1,
                op: "voice_secretary_runtime".into(),
                args: serde_json::Map::new(),
            },
        )
        .expect("isolated fixture invariant");
        assert_eq!(
            status["phase"], "not_started",
            "reads must not relaunch the provider"
        );
    }
    assert!(
        !jobs.session_grant_file().exists(),
        "reset revokes the idle session grant"
    );
    let uncertain = client
        .call(&call(&a, "DISCONNECT after receiving", "uncertain", ""))
        .await
        .expect("uncertain");
    assert!(uncertain.ok);
    let uncertain_id = uncertain.result["secretary_task_id"]
        .as_str()
        .expect("id")
        .to_owned();
    wait_until(|| {
        jobs.load(&uncertain_id)
            .is_ok_and(|t| t.phase == SecretaryTaskPhase::Unconfirmed && t.cleanup_confirmed)
    })
    .await;
    let received_before_repeat = std::fs::read_dir(&events)
        .expect("received prompts")
        .count();
    let repeated = client
        .call(&call(&a, "DISCONNECT after receiving", "uncertain", ""))
        .await
        .expect("repeat");
    assert!(repeated.ok);
    assert_eq!(repeated.result["secretary_task_id"], uncertain_id);
    assert_eq!(
        std::fs::read_dir(&events).expect("launches").count(),
        received_before_repeat,
        "same uncertain source must not launch a replacement provider"
    );
    let prompt=client.call(&DaemonRequest {v:1,op:"assistant_voice_input_append".into(),args:json!({"group_id":b,"kind":"prompt_refine","request_id":"prompt-fixture","input_append_id":"prompt-source","composer_text":"Improve the proposal","composer_snapshot_hash":"composer-fixture"}).as_object().expect("args").clone()}).await.expect("prompt");
    assert!(prompt.ok);
    let prompt_id = prompt.result["secretary_task_id"]
        .as_str()
        .expect("prompt task");
    wait_until(|| {
        jobs.load(prompt_id).is_ok_and(|task| {
            task.phase == SecretaryTaskPhase::Done
                && task.cleanup_confirmed
                && !task.projected_at.is_empty()
        })
    })
    .await;
    let state = cccc_core::assistant_state::load(&home, &b).expect("prompt state");
    assert_eq!(
        state["prompt_draft"]["draft_text"],
        "Polished fixture input"
    );
    let clarification = client
        .call(&call(&b, "NEEDS_USER fixture", "clarification", ""))
        .await
        .expect("clarification");
    assert!(clarification.ok);
    let clarification_id = clarification.result["secretary_task_id"]
        .as_str()
        .expect("clarification id");
    wait_until(|| {
        jobs.load(clarification_id).is_ok_and(|task| {
            task.phase == SecretaryTaskPhase::NeedsUser
                && task.cleanup_confirmed
                && !task.projected_at.is_empty()
        })
    })
    .await;
    let continued = client
        .call(&DaemonRequest {
            v: 1,
            op: "voice_secretary_task_retry".into(),
            args:
                json!({"group_id":b,"task_id":clarification_id,"followup":"The requested detail"})
                    .as_object()
                    .expect("args")
                    .clone(),
        })
        .await
        .expect("continue");
    assert!(continued.ok);
    let continued_id = continued.result["task"]["task_id"]
        .as_str()
        .expect("successor");
    assert_ne!(continued_id, clarification_id);
    wait_until(|| {
        jobs.load(continued_id)
            .is_ok_and(|task| task.phase == SecretaryTaskPhase::Done && task.cleanup_confirmed)
    })
    .await;
    assert!(store.load(&a).expect("A").actors.is_empty());
    assert!(store.load(&b).expect("B").actors.is_empty());
    let held = client
        .call(&call(
            &a,
            "HOLD shutdown admission",
            "shutdown-held",
            "notes.md",
        ))
        .await
        .expect("held during shutdown");
    assert!(held.ok);
    let held_id = held.result["secretary_task_id"].as_str().expect("held id");
    wait_until(|| events.join(held_id).exists()).await;
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
        .expect("task")
        .expect("daemon");
    let stopped = jobs.load(held_id).expect("durable shutdown outcome");
    assert_eq!(stopped.phase, SecretaryTaskPhase::Cancelled);
    assert!(stopped.cleanup_confirmed);
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "offline secretary condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
#[ignore = "isolated protocol fixture subprocess, never a provider model task"]
async fn offline_provider_child() {
    let Some(root) = std::env::var_os("FIXTURE_HOME") else {
        return;
    };
    let home = HomeLayout::from_path(root).expect("fixture home");
    let profile = std::env::var("CCCC_SECRETARY_PERMISSION_PROFILE").expect("profile");
    let token = std::env::var("CCCC_SECRETARY_TASK_TOKEN").expect("task grant");
    let client = DaemonClient::new(home);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    println!(
        "listening on: ws://{}",
        listener.local_addr().expect("address")
    );
    let (socket, _) = listener.accept().await.expect("connection");
    let mut socket = accept_async(socket).await.expect("websocket");
    let thread = uuid::Uuid::new_v4().simple().to_string();
    let mut previous_task = String::new();
    while let Some(Ok(Message::Text(frame))) = socket.next().await {
        let message: Value = serde_json::from_str(&frame).expect("request");
        let id = message["id"].clone();
        let method = message["method"].as_str().unwrap_or("");
        let result = match method {
            "initialize" | "thread/name/set" | "turn/interrupt" => json!({}),
            "config/read" => json!({"config":{"mcp_servers":{profile.clone():{},"unrelated":{}}}}),
            "skills/list" => json!({"data":[]}),
            "thread/start" => {
                assert!(message["params"].get("sandbox").is_none());
                assert_eq!(message["params"]["permissions"], profile);
                assert_eq!(
                    message["params"]["config"]["mcp_servers.unrelated.enabled"],
                    false
                );
                json!({"thread":{"id":thread},"activePermissionProfile":{"id":profile},"sandbox":{"type":"workspaceWrite","networkAccess":false},"runtimeWorkspaceRoots":[std::env::current_dir().expect("cwd")]})
            }
            "thread/read" => json!({"thread":{"id":thread,"turns":[]}}),
            "turn/start" => {
                let prompt = message["params"]["input"][0]["text"]
                    .as_str()
                    .expect("prompt");
                let task_id = prompt
                    .strip_prefix("TASK_ID: ")
                    .expect("task prefix")
                    .lines()
                    .next()
                    .expect("id");
                if !previous_task.is_empty() {
                    let stale = client.call(&DaemonRequest { v:1, op:"voice_secretary_task".into(),
                        args:json!({"_cccc_secretary_token":token,"task_id":previous_task,"action":"report","status":"done","reply_text":"late old output"}).as_object().expect("isolated fixture invariant").clone() }).await.expect("late call");
                    assert!(!stale.ok, "old task must not act on the replacement");
                }
                previous_task = task_id.into();
                let context =
                    task_call(&client, &token, task_id, json!({"action":"context"})).await;
                let task_id = context["task_id"].as_str().expect("task id");
                let events = std::env::var_os("FIXTURE_EVENTS").expect("events");
                cccc_core::fs::write_json(&Path::new(&events).join(task_id),&json!({"task_id":task_id,"group_id":context["target"]["group_id"],"cwd":std::env::current_dir().expect("cwd"),"pid":std::process::id()})).expect("safe receipt");
                let text = context["inputs"][0]["text"].as_str().unwrap_or("");
                if text.contains("HOLD") {
                    continue;
                }
                if text.contains("DISCONNECT") {
                    break;
                }
                let turn = uuid::Uuid::new_v4().simple().to_string();
                socket
                    .send(Message::Text(
                        json!({"id":id,"result":{"turn":{"id":turn}}})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .expect("admission");
                socket.send(Message::Text(json!({"method":"turn/started","params":{"threadId":thread,"turn":{"id":turn,"status":"inProgress"}}}).to_string().into())).await.expect("activity");
                if text.contains("NEEDS_USER")
                    && context["followup"].as_str().is_none_or(str::is_empty)
                {
                    task_call(&client,&token,task_id,json!({"action":"report","status":"needs_user","reply_text":"Which detail?"})).await;
                } else {
                    match context["target"]["kind"].as_str().expect("kind") {
                        "document" => {
                            let file = std::env::current_dir()
                                .expect("cwd")
                                .join(context["working_document"].as_str().expect("working file"));
                            let mut content = std::fs::read_to_string(&file).expect("work copy");
                            content.push_str(text);
                            content.push('\n');
                            std::fs::write(&file, content).expect("incremental edit");
                            task_call(
                                &client,
                                &token,
                                task_id,
                                json!({"action":"commit","base_version":context["base_version"]}),
                            )
                            .await;
                        }
                        "prompt" => {
                            task_call(
                                &client,
                                &token,
                                task_id,
                                json!({"action":"draft","draft_text":"Polished fixture input"}),
                            )
                            .await;
                        }
                        _ => {
                            task_call(&client,&token,task_id,json!({"action":"report","status":"done","reply_text":"Offline fixture answer"})).await;
                        }
                    }
                }
                socket.send(Message::Text(json!({"method":"turn/completed","params":{"threadId":thread,"turn":{"id":turn,"status":"completed"}}}).to_string().into())).await.expect("completion");
                continue;
            }
            _ => panic!("unsupported fixture request: {method}"),
        };
        socket
            .send(Message::Text(
                json!({"id":id,"result":result}).to_string().into(),
            ))
            .await
            .expect("response");
    }
}

async fn task_call(client: &DaemonClient, token: &str, task_id: &str, args: Value) -> Value {
    let mut args = args.as_object().expect("args").clone();
    args.insert("_cccc_secretary_token".into(), json!(token));
    args.insert("task_id".into(), json!(task_id));
    let reply = client
        .call(&DaemonRequest {
            v: 1,
            op: "voice_secretary_task".into(),
            args,
        })
        .await
        .expect("task IPC");
    assert!(
        reply.ok,
        "task action rejected: {}",
        reply
            .error
            .as_ref()
            .map_or("", |error| error.message.as_str())
    );
    json!(reply.result)
}
