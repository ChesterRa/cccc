#![cfg(unix)]
use cccc_client::DaemonClient;
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, local_session::LocalSessionRuntime};
use cccc_core::{
    GroupStore, HomeLayout, actors,
    local_sessions::{self as native, NativeHost, NativeSession},
};
use serde_json::{Map, Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn isolated_command(root: &Path, home: &HomeLayout) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    command
        .env_clear()
        .env("PATH", "")
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("CCCC_HOME", home.root())
        .env("CODEX_HOME", root.join("codex"))
        .env("CLAUDE_CONFIG_DIR", root.join("provider"))
        .env("PI_CODING_AGENT_DIR", root.join("pi"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .current_dir(root)
        .kill_on_drop(true);
    command
}

async fn call(client: &DaemonClient, op: &str, args: Value) -> Value {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("args").clone(),
        })
        .await
        .expect("isolated daemon response");
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

#[tokio::test]
async fn claude_monitor_reattaches_after_in_app_resume_without_restarting_the_receiver() {
    let temp = tempfile::tempdir().expect("isolated native fixture");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    let provider = temp.path().join("provider");
    std::fs::create_dir(&provider).expect("provider");
    let host = NativeHost::capture(std::process::id()).expect("native host fixture");
    let mut current = NativeSession {
        source_id: format!("claude:{}", host.key()),
        runtime: LocalSessionRuntime::Claude,
        native_session_id: "initial-conversation".into(),
        host: host.clone(),
        cwd: temp.path().to_string_lossy().into_owned(),
        title: "Current native host".into(),
        origin: None,
        revision: "initial-revision".into(),
        codex_home: None,
        codex_can_receive: false,
        codex_program: None,
        claude_config_dir: Some(provider),
        pi_agent_dir: None,
        opencode_config_dir: None,
        opencode_server: None,
        retired: None,
    };
    native::write(&home, current.clone()).expect("initial conversation");
    let mut previous = current.clone();
    previous.host.started = "0".into();
    previous.source_id = format!("claude:{}", previous.host.key());
    previous.native_session_id = "resumed-conversation".into();
    previous.revision = "previous-revision".into();
    native::write(&home, previous.clone()).expect("previous exited host");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("Original Group", "").expect("Group");
    let mut actor = Actor::new("original-member");
    actor.runtime = ActorRuntime::Claude;
    actor.external_session_id.clone_from(&previous.source_id);
    let actor = actors::add(&mut group, actor).expect("member");
    groups.save(&group).expect("persist Group");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
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
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("daemon ready");
    let mut receiver = isolated_command(temp.path(), &home)
        .args(["sessions", "receive", "--host-pid", &host.pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("real native receive process");
    let mut lines = BufReader::new(receiver.stdout.take().expect("Monitor output")).lines();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let sources = call(&client, "local_session_list", json!({})).await;
            if sources["sources"]
                .as_array()
                .expect("sources")
                .iter()
                .any(|s| s["source_id"] == current.source_id)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("initial receive attachment");
    assert_eq!(
        groups.load(&group.group_id).expect("Group").actors[0].external_session_id,
        previous.source_id
    );

    // Match a native in-app /resume: the Monitor process stays running. No
    // artificial delay forces it to observe the retired gap before the new ID.
    native::retire(
        &home,
        &current.source_id,
        &current.native_session_id,
        &host,
        native::RetireReason::SessionEnd,
    )
    .expect("old context ends");
    current.native_session_id = "resumed-conversation".into();
    current.revision = "resumed-revision".into();
    native::write(&home, current.clone()).expect("resumed context starts");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if groups.load(&group.group_id).expect("Group").actors[0].external_session_id
                == current.source_id
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the same Monitor must reattach and restore membership");
    let restored = groups.load(&group.group_id).expect("restored Group");
    assert_eq!(restored.actors[0].id, actor.id);
    assert_eq!(restored.actors[0].generation, actor.generation);
    let sent=call(&client,"send",json!({"group_id":group.group_id,"by":"user","to":[actor.id],"text":"Native resume integration check","message_mode":"send"})).await;
    let event_id = sent["event"]["id"].as_str().expect("sent message");
    let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .expect("Monitor receipt deadline")
        .expect("Monitor stdout")
        .expect("Monitor notice");
    let notice: Value = serde_json::from_str(&line).expect("Monitor JSON");
    let message = notice["message"].as_str().expect("notification");
    assert!(
        message.contains(event_id) && message.contains(&group.group_id),
        "{message}"
    );

    // The resumed native MCP must also authorize the original Actor, rather
    // than proving only that a notification was written to a pipe.
    let mut mcp = isolated_command(temp.path(), &home)
        .args([
            "mcp",
            "--local-session-runtime",
            "claude",
            "--local-session-host-pid",
            &host.pid.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("native MCP");
    let mut input = mcp.stdin.take().expect("MCP stdin");
    input.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"cccc_bootstrap","arguments":{"group_id":group.group_id}}})).as_bytes()).await.expect("bootstrap request");
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(5), mcp.wait_with_output())
        .await
        .expect("MCP exit")
        .expect("MCP output");
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).expect("MCP response");
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["session"]["actor_id"],
        actor.id
    );
    receiver.start_kill().expect("stop own receiver");
    receiver.wait().await.expect("receiver exit");
    call(&client, "shutdown", json!({})).await;
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .expect("daemon exit")
        .expect("task")
        .expect("clean shutdown");
}
