//! Real nested commands in an isolated Home. No provider or Actor process starts.
#![cfg(unix)]
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};
use std::time::Duration;

async fn call(home: &HomeLayout, group: &str, name: &str, args: Value) -> Value {
    let response = cccc_mcp::handle_request_for_actor(home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}),
        group, "web").await;
    let result = response["result"].clone();
    assert_ne!(result["isError"], true, "{response}");
    result["structuredContent"].clone()
}

#[tokio::test]
async fn nested_commands_survive_poll_deadlines_and_cancel_only_with_the_cell() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("project");
    std::fs::create_dir(&root).expect("workspace");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("code lifecycle", "").expect("group");
    let mut actor = Actor::new("web");
    actor.runtime = ActorRuntime::WebModel;
    actor.normalize_runtime_constraints();
    cccc_core::actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    cccc_core::group_scope::attach(
        &store,
        &group.group_id,
        cccc_core::Scope {
            scope_key: "project".into(),
            url: root.to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        },
    )
    .expect("scope");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        while client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Default::default(),
            })
            .await
            .is_err()
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("ready");
    let source = "text(await tools.cccc_shell({command:\"sh -c 'printf x >> calls; sleep 2; printf result; printf finished > finished'\"}));";
    let result = tokio::time::timeout(
        Duration::from_millis(900),
        call(
            &home,
            &group.group_id,
            "cccc_code_exec",
            json!({"source":source,"yield_time_ms":100}),
        ),
    )
    .await
    .expect("nested execution must not block initial yield");
    assert_eq!(result["status"], "running");
    let id = result["cell_id"].as_str().expect("cell").to_owned();
    for _ in 0..30 {
        if root.join("calls").exists() {
            break;
        }
        call(
            &home,
            &group.group_id,
            "cccc_code_wait",
            json!({"cell_id":id,"yield_time_ms":50}),
        )
        .await;
    }
    assert!(root.join("calls").exists());
    // A disconnected poll must not cancel the command or lose its eventual result.
    let poll_home = home.clone();
    let gid = group.group_id.clone();
    let poll_id = id.clone();
    let poll = tokio::spawn(async move {
        call(
            &poll_home,
            &gid,
            "cccc_code_wait",
            json!({"cell_id":poll_id,"yield_time_ms":10000}),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    let other = tokio::time::timeout(
        Duration::from_millis(500),
        call(
            &home,
            &group.group_id,
            "cccc_code_wait",
            json!({"cell_id":id,"yield_time_ms":30}),
        ),
    )
    .await
    .expect("a second poll has its own deadline");
    assert_eq!(other["status"], "running");
    poll.abort();
    let _ = poll.await;
    // The nested command makes progress even with no active result poll.
    tokio::time::timeout(Duration::from_secs(4), async {
        while !root.join("finished").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background command finishes");
    let result = call(
        &home,
        &group.group_id,
        "cccc_code_wait",
        json!({"cell_id":id,"yield_time_ms":10000}),
    )
    .await;
    assert_eq!(result["status"], "completed");
    assert!(
        result["output"]
            .as_str()
            .expect("output")
            .contains("result")
    );
    assert_eq!(std::fs::read(root.join("calls")).expect("once"), b"x");
    let missing = call(
        &home,
        &group.group_id,
        "cccc_code_wait",
        json!({"cell_id":id}),
    )
    .await;
    assert_eq!(missing["status"], "missing");

    for shutdown in [false, true] {
        let prefix = if shutdown { "shutdown" } else { "terminate" };
        let command = format!(
            "sh -c 'printf started > {prefix}.started; sleep 1; printf leaked > {prefix}.leaked'"
        );
        let source = format!(
            "text(await tools.cccc_shell({}));",
            json!({"command":command})
        );
        let running = call(
            &home,
            &group.group_id,
            "cccc_code_exec",
            json!({"source":source,"yield_time_ms":50}),
        )
        .await;
        let id = running["cell_id"].as_str().expect("cell");
        for _ in 0..20 {
            if root.join(format!("{prefix}.started")).exists() {
                break;
            }
            call(
                &home,
                &group.group_id,
                "cccc_code_wait",
                json!({"cell_id":id,"yield_time_ms":20}),
            )
            .await;
        }
        assert!(root.join(format!("{prefix}.started")).exists());
        if shutdown {
            cccc_mcp::shutdown(&home).await;
        } else {
            let stopped = call(
                &home,
                &group.group_id,
                "cccc_code_wait",
                json!({"cell_id":id,"terminate":true}),
            )
            .await;
            assert_eq!(stopped["status"], "terminated");
        }
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(
            !root.join(format!("{prefix}.leaked")).exists(),
            "owned nested process must stop"
        );
    }
    cccc_mcp::shutdown(&home).await;
    daemon.abort();
    let _ = daemon.await;
}
