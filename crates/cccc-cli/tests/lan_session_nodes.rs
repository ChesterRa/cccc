//! Two isolated Homes and a real iroh connection. Claude's existing Monitor
//! adapter is the provider boundary; no Chrome, real model or user Home is used.
#![cfg(unix)]
#[path = "lan_session_nodes/codex.rs"]
mod codex;
#[path = "lan_session_nodes/fixture.rs"]
mod fixture;
#[path = "lan_session_nodes/native_mcp.rs"]
mod native_mcp;
#[path = "lan_session_nodes/recovery.rs"]
mod recovery;
use cccc_core::{GroupStore, ledger, session_nodes};
use fixture::{Fixture, call, until};
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lan_native_session_receives_replies_and_loses_authority_on_revoke() {
    let mut f = Fixture::start().await;
    let directory = call(&f.hub_client, "local_session_list", json!({})).await;
    let alias = directory["sources"][0]["source_id"]
        .as_str()
        .expect("remote alias")
        .to_owned();
    let store = GroupStore::new(f.hub.clone()).expect("store");
    let mut group = store.create("Existing hub Group", "").expect("Group");
    group.running = true;
    store.save(&group).expect("running Group");
    call(
        &f.hub_client,
        "local_session_join",
        json!({"group_id":group.group_id,"source_ids":[alias],"by":"user"}),
    )
    .await;
    group = store.load(&group.group_id).expect("joined Group");
    assert!(
        group.scopes.is_empty(),
        "Remote cwd must never become a hub filesystem scope"
    );
    let actor = group.actors[0].clone();
    let hub_id = session_nodes::endpoint(&f.hub)
        .expect("hub endpoint")
        .expect("live endpoint")
        .endpoint_id;
    let sent = call(&f.hub_client, "message_send", json!({"group_id":group.group_id,"by":"user","to":[actor.id],"text":"lan-message-token","message_mode":"send"})).await;
    let event_id = sent["event"]["id"]
        .as_str()
        .expect("message event")
        .to_owned();
    let mut notice = String::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        while !notice.contains(&event_id) {
            let line = f
                .lines
                .next_line()
                .await
                .expect("Monitor read")
                .expect("Monitor remains open");
            let record: Value = serde_json::from_str(&line).expect("Monitor JSON");
            notice.push_str(record["message"].as_str().expect("native notice"));
        }
    })
    .await
    .expect("original native receive");
    assert!(
        notice.contains(&hub_id) && notice.contains(&group.group_id),
        "{notice}"
    );
    let path = store.ledger_path(&group.group_id).expect("ledger");
    until(async || {
        ledger::read_all(&path)
            .expect("events")
            .iter()
            .any(|event| {
                event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == event_id
                    && event.data["state"] == "accepted"
            })
    })
    .await;
    let accepted = ledger::read_all(&path)
        .expect("events")
        .into_iter()
        .filter(|event| {
            event.kind == "runtime.delivery"
                && event.data["source_event_id"] == event_id
                && event.data["state"] == "accepted"
        })
        .count();
    assert_eq!(accepted, 1, "one authoritative native delivery receipt");

    // The existing Claude Monitor intentionally sends an exact-read notice on
    // first delivery. Prove that the original native MCP can fetch its body.
    let history = f
        .mcp(
            "cccc_message_history",
            json!({"group_id":group.group_id,"hub_id":hub_id,"event_id":event_id}),
        )
        .await;
    assert_ne!(history["result"]["isError"], true, "{history}");
    assert!(
        history["result"]["structuredContent"]["delivery_text"]
            .as_str()
            .is_some_and(|text| text.contains("lan-message-token")),
        "Exact message must be accessible over the remote native route: {history}"
    );

    let bootstrap = f
        .mcp(
            "cccc_bootstrap",
            json!({"group_id":group.group_id,"hub_id":hub_id}),
        )
        .await;
    assert_ne!(bootstrap["result"]["isError"], true, "{bootstrap}");
    assert_eq!(
        bootstrap["result"]["structuredContent"]["session"]["actor_id"],
        actor.id
    );
    assert_eq!(
        bootstrap["result"]["structuredContent"]["session"]["effective_scope"]["path"],
        f.source.cwd
    );
    let reply = f.mcp("cccc_message_reply", json!({"group_id":group.group_id,"hub_id":hub_id,"event_id":event_id,"to":["user"],"text":"remote-reply-token"})).await;
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    assert!(
        ledger::read_all(&path)
            .expect("reply ledger")
            .iter()
            .any(|event| event.kind == "chat.message"
                && event.by == actor.id
                && event.data["text"] == "remote-reply-token")
    );
    let task = f.mcp("cccc_task", json!({"group_id":group.group_id,"hub_id":hub_id,"action":"create","title":"Remote collaboration task"})).await;
    assert_ne!(task["result"]["isError"], true, "{task}");
    let denied = f.mcp("cccc_file", json!({"group_id":group.group_id,"hub_id":hub_id,"action":"read","rel_path":"README.md"})).await;
    assert_eq!(
        denied["result"]["isError"], true,
        "Remote calls must not access hub files: {denied}"
    );

    codex::verify(&f, &group.group_id, &hub_id).await;
    recovery::verify(&mut f, &group.group_id, &hub_id, &alias).await;
    let peer = session_nodes::peers(&f.hub).expect("peers").remove(0);
    cccc_web::session_node::revoke(&f.hub, &peer.node_id)
        .await
        .expect("revoke device");
    assert!(session_nodes::source(&f.hub, &alias).is_err());
    let rejected = f.mcp("cccc_message_send", json!({"group_id":group.group_id,"hub_id":hub_id,"to":["user"],"text":"must-not-be-written"})).await;
    assert_eq!(rejected["result"]["isError"], true, "{rejected}");
    assert!(
        !ledger::read_all(&path)
            .expect("ledger")
            .iter()
            .any(|event| event.data.get("text") == Some(&json!("must-not-be-written")))
    );
    assert!(
        f.source.host.alive(),
        "Revoking CCCC must not stop the native host"
    );
    f.close().await;
}
