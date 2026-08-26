#![cfg(unix)]

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{HomeLayout, ledger};
use serde_json::{Map, Value, json};
use std::time::Duration;

static DAEMON_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn directed_message_restarts_an_actor_after_unexpected_process_exit() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("crash-auto-wake-test", false).await;
    call(
        &client,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"command":["sh","-c","sleep 1; exit 17"]},
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

    let ledger_path = group_ledger_path(&temp, &group_id);
    wait_until_async(|| async {
        let actors = raw_call(
            &client,
            "actor_list",
            json!({"group_id":group_id,"by":"user"}),
        )
        .await;
        let exit_recorded = ledger::read_all(&ledger_path).is_ok_and(|events| {
            events.iter().any(|event| {
                event.kind == "actor.stop"
                    && event.data["actor_id"] == "peer1"
                    && event.data["reason"] == "process_exit"
            })
        });
        exit_recorded
            && actors.ok
            && actors.result["actors"][0]["enabled"] == true
            && actors.result["actors"][0]["running"] == false
    })
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
    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake after crash","message_mode":"send"}),
    )
    .await;
    let source_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("source event id")
        .to_owned();
    wait_for_accepted_delivery(&ledger_path, &source_event_id).await;
    let actors = call(
        &client,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(actors.result["actors"][0]["enabled"], true);
    assert_eq!(actors.result["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn request_reply_wakes_an_explicitly_stopped_actor() {
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
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake up","message_mode":"request_reply"}),
    )
    .await;
    assert_eq!(sent.result["message_mode"], "request_reply");
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();
    let ledger_path = group_ledger_path(&temp, &group_id);
    wait_for_accepted_delivery(&ledger_path, &sent_event_id).await;
    let actors = call(
        &client,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(actors.result["actors"][0]["enabled"], true);
    assert_eq!(actors.result["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn directed_message_wakes_an_explicitly_stopped_group() {
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
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();
    wait_for_accepted_delivery(&group_ledger_path(&temp, &group_id), &sent_event_id).await;
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "active");
    assert_eq!(group.result["group"]["actors"][0]["enabled"], true);
    assert_eq!(group.result["group"]["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn mail_does_not_wake_an_explicitly_stopped_group() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("stopped-group-mail-test", false).await;
    call(
        &client,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;

    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"stay in inbox","message_mode":"mail"}),
    )
    .await;
    assert_eq!(sent.result["message_mode"], "mail");
    let sent_event_id = sent.result["event"]["id"].as_str().expect("sent event id");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let deliveries = ledger::read_all(&group_ledger_path(&temp, &group_id))
        .expect("read ledger")
        .into_iter()
        .filter(|event| {
            event.kind == "runtime.delivery" && event.data["source_event_id"] == sent_event_id
        })
        .count();
    assert_eq!(deliveries, 0);
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "stopped");
    assert_eq!(group.result["group"]["actors"][0]["running"], false);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn directed_message_resumes_a_paused_group_and_delivers() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("paused-session-recovery", false).await;
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"paused","by":"user"}),
    )
    .await;
    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"resume-on-send","message_mode":"send"}),
    )
    .await;
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();
    wait_for_accepted_delivery(&group_ledger_path(&temp, &group_id), &sent_event_id).await;
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "active");
    assert_eq!(group.result["group"]["actors"][0]["enabled"], true);
    assert_eq!(group.result["group"]["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn directed_message_resumes_a_paused_group_and_stopped_actor() {
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
    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"message-A","message_mode":"send"}),
    )
    .await;
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();
    wait_for_accepted_delivery(&group_ledger_path(&temp, &group_id), &sent_event_id).await;
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "active");
    assert_eq!(group.result["group"]["actors"][0]["enabled"], true);
    assert_eq!(group.result["group"]["actors"][0]["running"], true);

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

fn group_ledger_path(temp: &tempfile::TempDir, group_id: &str) -> std::path::PathBuf {
    temp.path()
        .join("rust-home/groups")
        .join(group_id)
        .join("ledger.jsonl")
}

async fn wait_for_accepted_delivery(ledger_path: &std::path::Path, source_event_id: &str) {
    wait_until_async(|| async {
        ledger::read_all(ledger_path).is_ok_and(|events| {
            events.iter().any(|event| {
                event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == source_event_id
                    && event.data["actor_id"] == "peer1"
                    && event.data["state"] == "accepted"
            })
        })
    })
    .await;
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
