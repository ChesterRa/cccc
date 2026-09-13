use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, web_model_connectors};
use serde_json::{Map, Value, json};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

fn request(name: &str, arguments: Value, session: Option<&str>) -> Value {
    let mut params = json!({"name": name, "arguments": arguments});
    if let Some(session) = session {
        params["_meta"] = json!({"openai/session":session});
    }
    json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":params})
}

fn payload(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}
fn error(response: &Value) -> &Value {
    &payload(response)["error"]["code"]
}

async fn start_daemon(
    home: &HomeLayout,
) -> (tokio::task::JoinHandle<anyhow::Result<()>>, DaemonClient) {
    let daemon_home = home.clone();
    let task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Map::new(),
            })
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    (task, client)
}

async fn stop_daemon(client: &DaemonClient, task: tokio::task::JoinHandle<anyhow::Result<()>>) {
    let _ = client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
}

async fn daemon(client: &DaemonClient, op: &str, args: Value) -> Value {
    let r = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().expect("object"),
        })
        .await
        .expect("daemon call");
    assert!(r.ok, "daemon operation {op} failed: {r:?}");
    Value::Object(r.result)
}

async fn daemon_group(client: &DaemonClient, path: &Path, title: &str) -> String {
    daemon(
        client,
        "group_create_with_scope",
        json!({"path":path,"title":title,"by":"user","set_active":false}),
    )
    .await["group_id"]
        .as_str()
        .expect("group id")
        .to_owned()
}

async fn add_actor(
    client: &DaemonClient,
    group: &str,
    actor_id: &str,
    runtime: &str,
    command: Option<&[&str]>,
) {
    let mut args = json!({"group_id":group,"actor_id":actor_id,"runtime":runtime,"by":"user"});
    if let Some(command) = command {
        args["command"] = json!(command);
    }
    daemon(client, "actor_add", args).await;
}

fn load_group(home: &HomeLayout, group: &str) -> GroupDoc {
    GroupStore::new(home.clone())
        .expect("store")
        .load(group)
        .expect("group")
}

#[tokio::test]
async fn gateway_requires_transport_session_before_any_group_or_user_fallback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let forged = request(
        "cccc_bootstrap",
        json!({"group_id":"foreign", "by":"user",
        "session":"model-supplied", "_meta":{"openai/session":"model-supplied"}}),
        None,
    );
    let result = cccc_mcp::handle_request_for_gateway(&home, &forged).await;
    assert_eq!(error(&result), "session_binding_required", "{result}");
    assert!(!result.to_string().contains("model-supplied"));
    let unbound = cccc_mcp::handle_request_for_gateway(
        &home,
        &request("cccc_bootstrap", json!({}), Some("unbound-chat")),
    )
    .await;
    assert_eq!(error(&unbound), "session_binding_required", "{unbound}");
    let malformed = cccc_mcp::handle_request_for_gateway(
        &home,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":[]}),
    )
    .await;
    assert_eq!(malformed["error"]["code"], -32602);
}

#[tokio::test]
async fn gateway_catalog_is_fixed_and_legacy_catalog_does_not_acquire_binding_tools() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let req = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let gateway = cccc_mcp::handle_request_for_gateway(&home, &req).await;
    let tools = gateway["result"]["tools"].as_array().expect("tools");
    assert!(tools.iter().any(|t| t["name"] == "cccc_session_bind"));
    assert!(tools.iter().any(|t| t["name"] == "cccc_bootstrap"));
    assert!(!tools.iter().any(|t| t["name"] == "cccc_remote_shell"));
    let mut another = req.clone();
    another["params"] = json!({"_meta":{"openai/session":"another"}});
    assert_eq!(
        gateway,
        cccc_mcp::handle_request_for_gateway(&home, &another).await
    );
    let legacy = cccc_mcp::handle_request(&home, &req).await;
    assert!(
        !legacy["result"]["tools"]
            .as_array()
            .expect("legacy tools")
            .iter()
            .any(|t| t["name"] == "cccc_session_bind")
    );
}

struct Gateway {
    child: Child,
    input: ChildStdin,
    output: tokio::io::Lines<BufReader<ChildStdout>>,
}
impl Gateway {
    fn start(home: &HomeLayout) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cccc-mcp"))
            .arg("--gateway")
            .env("CCCC_HOME", home.root())
            // These deliberately wrong defaults must never override a gateway binding.
            .env("CCCC_GROUP_ID", "g_wrong_default")
            .env("CCCC_ACTOR_ID", "user")
            .env("CCCC_MCP_TOOL_PROFILE", "full")
            .env("CCCC_WEB_MODEL_CODE_MODE", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("gateway child");
        let input = child.stdin.take().expect("stdin");
        let output = BufReader::new(child.stdout.take().expect("stdout")).lines();
        Self {
            child,
            input,
            output,
        }
    }
    async fn call(&mut self, req: Value) -> Value {
        self.input
            .write_all(format!("{req}\n").as_bytes())
            .await
            .expect("write request");
        self.input.flush().await.expect("flush");
        let line = tokio::time::timeout(Duration::from_secs(10), self.output.next_line())
            .await
            .expect("response timeout")
            .expect("response read")
            .expect("response present");
        serde_json::from_str(&line).expect("JSON response")
    }
    async fn stop(mut self) {
        self.input.shutdown().await.expect("close input");
        drop(self.input);
        let status = tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .expect("child exit timeout")
            .expect("child exit");
        assert!(status.success());
    }
}

#[tokio::test]
async fn real_stdio_gateway_binds_reads_mail_rejects_cross_group_and_invalidates_old_chat() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let (daemon_task, client) = start_daemon(&home).await;
    // Creation uses real daemon policy. No second Web Model or bypass of its singleton.
    let created = daemon(
        &client,
        "group_create",
        json!({"title":"gateway-visible","by":"user"}),
    )
    .await;
    let gid = created["group_id"].as_str().expect("group id").to_owned();
    add_actor(&client, &gid, "web-lead", "web_model", None).await;
    let hidden = daemon(
        &client,
        "group_create",
        json!({"title":"must-not-leak","by":"user"}),
    )
    .await;
    let foreign = hidden["group_id"]
        .as_str()
        .expect("foreign group")
        .to_owned();
    web_model_connectors::replace_active(
        &home,
        &json!({"connector_id":"test-route","group_id":gid,
        "actor_id":"web-lead","secret_hash":"test-only-placeholder","provider":"chatgpt"}),
    )
    .expect("connector");
    daemon(
        &client,
        "attach",
        json!({"group_id":gid,"path":temp.path().to_string_lossy(),"by":"user"}),
    )
    .await;
    let issue = || {
        web_model_connectors::prepare_binding(&home, "test-route", 600).expect("issue")["code"]
            .as_str()
            .expect("code")
            .to_owned()
    };
    let mut g = Gateway::start(&home);
    let init = g
        .call(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}))
        .await;
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    let a = Some("synthetic-chat-a");
    let b = Some("synthetic-chat-b");
    assert_eq!(
        error(&g.call(request("cccc_bootstrap", json!({}), a)).await),
        "session_binding_required"
    );
    let code = issue();
    let bound = g
        .call(request("cccc_session_bind", json!({"code":code}), a))
        .await;
    assert_eq!(payload(&bound)["bound"], true, "{bound}");
    assert_eq!(payload(&bound)["group_id"], gid);
    let boot = g
        .call(request(
            "cccc_bootstrap",
            json!({"by":"user","actor_id":"user"}),
            a,
        ))
        .await;
    assert_eq!(payload(&boot)["session"]["group_id"], gid, "{boot}");
    assert_eq!(payload(&boot)["session"]["actor_id"], "web-lead");
    assert!(!boot.to_string().contains("must-not-leak"));
    assert_eq!(
        error(
            &g.call(request("cccc_bootstrap", json!({"group_id":foreign}), a))
                .await
        ),
        "group_scope_mismatch"
    );
    assert_eq!(
        error(
            &g.call(request(
                "cccc_message_send",
                json!({"dst_group_id":foreign,"text":"no","to":["user"]}),
                a
            ))
            .await
        ),
        "group_scope_mismatch"
    );
    let nested = g
        .call(request(
            "cccc_capability_use",
            json!({"tool_name":"cccc_project_info","tool_arguments":{"group_id":foreign}}),
            a,
        ))
        .await;
    assert_eq!(error(&nested), "group_scope_mismatch", "{nested}");
    let listed = g
        .call(request(
            "cccc_capability_use",
            json!({"tool_name":"cccc_group","tool_arguments":{"action":"list"}}),
            a,
        ))
        .await;
    assert!(!listed.to_string().contains("must-not-leak"), "{listed}");
    assert_eq!(
        error(&g.call(request("cccc_bootstrap", json!({}), b)).await),
        "session_binding_required"
    );
    assert_eq!(
        error(
            &g.call(request("cccc_session_bind", json!({"code":code}), b))
                .await
        ),
        "session_binding_code_invalid"
    );
    daemon(
        &client,
        "send",
        json!({"group_id":gid,"by":"user","to":["web-lead"],"text":"local-report","message_mode":"mail"}),
    )
    .await;
    let inbox = g.call(request("cccc_inbox_read", json!({}), a)).await;
    assert!(inbox.to_string().contains("local-report"), "{inbox}");
    // Real code-mode calls must preserve the conversation scope through nested tools.
    let source = format!(
        r#"
        const boot = await tools.cccc_bootstrap({{}});
        text(boot.session.group_id);
        try {{ await tools.cccc_bootstrap({{group_id: {}}}); text("UNEXPECTED"); }}
        catch (err) {{ text(err.code); }}
    "#,
        serde_json::to_string(&foreign).expect("quoted id")
    );
    let code_result = g
        .call(request(
            "cccc_code_exec",
            json!({"source":source,"yield_time_ms":5000}),
            a,
        ))
        .await;
    assert_eq!(
        payload(&code_result)["status"],
        "completed",
        "{code_result}"
    );
    assert!(
        payload(&code_result)["output"]
            .as_str()
            .expect("output")
            .contains(&gid)
    );
    assert!(
        payload(&code_result)["output"]
            .as_str()
            .expect("output")
            .contains("group_scope_mismatch")
    );
    assert!(!code_result.to_string().contains("UNEXPECTED"));
    let pending = g
        .call(request(
            "cccc_code_exec",
            json!({"source":"await new Promise(resolve=>setTimeout(resolve,2000)); text(await tools.cccc_bootstrap({}));", "yield_time_ms":0}),
            a,
        ))
        .await;
    let pending_cell = payload(&pending)["cell_id"].clone();
    assert_eq!(payload(&pending)["running"], true, "{pending}");
    let replacement = issue();
    assert_eq!(
        payload(
            &g.call(request("cccc_session_bind", json!({"code":replacement}), b))
                .await
        )["bound"],
        true
    );
    assert_eq!(
        error(&g.call(request("cccc_bootstrap", json!({}), a)).await),
        "session_binding_required"
    );
    let boot = g.call(request("cccc_bootstrap", json!({}), b)).await;
    assert_eq!(payload(&boot)["session"]["group_id"], gid);
    let old_cell = g
        .call(request(
            "cccc_code_wait",
            json!({"cell_id":pending_cell}),
            b,
        ))
        .await;
    assert_eq!(
        payload(&old_cell)["status"],
        "missing",
        "new chat must not collect old chat's cell: {old_cell}"
    );
    // A process restart retains the stored binding, not an in-memory current group.
    g.stop().await;
    let mut g = Gateway::start(&home);
    assert_eq!(
        payload(&g.call(request("cccc_bootstrap", json!({}), b)).await)["session"]["group_id"],
        gid
    );
    GroupStore::new(home.clone())
        .expect("store")
        .mutate(&gid, |group| {
            group.actors[0].enabled = false;
            Ok(())
        })
        .expect("disable");
    assert_eq!(
        error(&g.call(request("cccc_bootstrap", json!({}), b)).await),
        "connector_actor_unavailable"
    );
    g.stop().await;
    stop_daemon(&client, daemon_task).await;
}

#[tokio::test]
async fn chat_first_creates_two_groups_and_manages_the_named_local_peer() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let (daemon_task, client) = start_daemon(&home).await;
    let mut gateway = Gateway::start(&home);
    let path = temp.path().join("project");
    std::fs::create_dir(&path).expect("project");
    let create = json!({"path":path,"title":"Chat first A"});
    let a = gateway
        .call(request(
            "cccc_group_create",
            create.clone(),
            Some("chat-first-a"),
        ))
        .await;
    assert_eq!(payload(&a)["status"], "needs_chat_url", "{a}");
    let ga = payload(&a)["group_id"]
        .as_str()
        .expect("group A")
        .to_owned();
    assert_eq!(payload(&a)["role"], "foreman");
    assert_eq!(payload(&a)["can_dispatch"], true);
    assert_eq!(payload(&a)["callback_target_ready"], false);
    assert_eq!(
        cccc_core::active::get(&home).expect("active group"),
        None,
        "Chat-first creation changed the global active group"
    );
    let again = gateway
        .call(request("cccc_group_create", create, Some("chat-first-a")))
        .await;
    assert_eq!(payload(&again)["group_id"], ga);
    assert_eq!(payload(&again)["reused"], true);
    let b = gateway
        .call(request(
            "cccc_group_create",
            json!({"path":path,"title":"Chat first B"}),
            Some("chat-first-b"),
        ))
        .await;
    let gb = payload(&b)["group_id"]
        .as_str()
        .expect("group B")
        .to_owned();
    assert_ne!(ga, gb);
    let add = gateway.call(request("cccc_capability_use", json!({"tool_name":"cccc_actor","tool_arguments":{"action":"add","actor_id":"local-worker","runtime":"opencode","title":"Local worker"}}), Some("chat-first-a"))).await;
    assert_ne!(add["result"]["isError"], true, "{add}");
    assert!(
        load_group(&home, &ga)
            .actors
            .iter()
            .any(|actor| actor.id == "local-worker"
                && actor.runtime == cccc_contracts::ActorRuntime::Opencode),
        "{add}"
    );
    let update = gateway.call(request("cccc_capability_use", json!({"tool_name":"cccc_actor","tool_arguments":{"action":"update","actor_id":"local-worker","patch":{"title":"Updated worker"}}}), Some("chat-first-a"))).await;
    assert_ne!(update["result"]["isError"], true, "{update}");
    assert_eq!(load_group(&home, &ga).actors[1].title, "Updated worker");
    let second_web = gateway.call(request("cccc_capability_use", json!({"tool_name":"cccc_actor","tool_arguments":{"action":"add","actor_id":"second-web","runtime":"web_model"}}), Some("chat-first-a"))).await;
    assert_eq!(
        error(&second_web),
        "chatgpt_web_model_singleton",
        "{second_web}"
    );
    let url = "https://chatgpt.com/c/stable-chat-first-a";
    let target = gateway
        .call(request(
            "cccc_group_bind",
            json!({"group":ga,"chat_url":url}),
            Some("chat-first-a"),
        ))
        .await;
    assert_eq!(payload(&target)["callback_target_ready"], true, "{target}");
    assert_eq!(payload(&target)["status"], "configured");
    assert_eq!(
        load_group(&home, &ga).extra["web_model_browser_targets"]["chat-foreman"]["url"],
        url
    );
    let steal = gateway
        .call(request(
            "cccc_group_bind",
            json!({"group":ga}),
            Some("third-chat"),
        ))
        .await;
    assert_eq!(error(&steal), "group_already_bound", "{steal}");
    let invalid = gateway
        .call(request(
            "cccc_group_create",
            json!({"path":path,"chat_url":"https://example.com/c/wrong"}),
            Some("fourth-chat"),
        ))
        .await;
    assert_eq!(error(&invalid), "invalid_chat_url", "{invalid}");
    assert_eq!(
        GroupStore::new(home.clone())
            .expect("store")
            .list()
            .expect("groups")
            .len(),
        2
    );
    gateway.stop().await;
    stop_daemon(&client, daemon_task).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_gateways_keep_ten_groups_separate_and_create_once() {
    use std::sync::Arc;
    use tokio::sync::{Barrier, Mutex};
    use tokio::task::JoinSet;
    let temp = tempfile::tempdir().expect("isolated concurrency directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let (daemon_task, client) = start_daemon(&home).await;
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project");
    let home_for_work = home.clone();
    let mut check = tokio::spawn(async move {
        for width in [2, 10] {
            let barrier = Arc::new(Barrier::new(width));
            let ids = Arc::new(Mutex::new(vec![String::new(); width]));
            let mut tasks = JoinSet::new();
            for slot in 0..width {
                let home = home_for_work.clone();
                let path = project.clone();
                let barrier = Arc::clone(&barrier);
                let ids = Arc::clone(&ids);
                tasks.spawn(async move {
                    let mut g = Gateway::start(&home);
                    let session = format!("synthetic-{width}-{slot}");
                    let create = json!({"path":path,"title":format!("concurrent-{width}-{slot}")});
                    barrier.wait().await;
                    let result = g
                        .call(request("cccc_group_create", create.clone(), Some(&session)))
                        .await;
                    let id = payload(&result)["group_id"]
                        .as_str()
                        .expect("created group")
                        .to_owned();
                    let retry = g
                        .call(request("cccc_group_create", create, Some(&session)))
                        .await;
                    assert_eq!(payload(&retry)["group_id"], id, "retry created a second group");
                    ids.lock().await[slot] = id.clone();
                    barrier.wait().await;
                    let foreign = ids.lock().await[(slot + 1) % width].clone();
                    for round in 0..5 {
                        let state = g
                            .call(request("cccc_bootstrap", json!({}), Some(&session)))
                            .await;
                        assert_eq!(payload(&state)["session"]["group_id"], id, "binding drifted on round {round}");
                        let wrong = g
                            .call(request("cccc_bootstrap", json!({"group_id":foreign}), Some(&session)))
                            .await;
                        assert_eq!(error(&wrong), "group_scope_mismatch");
                    }
                    let report = format!("REPORT_{width}_{slot}");
                    daemon(&DaemonClient::new(home.clone()), "send", json!({"group_id":id,"by":"user","to":["chat-foreman"],"text":report,"message_mode":"mail"})).await;
                    let mail = g
                        .call(request("cccc_inbox_read", json!({}), Some(&session)))
                        .await;
                    let messages = payload(&mail)["messages"].as_array().expect("Mail");
                    assert_eq!(messages.len(), 1, "wrong group's inbox or duplicate report");
                    assert_eq!(messages[0]["data"]["text"], report);
                    g.stop().await;
                    id
                });
            }
            let mut seen = std::collections::HashSet::new();
            while let Some(result) = tasks.join_next().await {
                assert!(
                    seen.insert(result.expect("concurrent request task")),
                    "two sessions got the same group"
                );
            }
            assert_eq!(seen.len(), width);
        }
        // Ten chats racing on one synthetic identity must still create the group once.
        let barrier = Arc::new(Barrier::new(10));
        let mut tasks = JoinSet::new();
        for _ in 0..10 {
            let home = home_for_work.clone();
            let path = project.clone();
            let barrier = Arc::clone(&barrier);
            tasks.spawn(async move {
                let mut g = Gateway::start(&home);
                barrier.wait().await;
                let result = g
                    .call(request(
                        "cccc_group_create",
                        json!({"path":path,"title":"one-racing-chat"}),
                        Some("one-synthetic-chat"),
                    ))
                    .await;
                let id = payload(&result)["group_id"]
                    .as_str()
                    .expect("race group")
                    .to_owned();
                g.stop().await;
                id
            });
        }
        let mut ids = std::collections::HashSet::new();
        while let Some(result) = tasks.join_next().await {
            ids.insert(result.expect("race task"));
        }
        assert_eq!(ids.len(), 1, "concurrent retries created duplicate groups");
        assert_eq!(
            GroupStore::new(home_for_work)
                .expect("store")
                .list()
                .expect("groups")
                .len(),
            13
        );
    });
    let result = tokio::time::timeout(Duration::from_secs(90), &mut check).await;
    if result.is_err() {
        check.abort();
        let _ = check.await;
    }
    stop_daemon(&client, daemon_task).await;
    result
        .expect("bounded concurrency test")
        .expect("concurrency assertions");
}

#[tokio::test]
async fn bound_web_peer_keeps_native_role_permissions_and_updates_only_its_target() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let (daemon_task, client) = start_daemon(&home).await;
    let work_home = home.clone();
    let work_client = client.clone();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project");
    let result = tokio::spawn(async move {
        let gid = daemon_group(&work_client, &project, "peer test").await;
        add_actor(&work_client, &gid, "local-lead", "custom", Some(&["cat"])).await;
        add_actor(&work_client, &gid, "web-peer", "web_model", None).await;
        let (entry, _) = web_model_connectors::create(&work_home, &gid, "web-peer", "chatgpt", "Web member")
            .expect("connector");
        let id = entry["connector_id"].as_str().expect("id");
        let prepared = web_model_connectors::prepare_binding(&work_home, id, 600)
            .expect("peer binding code");
        let mut gateway = Gateway::start(&work_home);
        let bound = gateway
            .call(request(
                "cccc_session_bind",
                json!({"code":prepared["code"]}),
                Some("peer-chat"),
            ))
            .await;
        assert_eq!(payload(&bound)["bound"], true, "{bound}");
        let boot = gateway
            .call(request("cccc_bootstrap", json!({}), Some("peer-chat")))
            .await;
        assert_eq!(payload(&boot)["session"]["role"], "peer", "{boot}");
        assert_eq!(payload(&boot)["session"]["actor_id"], "web-peer");
        let report = gateway
            .call(request(
                "cccc_message_send",
                json!({"to":["user"],"mode":"send","text":"peer report"}),
                Some("peer-chat"),
            ))
            .await;
        assert_ne!(report["result"]["isError"], true, "{report}");
        let forbidden = gateway.call(request("cccc_capability_use", json!({"tool_name":"cccc_actor","tool_arguments":{"action":"add","actor_id":"forbidden","runtime":"custom","by":"user"}}), Some("peer-chat"))).await;
        assert_eq!(
            forbidden["result"]["isError"], true,
            "peer received Foreman administration: {forbidden}"
        );
        let own_target = gateway
            .call(request(
                "cccc_group_bind",
                json!({"group":gid,"chat_url":"https://chatgpt.com/c/peer-chat"}),
                Some("peer-chat"),
            ))
            .await;
        assert_eq!(payload(&own_target)["role"], "peer", "{own_target}");
        assert_eq!(payload(&own_target)["actor_id"], "web-peer");
        assert_eq!(payload(&own_target)["callback_target_ready"], true);
        let group = load_group(&work_home, &gid);
        assert_eq!(group.actors[0].id, "local-lead");
        assert_eq!(group.actors.len(), 2, "peer created a privileged member");
        assert_eq!(
            group.extra["web_model_browser_targets"]["web-peer"]["url"],
            "https://chatgpt.com/c/peer-chat"
        );
        assert!(group.extra["web_model_browser_targets"].get("local-lead").is_none());
        gateway.stop().await;
    })
    .await;
    stop_daemon(&client, daemon_task).await;
    result.expect("peer assertions");
}

#[tokio::test]
async fn unbound_chat_first_bind_selects_the_unique_web_member_under_a_local_foreman() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let (daemon_task, client) = start_daemon(&home).await;
    let work_home = home.clone();
    let work_client = client.clone();
    let project = temp.path().join("lead-and-peer");
    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir(&project).expect("project");
    std::fs::create_dir(&elsewhere).expect("elsewhere");
    let result = tokio::spawn(async move {
        let gid = daemon_group(&work_client, &project, "lead and peer").await;
        add_actor(&work_client, &gid, "local-lead", "custom", Some(&["cat"])).await;
        add_actor(&work_client, &gid, "web-peer", "web_model", None).await;
        let mut gateway = Gateway::start(&work_home);
        let chat = Some("first-bind-chat");
        let bind = gateway
            .call(request(
                "cccc_group_bind",
                json!({"group":gid,"chat_url":"https://chatgpt.com/c/first-bind-chat"}),
                chat,
            ))
            .await;
        assert_eq!(payload(&bind)["role"], "peer", "{bind}");
        assert_eq!(payload(&bind)["actor_id"], "web-peer", "{bind}");
        assert_eq!(payload(&bind)["status"], "configured", "{bind}");
        assert_eq!(payload(&bind)["callback_target_ready"], true, "{bind}");
        assert_eq!(payload(&bind)["reused"], true, "{bind}");
        let rebind = gateway
            .call(request(
                "cccc_group_bind",
                json!({"group":gid,"chat_url":"https://chatgpt.com/c/first-bind-chat-2"}),
                chat,
            ))
            .await;
        assert_eq!(payload(&rebind)["actor_id"], "web-peer", "{rebind}");
        assert_eq!(payload(&rebind)["callback_target_ready"], true, "{rebind}");
        assert_eq!(
            load_group(&work_home, &gid).extra["web_model_browser_targets"]["web-peer"]["url"],
            "https://chatgpt.com/c/first-bind-chat-2"
        );
        let other_id = daemon_group(&work_client, &elsewhere, "elsewhere").await;
        let cross = gateway
            .call(request("cccc_group_bind", json!({"group":other_id}), chat))
            .await;
        assert_eq!(error(&cross), "session_already_bound", "{cross}");
        let boot = gateway
            .call(request("cccc_bootstrap", json!({}), chat))
            .await;
        assert_eq!(payload(&boot)["session"]["actor_id"], "web-peer", "{boot}");
        gateway.stop().await;
    })
    .await;
    stop_daemon(&client, daemon_task).await;
    result.expect("first-bind assertions");
}

#[tokio::test]
async fn unbound_chat_first_bind_keeps_conflict_semantics_and_rejects_unusable_web_members() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let (daemon_task, client) = start_daemon(&home).await;
    let work_home = home.clone();
    let work_client = client.clone();
    let project = temp.path().join("shapes");
    std::fs::create_dir(&project).expect("projects");
    let result = tokio::spawn(async move {
        let local_only = daemon_group(&work_client, &project, "local only").await;
        add_actor(
            &work_client,
            &local_only,
            "solo-local",
            "custom",
            Some(&["cat"]),
        )
        .await;
        let disabled = daemon_group(&work_client, &project, "disabled web").await;
        add_actor(
            &work_client,
            &disabled,
            "local-lead",
            "custom",
            Some(&["cat"]),
        )
        .await;
        add_actor(&work_client, &disabled, "web-off", "web_model", None).await;
        daemon(
            &work_client,
            "actor_update",
            json!({"group_id":disabled,"actor_id":"web-off",
            "patch":{"enabled":false},"by":"user"}),
        )
        .await;
        let ambiguous = daemon_group(&work_client, &project, "ambiguous web").await;
        add_actor(
            &work_client,
            &ambiguous,
            "local-lead",
            "custom",
            Some(&["cat"]),
        )
        .await;
        add_actor(&work_client, &ambiguous, "web-a", "web_model", None).await;
        // Group data can carry more than one Web Model member even though actor_add
        // refuses to create that state; onboarding must not resolve it by member order.
        GroupStore::new(work_home.clone())
            .expect("store")
            .mutate(&ambiguous, |group| {
                let mut actor = cccc_contracts::Actor::new("web-b");
                actor.runtime = cccc_contracts::ActorRuntime::WebModel;
                cccc_core::actors::add(group, actor)
            })
            .expect("second web member");
        let web_only = daemon_group(&work_client, &project, "web only").await;
        add_actor(&work_client, &web_only, "web-solo", "web_model", None).await;
        let mut gateway = Gateway::start(&work_home);
        let chat = Some("shape-chat");
        let local_only_bind = gateway
            .call(request(
                "cccc_group_bind",
                json!({"group":local_only,"chat_url":"https://chatgpt.com/c/local-only"}),
                chat,
            ))
            .await;
        assert_eq!(
            error(&local_only_bind),
            "foreman_conflict",
            "{local_only_bind}"
        );
        assert_eq!(
            error(
                &gateway
                    .call(request("cccc_bootstrap", json!({}), chat))
                    .await
            ),
            "session_binding_required",
            "a rejected bind must not leave the session bound"
        );
        let disabled_bind = gateway
            .call(request(
                "cccc_group_bind",
                json!({"group":disabled,"chat_url":"https://chatgpt.com/c/disabled"}),
                chat,
            ))
            .await;
        assert_eq!(error(&disabled_bind), "foreman_conflict", "{disabled_bind}");
        let ambiguous_bind = gateway
            .call(request(
                "cccc_group_bind",
                json!({"group":ambiguous,"chat_url":"https://chatgpt.com/c/ambiguous"}),
                chat,
            ))
            .await;
        assert_eq!(
            error(&ambiguous_bind),
            "ambiguous_web_member",
            "{ambiguous_bind}"
        );
        let connectors = web_model_connectors::load(&work_home).expect("connectors");
        assert!(
            connectors
                .iter()
                .all(|entry| { entry["group_id"] != ambiguous || entry["revoked"] == true }),
            "an ambiguous group must not gain an owned connector: {connectors:?}"
        );
        let web_only_bind = gateway
            .call(request(
                "cccc_group_bind",
                json!({"group":web_only,"chat_url":"https://chatgpt.com/c/web-solo"}),
                chat,
            ))
            .await;
        assert_eq!(
            payload(&web_only_bind)["role"],
            "foreman",
            "{web_only_bind}"
        );
        assert_eq!(
            payload(&web_only_bind)["actor_id"],
            "web-solo",
            "{web_only_bind}"
        );
        assert_eq!(
            payload(&web_only_bind)["status"],
            "configured",
            "{web_only_bind}"
        );
        let group = load_group(&work_home, &web_only);
        assert_eq!(group.actors.len(), 1, "{group:?}");
        let boot = gateway
            .call(request("cccc_bootstrap", json!({}), chat))
            .await;
        assert_eq!(payload(&boot)["session"]["actor_id"], "web-solo", "{boot}");
        assert_eq!(payload(&boot)["session"]["role"], "foreman", "{boot}");
        gateway.stop().await;
    })
    .await;
    stop_daemon(&client, daemon_task).await;
    result.expect("first-bind shape assertions");
}
