use super::*;
use cccc_contracts::{
    Actor, ActorRuntime, DaemonRequest, GroupState, local_session::LocalSessionRuntime,
};
use cccc_core::{
    GroupStore, actors,
    local_sessions::{self as native, NativeHost, NativeSession},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn opencode_metadata_cannot_select_a_source_from_another_backend() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let host = NativeHost::capture(std::process::id()).expect("host");
    let mut context = Context::new("opencode", None).expect("native MCP");
    assert_ne!(context.opencode_server.as_ref(), Some(&host));
    native::write(
        &home,
        NativeSession {
            source_id: "opencode:ses_other_backend".into(),
            runtime: LocalSessionRuntime::Opencode,
            native_session_id: "ses_other_backend".into(),
            host: host.clone(),
            cwd: temp.path().to_string_lossy().into_owned(),
            title: "Synthetic other backend".into(),
            origin: None,
            revision: "other-backend-revision".into(),
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: None,
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: Some(host),
            retired: None,
        },
    )
    .expect("registered source");
    let response = context.handle(&home, &DaemonClient::new(home.clone()), &json!({
        "id":1,"method":"tools/call","params":{
            "name":"cccc_session","arguments":{"action":"send","group_id":"g_fixture","text":"Must not be forwarded"},
            "_meta":{"ai.opencode/sessionID":"ses_other_backend"}
        }
    })).await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"].to_string().contains("captured backend"));
}

#[tokio::test]
async fn session_mutations_revalidate_captured_authority_at_daemon_execution() {
    for runtime in ["claude", "opencode"] {
        for action in ["send", "leave", "direct_send", "conversation"] {
            for transition in ["none", "end", "clear", "generation", "disabled"] {
                let grouped = matches!(action, "send" | "leave");
                if !grouped && matches!(transition, "generation" | "disabled") {
                    continue; // Unbound operations choose their Group inside the global permit.
                }
                let temp = tempfile::tempdir().expect("fixture");
                let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
                let host = NativeHost::capture(std::process::id()).expect("host");
                let session = native::write(
                    &home,
                    NativeSession {
                        source_id: if runtime == "opencode" {
                            "opencode:ses_original".into()
                        } else {
                            format!("claude:{}", host.key())
                        },
                        runtime: if runtime == "opencode" {
                            LocalSessionRuntime::Opencode
                        } else {
                            LocalSessionRuntime::Claude
                        },
                        native_session_id: if runtime == "opencode" {
                            "ses_original".into()
                        } else {
                            "original".into()
                        },
                        host: host.clone(),
                        cwd: temp.path().to_string_lossy().into_owned(),
                        title: "Caller".into(),
                        origin: None,
                        revision: "original-revision".into(),
                        codex_home: None,
                        codex_can_receive: false,
                        codex_program: None,
                        claude_config_dir: None,
                        pi_agent_dir: None,
                        opencode_config_dir: None,
                        opencode_server: if runtime == "opencode" {
                            Some(NativeHost::parent(std::process::id()).expect("native backend"))
                        } else {
                            None
                        },
                        retired: None,
                    },
                )
                .expect("native source");
                let peer_id = "codex:fixture-peer";
                let mut peer = session.clone();
                peer.source_id = peer_id.into();
                peer.runtime = LocalSessionRuntime::Codex;
                peer.native_session_id = "fixture-peer".into();
                peer.codex_home = Some(temp.path().join("codex"));
                native::write_with_codex_readiness(&home, peer, Some(true)).expect("peer source");
                let attached = cccc_daemon::handle_request(
                    &home,
                    &DaemonRequest {
                        v: 1,
                        op: "local_session_receiver_attach".into(),
                        args: json!({"source_id":session.source_id,"connection_id":"fixture"})
                            .as_object()
                            .expect("receiver args")
                            .clone(),
                    },
                );
                assert!(attached.ok);
                let store = GroupStore::new(home.clone()).expect("store");
                let mut group = store.create("Captured membership", "").expect("group");
                for (id, source_id, runtime) in [
                    (
                        "caller",
                        session.source_id.as_str(),
                        if runtime == "opencode" {
                            ActorRuntime::Opencode
                        } else {
                            ActorRuntime::Claude
                        },
                    ),
                    ("peer", peer_id, ActorRuntime::Codex),
                ] {
                    let mut actor = Actor::new(id);
                    actor.runtime = runtime;
                    actor.external_session_id = source_id.into();
                    actors::add(&mut group, actor).expect("Actor");
                }
                group.state = GroupState::Stopped;
                group.running = false;
                group
                    .scopes
                    .push(cccc_core::scope::detect(temp.path()).expect("scope"));
                group.active_scope_key = group.scopes[0].scope_key.clone();
                let mut ids = vec![session.source_id.clone(), peer_id.into()];
                ids.sort();
                group.extra.insert(
                    "local_session_conversation".into(),
                    json!({"direct":true,"source_ids":ids}),
                );
                store.save(&group).expect("group");
                let ledger = store.ledger_path(&group.group_id).expect("ledger");
                let before_groups = store.list().expect("groups").len();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("proxy");
                cccc_core::fs::write_json(&home.daemon_dir().join("ccccd.addr.json"), &json!({"v":1,"transport":"tcp","path":"","host":"127.0.0.1","port":listener.local_addr().expect("proxy address").port(),"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"ts":"2026-10-26T00:00:00Z"})).expect("address");
                let server_home = home.clone();
                let captured_group = group.clone();
                let mut server = tokio::spawn(async move {
                    loop {
                        let (stream, _) = timeout(Duration::from_secs(3), listener.accept())
                            .await
                            .expect("native authority fixture never received the expected request")
                            .expect("request");
                        let mut stream = BufReader::new(stream);
                        let mut line = String::new();
                        timeout(Duration::from_secs(3), stream.read_line(&mut line))
                            .await
                            .expect("native authority fixture received an incomplete request")
                            .expect("JSON line");
                        let request: DaemonRequest =
                            serde_json::from_str(&line).expect("request JSON");
                        let mutation = matches!(
                            request.op.as_str(),
                            "local_session_send"
                                | "local_session_leave"
                                | "local_session_conversation"
                        );
                        if mutation {
                            if grouped {
                                assert_eq!(
                                    request.args["_cccc_native_binding"]["actor_id"],
                                    "caller"
                                );
                                assert_eq!(
                                    request.args["_cccc_native_binding"]["actor_generation"],
                                    actors::generation_identity(&captured_group.actors[0])
                                );
                            } else {
                                assert_eq!(
                                    request.args["_cccc_native_source_binding"]["source_id"],
                                    session.source_id
                                );
                                assert_eq!(
                                    request.args["_cccc_native_source_binding"]["session_revision"],
                                    session.revision
                                );
                            }
                            match transition {
                                "end" => native::retire(
                                    &server_home,
                                    &session.source_id,
                                    &session.native_session_id,
                                    &session.host,
                                    native::RetireReason::SessionEnd,
                                )
                                .expect("end before forwarding"),
                                "clear" => {
                                    let mut changed = session.clone();
                                    changed.native_session_id = "cleared".into();
                                    changed.revision = "cleared-revision".into();
                                    native::write(&server_home, changed)
                                        .expect("clear before forwarding");
                                }
                                "generation" | "disabled" => {
                                    GroupStore::new(server_home.clone())
                                        .expect("membership store")
                                        .mutate(&captured_group.group_id, |group| {
                                            if transition == "generation" {
                                                group.actors[0].generation =
                                                    "replacement-generation".into();
                                            } else {
                                                group.actors[0].enabled = false;
                                            }
                                            Ok(())
                                        })
                                        .expect("membership changed before forwarding");
                                }
                                _ => {}
                            }
                        }
                        let response = cccc_daemon::handle_request(&server_home, &request);
                        stream
                            .get_mut()
                            .write_all(
                                format!(
                                    "{}\n",
                                    serde_json::to_string(&response).expect("response JSON")
                                )
                                .as_bytes(),
                            )
                            .await
                            .expect("response");
                        if mutation {
                            break;
                        }
                    }
                });
                let mut context = Context::new(runtime, Some(host.pid)).expect("context");
                let mut arguments = json!({"action":if action=="direct_send" {"send"} else {action},
                "source_id":"codex:forged", "_cccc_native_source_binding":{"source_id":"codex:forged","session_revision":"forged"}});
                if matches!(action, "send" | "direct_send") {
                    arguments["text"] = json!("Synthetic captured mutation");
                    arguments["mode"] = json!("mail");
                    arguments["insight"] = json!("Synthetic authority regression");
                }
                if grouped {
                    arguments["group_id"] = json!(group.group_id);
                    if action == "send" {
                        arguments["to"] = json!(["peer"]);
                    }
                } else {
                    arguments["source_ids"] = json!([peer_id]);
                }
                let client = DaemonClient::new(home.clone());
                let message = json!({"id":1,"method":"tools/call","params":{"name":"cccc_session","arguments":arguments,"_meta":if runtime=="opencode" {json!({"ai.opencode/sessionID":"ses_original"})} else {Value::Null}}});
                let response = timeout(
                    Duration::from_secs(5),
                    context.handle(&home, &client, &message),
                )
                .await;
                let completed = timeout(Duration::from_secs(3), &mut server).await;
                if completed.is_err() {
                    // Dropping a JoinHandle does not cancel its task. Retire it
                    // before the temporary Home disappears, including on failure.
                    server.abort();
                    let _ = server.await;
                }
                let response = response.expect("bounded native authority MCP call");
                completed
                    .expect("bounded native authority proxy completion")
                    .expect("proxy completion");
                let events = cccc_core::ledger::read_all(&ledger).expect("ledger");
                if transition == "none" {
                    assert_ne!(response["result"]["isError"], true, "{action}: {response}");
                } else {
                    assert_eq!(
                        response["result"]["isError"], true,
                        "{action}/{transition}: {response}"
                    );
                    assert!(response["result"].to_string().contains("permission_denied"));
                    assert!(
                        !events.iter().any(|event| event.kind == "chat.message"),
                        "stale {action}/{transition} appended a message"
                    );
                    assert_eq!(
                        store.list().expect("groups").len(),
                        before_groups,
                        "stale source created a Group"
                    );
                    assert!(
                        !events.iter().any(|event| event.kind == "actor.stop"),
                        "stale leave changed membership"
                    );
                }
            }
        }
    }
}
