use cccc_client::DaemonClient;
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};
use std::collections::BTreeSet;

async fn tools(home: &HomeLayout, group_id: &str, actor_id: &str) -> Vec<Value> {
    let response = cccc_mcp::handle_request_for_actor(
        home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        group_id,
        actor_id,
    )
    .await;
    response["result"]["tools"]
        .as_array()
        .expect("tools/list")
        .clone()
}
fn names(tools: &[Value]) -> BTreeSet<&str> {
    tools
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect()
}

#[tokio::test]
async fn actor_catalog_keeps_core_routes_with_and_without_a_daemon() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("catalog", "").expect("group");
    for id in ["lead", "peer", "voice-secretary"] {
        cccc_core::actors::add(&mut group, Actor::new(id)).expect("actor");
    }
    for (id, runtime) in [
        ("grok-peer", ActorRuntime::Grok),
        ("pty-peer", ActorRuntime::Antigravity),
        ("web-peer", ActorRuntime::WebModel),
    ] {
        let mut actor = Actor::new(id);
        actor.runtime = runtime;
        actor.normalize_runtime_constraints();
        cccc_core::actors::add(&mut group, actor).expect("runtime profile");
    }
    store.save(&group).expect("save");
    let mut profiles_before = Vec::new();
    for id in ["grok-peer", "pty-peer", "web-peer"] {
        profiles_before.push((id, tools(&home, &group.group_id, id).await));
    }
    let before = tools(&home, &group.group_id, "peer").await;
    let secretary_before = tools(&home, &group.group_id, "voice-secretary").await;
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if client
                .call(&DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");
    let peer = tools(&home, &group.group_id, "peer").await;
    let lead = tools(&home, &group.group_id, "lead").await;
    let secretary = tools(&home, &group.group_id, "voice-secretary").await;
    for (id, before) in profiles_before {
        let after = tools(&home, &group.group_id, id).await;
        assert_eq!(names(&before), names(&after), "runtime profile for {id}");
        assert!(names(&after).contains("cccc_connect"));
        assert_eq!(names(&after).contains("cccc_repo"), id == "web-peer");
        assert!(!names(&after).contains("cccc_actor"));
    }
    daemon.abort();
    let _ = daemon.await;
    for catalog in [&before, &peer, &lead] {
        let names = names(catalog);
        for required in [
            "cccc_connect",
            "cccc_message_deliver",
            "cccc_reply_request_cancel",
        ] {
            assert!(
                names.contains(required),
                "Actor tools/list must expose {required}"
            );
        }
        assert!(!names.contains("cccc_remote_access"));
        let send = catalog
            .iter()
            .find(|t| t["name"] == "cccc_message_send")
            .expect("send");
        for field in ["dst_instance_id", "dst_group_id", "mode", "insight"] {
            assert!(send["inputSchema"]["properties"].get(field).is_some());
        }
        let reply = catalog
            .iter()
            .find(|t| t["name"] == "cccc_message_reply")
            .expect("reply");
        for obsolete in ["priority", "reply_required"] {
            assert!(reply["inputSchema"]["properties"].get(obsolete).is_none());
        }
    }
    assert_eq!(names(&before), names(&peer));
    assert_eq!(names(&secretary_before), names(&secretary));
    assert!(names(&secretary).contains("cccc_voice_secretary_document"));
    assert!(!names(&secretary).contains("cccc_message_send"));
    assert!(!names(&peer).contains("cccc_actor"));
}
