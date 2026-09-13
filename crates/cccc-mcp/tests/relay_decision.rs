use cccc_client::DaemonClient;
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout, actors, ledger};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn request(name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":name,"arguments":arguments}
    })
}

fn payload(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}

async fn tool(home: &HomeLayout, group: &str, name: &str, arguments: Value) -> Value {
    cccc_mcp::handle_request_for_actor(home, &request(name, arguments), group, "web-lead").await
}

/// Running group with a Web Model lead that owns the relay decision flow.
fn group_with_web_lead(
    temp: &tempfile::TempDir,
    title: &str,
    workers: &[&str],
) -> (HomeLayout, String, PathBuf) {
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create(title, "").expect("group");
    let mut lead = Actor::new("web-lead");
    lead.runtime = ActorRuntime::WebModel;
    actors::add(&mut group, lead).expect("lead");
    for worker in workers {
        actors::add(&mut group, Actor::new(*worker)).expect("worker");
    }
    group.running = true;
    store.save(&group).expect("save group");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    (home, group.group_id.clone(), ledger_path)
}

/// Append a mail report, its pending handoff, and the web-lead delivery claim.
fn seed_pending_handoff(path: &Path, group: &str, worker: &str, text: &str) -> String {
    let mut report = Event::new("chat.message", group);
    report.by = worker.into();
    report.data = json!({"to":["web-lead"],"message_mode":"mail","text":text})
        .as_object()
        .cloned()
        .expect("report");
    ledger::append(path, &report).expect("report");
    let mut handoff = Event::new("coordination.handoff", group);
    handoff.by = worker.into();
    handoff.data = json!({
        "handoff_id":format!("handoff-{}", report.id),"source_event_ids":[report.id],
        "source_actor_id":worker,"target_actor_id":"web-lead",
        "turn_id":format!("turn-{}", report.id),"summary":text,
        "task_ids":[],"status":"pending_review"
    })
    .as_object()
    .cloned()
    .expect("handoff");
    ledger::append(path, &handoff).expect("handoff");
    let mut claimed = Event::new("runtime.delivery", group);
    claimed.by = "system".into();
    claimed.data = json!({
        "actor_id":"web-lead","source_event_id":report.id,
        "delivery_id":format!("delivery-{}", report.id),"state":"claimed",
        "transport":"web_model_browser"
    })
    .as_object()
    .cloned()
    .expect("claim");
    ledger::append(path, &claimed).expect("claim");
    report.id
}

fn assert_delivery_accepted(events: &[Event], report: &str) {
    let delivery = events
        .iter()
        .rev()
        .find(|event| {
            event.kind == "runtime.delivery"
                && event.data["actor_id"] == "web-lead"
                && event.data["source_event_id"] == report
        })
        .expect("delivery for report");
    assert_eq!(delivery.data["state"], "accepted");
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

#[tokio::test]
async fn mcp_read_exposes_pending_handoff_and_explicit_decision_resolves_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (home, group, path) = group_with_web_lead(&temp, "relay MCP", &["worker"]);
    let report = seed_pending_handoff(
        &path,
        &group,
        "worker",
        "The implementation is ready. The user must choose the release window.",
    );
    let (daemon_task, client) = start_daemon(&home).await;

    let catalog = cccc_mcp::handle_request_for_actor(
        &home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        &group,
        "web-lead",
    )
    .await;
    let coordination = catalog["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|item| item["name"] == "cccc_coordination")
        .expect("coordination tool");
    assert!(
        coordination["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .expect("actions")
            .contains(&json!("decide"))
    );

    let bootstrap = tool(&home, &group, "cccc_bootstrap", json!({})).await;
    assert_eq!(
        payload(&bootstrap)["relay_pending"]["count"],
        1,
        "{bootstrap}"
    );
    assert_eq!(
        payload(&bootstrap)["relay_pending"]["pending"][0]["source_event_ids"],
        json!([report])
    );

    let inbox = tool(&home, &group, "cccc_inbox_read", json!({})).await;
    assert_eq!(payload(&inbox)["messages"][0]["id"], report);
    assert_eq!(payload(&inbox)["relay_pending"]["requires_decision"], true);
    assert_eq!(payload(&inbox)["relay_pending"]["safe_to_idle"], false);

    let reply = tool(
        &home,
        &group,
        "cccc_message_reply",
        json!({
            "event_id":report,"text":"I have read this, but have not chosen the next responsibility yet.",
            "mode":"mail",
            "insight":"A visible acknowledgement and a durable responsibility decision are separate; this reply deliberately leaves the handoff unresolved."
        }),
    )
    .await;
    assert_eq!(payload(&reply)["relay_pending"]["count"], 1, "{reply}");
    assert_eq!(payload(&reply)["relay_pending"]["requires_decision"], true);

    let decision = tool(
        &home,
        &group,
        "cccc_coordination",
        json!({"action":"decide","event_ids":[report],"decision":"wait_user"}),
    )
    .await;
    assert_eq!(
        payload(&decision)["relay"]["decision"],
        "wait_user",
        "{decision}"
    );
    assert_eq!(payload(&decision)["safe_to_idle"], true);
    assert_eq!(payload(&decision)["caller_may_idle"], true);

    let replay = tool(
        &home,
        &group,
        "cccc_coordination",
        json!({
            "action":"decide","event_ids":[report],"decision":"wait_user",
            "summary":"Legacy wording is accepted but is not emitted as another message."
        }),
    )
    .await;
    assert_eq!(payload(&replay)["replayed"], true);

    let final_bootstrap = tool(&home, &group, "cccc_bootstrap", json!({})).await;
    assert_eq!(payload(&final_bootstrap)["relay_pending"]["count"], 0);
    let events = ledger::read_all(&path).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.kind == "chat.message"
                    && event.by == "web-lead"
                    && event.data["to"] == json!(["user"])
            })
            .count(),
        0,
        "machine-only decision duplicated the normal human-facing output"
    );
    assert_delivery_accepted(&events, &report);
    stop_daemon(&client, daemon_task).await;
}

#[tokio::test]
async fn mcp_continue_creates_one_real_next_task_and_transfers_responsibility() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (home, group, path) =
        group_with_web_lead(&temp, "relay MCP continue", &["worker-a", "worker-b"]);
    let report = seed_pending_handoff(
        &path,
        &group,
        "worker-a",
        "Implementation is complete. Run the full affected regression next.",
    );
    let (daemon_task, client) = start_daemon(&home).await;

    let args = json!({
        "action":"decide","event_ids":[report],"decision":"continue",
        "next_actor_id":"worker-b","next_title":"Run the affected regression",
        "next_text":"Run every affected Rust and browser-delivery test. Report the exact commands, failures, remaining risk, and requested next action.",
        "outcome":"All affected checks pass with evidence"
    });
    let decision = tool(&home, &group, "cccc_coordination", args.clone()).await;
    assert_eq!(
        payload(&decision)["relay"]["decision"],
        "continue",
        "{decision}"
    );
    assert_eq!(payload(&decision)["caller_may_idle"], true);
    assert_eq!(payload(&decision)["safe_to_idle"], false);
    assert_eq!(
        payload(&decision)["current_responsibility"]["kind"],
        "actor_work"
    );
    assert_eq!(
        payload(&decision)["current_responsibility"]["tasks"][0]["actor_id"],
        "worker-b"
    );
    let next_task_id = payload(&decision)["relay"]["next_task_id"]
        .as_str()
        .expect("next task id")
        .to_owned();

    let replay = tool(&home, &group, "cccc_coordination", args).await;
    assert_eq!(payload(&replay)["replayed"], true);
    assert_eq!(payload(&replay)["relay"]["next_task_id"], next_task_id);

    let context = cccc_core::context::ContextStore::new(home.clone())
        .expect("context")
        .load(&group)
        .expect("context document");
    let next_task = context
        .tasks
        .iter()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(next_task_id.as_str()))
        .expect("next task");
    assert_eq!(next_task["status"], "active");
    assert_eq!(next_task["assignee"], "worker-b");
    assert_eq!(next_task["waiting_on"], "actor");

    let events = ledger::read_all(&path).expect("events");
    let next_messages = events
        .iter()
        .filter(|event| {
            event.kind == "chat.message"
                && event.by == "web-lead"
                && event.data["to"] == json!(["worker-b"])
        })
        .collect::<Vec<_>>();
    assert_eq!(
        next_messages.len(),
        1,
        "continue replay duplicated real work"
    );
    assert_eq!(
        next_messages[0].data["text"],
        "Run every affected Rust and browser-delivery test. Report the exact commands, failures, remaining risk, and requested next action."
    );
    assert_delivery_accepted(&events, &report);
    stop_daemon(&client, daemon_task).await;
}
