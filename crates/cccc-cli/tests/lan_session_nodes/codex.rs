//! The executable is a narrow native queue boundary, not evidence about a live
//! Codex account. iroh, membership, daemon delivery and native MCP stay real.
use super::fixture::{Fixture, call, until};
use cccc_contracts::local_session::LocalSessionRuntime;
use cccc_core::{GroupStore, ledger, local_sessions};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

pub async fn verify(f: &Fixture, group_id: &str, hub_id: &str) {
    let provider = f._temp.path().join("codex");
    std::fs::create_dir(&provider).expect("private provider");
    let program = provider.join("codex-queue-fixture");
    std::fs::write(&program, "#!/bin/sh\n[ \"$1\" = queue ] || exit 1\n[ \"$2 $3 $4 $5 $6\" = '--remote unix:// --thread lan-native-thread --message' ] || exit 2\nprintf 'submitted\\n' >> \"$CODEX_HOME/calls\"\nprintf '%s' \"$7\" > \"$CODEX_HOME/payload\"\n").expect("queue fixture");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("executable");
    let mut source = f.source.clone();
    source.runtime = LocalSessionRuntime::Codex;
    source.source_id = "codex:lan-native-thread".into();
    source.native_session_id = "lan-native-thread".into();
    source.codex_home = Some(provider.clone());
    source.codex_program = Some(program);
    source.codex_can_receive = true;
    source.claude_config_dir = None;
    local_sessions::write(&f.node, source.clone()).expect("native Codex lifecycle");
    let mut alias = String::new();
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let directory = call(&f.hub_client, "local_session_list", json!({})).await;
            if let Some(found) = directory["sources"]
                .as_array()
                .expect("sources")
                .iter()
                .find(|s| s["runtime"] == "codex")
            {
                alias = found["source_id"].as_str().expect("alias").into();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("Codex remote discovery");
    call(
        &f.hub_client,
        "local_session_join",
        json!({"group_id":group_id,"source_ids":[alias],"by":"user"}),
    )
    .await;
    let store = GroupStore::new(f.hub.clone()).expect("store");
    let group = store.load(group_id).expect("Group");
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.external_session_id == alias)
        .expect("Codex member");
    let event = call(&f.hub_client, "message_send", json!({"group_id":group_id,"by":"user","to":[actor.id],"text":"codex-lan-payload","message_mode":"send"})).await;
    let id = event["event"]["id"].as_str().expect("event");
    let path = store.ledger_path(group_id).expect("ledger");
    until(async || {
        ledger::read_all(&path)
            .expect("events")
            .iter()
            .any(|event| {
                event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == id
                    && event.data["state"] == "accepted"
            })
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(provider.join("calls")).expect("native queue calls"),
        "submitted\n"
    );
    assert!(
        std::fs::read_to_string(provider.join("payload"))
            .expect("native queue payload")
            .contains("codex-lan-payload")
    );
    until(async || {
        local_sessions::validate_source(&f.node, &source.source_id, &source.revision).is_ok()
            && cccc_core::session_nodes::saved_client(&f.node)
                .expect("client")
                .is_some_and(|client| {
                    client.memberships.iter().any(|member| {
                        member.source_id == source.source_id && member.group_id == group_id
                    })
                })
    })
    .await;
    let reply = f.mcp_for(&source, "cccc_message_reply", json!({"group_id":group_id,"hub_id":hub_id,"event_id":id,"to":["user"],"text":"codex-remote-reply"})).await;
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    assert!(
        ledger::read_all(&path)
            .expect("reply events")
            .iter()
            .any(|event| event.by == actor.id
                && event.data.get("text") == Some(&json!("codex-remote-reply")))
    );
}
