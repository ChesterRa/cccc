#![cfg(unix)]

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::time::Duration;

static DAEMON_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn directed_message_auto_wakes_a_stopped_actor() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("auto-wake-test", true).await;

    let stopped_group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(stopped_group.result["group"]["state"], "active");
    assert_eq!(stopped_group.result["group"]["actors"][0]["enabled"], false);

    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake up"}),
    )
    .await;
    assert_eq!(sent.result["delivery"]["state"], "queued");
    assert_eq!(sent.result["delivery"]["online"], 0);
    assert_eq!(sent.result["delivery"]["queued"], 1);

    wait_for_terminal(&client, &group_id, "MESSAGE:[cccc] user → peer1: wake up").await;
    let actors = call(
        &client,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(actors.result["actors"][0]["enabled"], true);
    assert_eq!(actors.result["actors"][0]["running"], true);
    wait_until_async(|| async {
        let inbox = call(
            &client,
            "inbox_list",
            json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
        )
        .await;
        inbox.result["messages"]
            .as_array()
            .is_some_and(Vec::is_empty)
    })
    .await;

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn directed_message_does_not_wake_an_explicitly_stopped_group() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("stopped-group-test", false).await;
    call(
        &client,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;

    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"stay stopped"}),
    )
    .await;
    assert_eq!(sent.result["delivery"]["queued"], 0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!cccc_runtime::status(&group_id, "peer1").is_ok_and(|status| status.running));
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "stopped");

    shutdown(&client, daemon).await;
    drop(temp);
}

async fn setup(
    title: &str,
    reads_delivery: bool,
) -> (
    tempfile::TempDir,
    tokio::task::JoinHandle<anyhow::Result<()>>,
    DaemonClient,
    String,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let daemon = tokio::spawn(cccc_daemon::run(home.clone()));
    wait_until(|| cccc_daemon::DaemonPaths::new(home.clone()).address.exists()).await;
    let client = DaemonClient::new(home.clone());
    let created = call(&client, "group_create", json!({"title":title,"by":"user"})).await;
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    let command = if reads_delivery {
        "stty -echo; IFS= read -r preamble; IFS= read -r message; printf 'PREAMBLE:%s\\nMESSAGE:%s' \"$preamble\" \"$message\"; sleep 2"
    } else {
        "sleep 30"
    };
    call(
        &client,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runner":"pty",
            "runtime":"custom",
            "submit":"newline",
            "command":["sh","-c",command],
            "by":"user"
        }),
    )
    .await;
    call(
        &client,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;
    if reads_delivery {
        call(
            &client,
            "actor_stop",
            json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
        )
        .await;
    }
    (temp, daemon, client, group_id)
}

async fn wait_for_terminal(client: &DaemonClient, group_id: &str, expected: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    loop {
        let response = raw_call(
            client,
            "terminal_tail",
            json!({"group_id":group_id,"actor_id":"peer1"}),
        )
        .await;
        if response.ok
            && response.result["text"]
                .as_str()
                .is_some_and(|text| text.contains(expected))
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "PTY did not receive {expected:?}; response={response:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn shutdown(client: &DaemonClient, daemon: tokio::task::JoinHandle<anyhow::Result<()>>) {
    call(client, "shutdown", json!({})).await;
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .expect("daemon shutdown timeout")
        .expect("daemon task")
        .expect("daemon result");
}

async fn call(client: &DaemonClient, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(client, op, args).await;
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

async fn raw_call(client: &DaemonClient, op: &str, args: Value) -> DaemonResponse {
    client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        })
        .await
        .expect("daemon request")
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_until_async<F, Fut>(mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(7);
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
