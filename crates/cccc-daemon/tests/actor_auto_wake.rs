#![cfg(unix)]

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::time::Duration;

static DAEMON_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn directed_message_waits_for_an_explicitly_stopped_actor_to_start() {
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
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake up","message_mode":"send"}),
    )
    .await;
    assert_eq!(sent.result["message_mode"], "send");
    assert!(sent.result.get("delivery").is_none());
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();

    let actors = call(
        &client,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(actors.result["actors"][0]["enabled"], false);
    assert_eq!(actors.result["actors"][0]["running"], false);
    let inbox = call(
        &client,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    )
    .await;
    assert_eq!(inbox.result["messages"], json!([]));
    let history = call(
        &client,
        "message_history",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","mode":"send"}),
    )
    .await;
    assert_eq!(history.result["messages"][0]["data"]["text"], "wake up");

    call(
        &client,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;

    let metadata = format!("[event_id={sent_event_id} message_mode=send]");
    let output = wait_for_terminal(&client, &group_id, &metadata).await;
    assert!(output.contains(&format!(
        "[cccc] user → peer1 [event_id={sent_event_id} message_mode=send]: wake up"
    )));
    let actors = call(
        &client,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(actors.result["actors"][0]["enabled"], true);
    assert_eq!(actors.result["actors"][0]["running"], true);
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"paused","by":"user"}),
    )
    .await;
    call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"message-C","message_mode":"send"}),
    )
    .await;
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"active","by":"user"}),
    )
    .await;

    let output = wait_for_terminal(&client, &group_id, "message-C").await;
    assert!(output.contains("message-C"));
    let inbox = call(
        &client,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    )
    .await;
    assert_eq!(inbox.result["messages"], json!([]));

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
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"stay stopped","message_mode":"send"}),
    )
    .await;
    assert_eq!(sent.result["message_mode"], "send");
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

#[tokio::test]
async fn resume_delivers_paused_messages_in_order_without_creating_mail() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("paused-session-recovery", false).await;
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"paused","by":"user"}),
    )
    .await;
    call(
        &client,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"command":["sh","-c","stty -echo; while IFS= read -r line; do printf 'RECOVERED:%s\\n' \"$line\"; done"]},
            "by":"user"
        }),
    )
    .await;
    call(
        &client,
        "actor_restart",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;
    call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"message-A","message_mode":"send"}),
    )
    .await;
    call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"message-B","message_mode":"send"}),
    )
    .await;
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"active","by":"user"}),
    )
    .await;

    let output = wait_for_terminal(&client, &group_id, "message-B").await;
    assert!(output.contains("message-A"));
    assert!(output.contains("message-B"));
    let inbox = call(
        &client,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    )
    .await;
    assert_eq!(inbox.result["messages"], json!([]));
    let history = call(
        &client,
        "message_history",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","mode":"send"}),
    )
    .await;
    assert_eq!(history.result["messages"][0]["data"]["text"], "message-B");
    assert_eq!(history.result["messages"][1]["data"]["text"], "message-A");

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn actor_start_delivers_work_that_arrived_while_the_actor_was_paused() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("actor-start-recovery", false).await;
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"paused","by":"user"}),
    )
    .await;
    call(
        &client,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;
    call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"message-A","message_mode":"send"}),
    )
    .await;
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"active","by":"user"}),
    )
    .await;

    call(
        &client,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"command":["sh","-c","stty -echo; while IFS= read -r line; do printf 'RECOVERED:%s\\n' \"$line\"; done"]},
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

    let output = wait_for_terminal(&client, &group_id, "message-A").await;
    assert!(output.contains("message-A"));
    let inbox = call(
        &client,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    )
    .await;
    assert_eq!(inbox.result["messages"], json!([]));

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
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    call(
        &client,
        "attach",
        json!({"group_id":group_id,"path":workspace,"by":"user"}),
    )
    .await;
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

async fn wait_for_terminal(client: &DaemonClient, group_id: &str, expected: &str) -> String {
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
            return response.result["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
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
