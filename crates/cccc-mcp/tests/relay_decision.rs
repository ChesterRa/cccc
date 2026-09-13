mod support;

use cccc_contracts::{Actor, ActorRuntime, Event};
use cccc_core::{GroupStore, HomeLayout, actors, ledger};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use support::{payload, start_daemon, stop_daemon};

async fn tool(home: &HomeLayout, group: &str, name: &str, arguments: Value) -> Value {
    let request = json!({
        "jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":name,"arguments":arguments}
    });
    cccc_mcp::handle_request_for_actor(home, &request, group, "web-lead").await
}

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

#[tokio::test]
async fn mcp_relay_handoff_lifecycle_read_reply_decide_and_dispatch_one_real_task() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (home, group, path) = group_with_web_lead(&temp, "relay MCP", &["worker-a", "worker-b"]);
    let report = seed_pending_handoff(
        &path,
        &group,
        "worker-a",
        "The implementation is ready. The user must choose the release window.",
    );
    let (daemon_task, client) = start_daemon(&home).await;

    // The actor-scoped MCP catalog must still declare the decide action.
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

    // Reading exposes the pending handoff without resolving it.
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

    // A wait_user decision resolves the handoff without duplicating human-facing output.
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
    let resolved = tool(&home, &group, "cccc_bootstrap", json!({})).await;
    assert_eq!(payload(&resolved)["relay_pending"]["count"], 0);
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

    // Interface contract: one real task/message and exact replay. Task-state details are tested directly in the daemon.
    let follow_up = seed_pending_handoff(
        &path,
        &group,
        "worker-a",
        "Implementation is complete. Run the full affected regression next.",
    );
    let args = json!({
        "action":"decide","event_ids":[follow_up],"decision":"continue",
        "next_actor_id":"worker-b","next_title":"Run the affected regression",
        "next_text":"Run every affected Rust and browser-delivery test. Report the exact commands, failures, remaining risk, and requested next action.",
        "outcome":"All affected checks pass with evidence"
    });
    let dispatch = tool(&home, &group, "cccc_coordination", args.clone()).await;
    assert_eq!(
        payload(&dispatch)["relay"]["decision"],
        "continue",
        "{dispatch}"
    );
    assert_eq!(payload(&dispatch)["caller_may_idle"], true);
    assert_eq!(payload(&dispatch)["safe_to_idle"], false);
    let next_task_id = payload(&dispatch)["relay"]["next_task_id"]
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
    assert_eq!(
        context.tasks.len(),
        1,
        "continue must create exactly one task"
    );
    assert_eq!(context.tasks[0]["id"], next_task_id);
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
    assert_delivery_accepted(&events, &follow_up);
    stop_daemon(&client, daemon_task).await;
}
