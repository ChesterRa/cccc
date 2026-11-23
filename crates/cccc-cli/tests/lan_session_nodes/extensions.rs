//! Pi and OpenCode use an inert extension caller at the provider boundary.
use super::fixture::{Fixture, call, command, until};
use cccc_contracts::local_session::LocalSessionRuntime;
use cccc_core::{GroupStore, ledger, local_sessions};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn verify(f: &Fixture, group_id: &str, hub_id: &str) {
    let root = f._temp.path();
    let host = &f.source.host;
    let mut extensions = Vec::new();
    for (runtime, native_id) in [
        (
            LocalSessionRuntime::Pi,
            "e089549e-2415-4347-861f-48dc3fb68ecc",
        ),
        (LocalSessionRuntime::Opencode, "ses_lan_fixture"),
    ] {
        let pid = host.pid.to_string();
        let mut runner = command(root, &f.node);
        runner.args([
            "sessions",
            "extension-bridge",
            "--runtime",
            runtime.as_str(),
            "--host-pid",
            &pid,
            "--session-id",
            native_id,
        ]);
        runner
            .arg("--cwd")
            .arg(root)
            .arg("--provider-home")
            .arg(root.join("provider"));
        let key = if runtime == LocalSessionRuntime::Opencode {
            runner.args(["--backend-pid", &pid]);
            native_id.to_owned()
        } else {
            host.key()
        };
        let mut bridge = runner
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("native extension bridge");
        let mut lines = BufReader::new(bridge.stdout.take().expect("extension out")).lines();
        let ready = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
            .await
            .expect("extension ready deadline")
            .expect("extension line")
            .expect("ready line");
        assert_eq!(
            serde_json::from_str::<Value>(&ready).expect("ready JSON"),
            json!({"type":"ready"})
        );
        let source =
            local_sessions::read(&f.node, &runtime.source_id(&key)).expect("native identity");
        extensions.push((source, bridge, lines));
    }

    let mut aliases = Vec::new();
    until(async || {
        let rows = call(
            &f.hub_client,
            "session_node_source_list",
            json!({"by":"user"}),
        )
        .await;
        aliases = extensions
            .iter()
            .filter_map(|(source, _, _)| {
                rows["sources"]
                    .as_array()
                    .expect("device directory")
                    .iter()
                    .find(|row| row["runtime"] == source.runtime.as_str())
                    .map(|row| row["source_id"].clone())
            })
            .collect();
        aliases.len() == extensions.len()
    })
    .await;
    call(
        &f.hub_client,
        "local_session_join",
        json!({"group_id":group_id,"source_ids":aliases,"by":"user"}),
    )
    .await;
    let groups = GroupStore::new(f.hub.clone()).expect("store");
    let group = groups.load(group_id).expect("Group");
    let path = groups.ledger_path(group_id).expect("ledger");
    assert!(group.scopes.is_empty(), "Device paths remain on the node");

    for ((source, mut bridge, mut lines), alias) in extensions.into_iter().zip(aliases) {
        let actor = group
            .actors
            .iter()
            .find(|actor| alias.as_str() == Some(actor.external_session_id.as_str()))
            .expect("extension Actor");
        assert_eq!(actor.runtime.wire_name(), source.runtime.as_str());
        let sent = call(&f.hub_client, "message_send", json!({"group_id":group_id,"by":"user","to":[actor.id],"text":"extension-lan-input","message_mode":"send"})).await;
        let event_id = sent["event"]["id"].as_str().expect("event ID");
        let next = tokio::time::timeout(Duration::from_secs(15), lines.next_line())
            .await
            .expect("remote input deadline")
            .expect("input line")
            .expect("native input");
        let entry: Value = serde_json::from_str(&next).expect("extension handoff");
        let handoff = &entry["handoff"];
        assert_eq!(handoff["event_ids"], json!([event_id]));
        assert_eq!(handoff["native_session_id"], source.native_session_id);
        assert!(
            handoff["text"]
                .as_str()
                .expect("native text")
                .contains(hub_id)
        );
        let ack = json!({"handoff_id":handoff["handoff_id"],"state":"accepted"});
        bridge
            .stdin
            .as_mut()
            .expect("extension input")
            .write_all(format!("{ack}\n").as_bytes())
            .await
            .expect("native receipt");
        let accepted = || {
            ledger::read_all(&path)
                .expect("delivery ledger")
                .into_iter()
                .filter(|event| {
                    event.kind == "runtime.delivery"
                        && event.data["source_event_id"] == event_id
                        && event.data["state"] == "accepted"
                })
                .count()
        };
        until(async || accepted() == 1).await;

        let listing = f
            .mcp_for(&source, "cccc_session", json!({"action":"list"}))
            .await;
        let list = listing["result"]["structuredContent"]["self_memberships"]
            .as_array()
            .expect("native memberships");
        assert!(list.iter().any(|row| row["hub_id"] == hub_id
            && row["group_id"] == group_id
            && row["actor_id"] == actor.id));
        let mut args = json!({"group_id":group_id,"event_id":event_id,"to":["user"],"text":"extension-lan-reply"});
        let local = f.mcp_for(&source, "cccc_message_reply", args.clone()).await;
        assert_eq!(
            local["result"]["isError"], true,
            "The device Group must be selected with hub_id"
        );
        args["hub_id"] = json!(hub_id);
        let reply = f.mcp_for(&source, "cccc_message_reply", args).await;
        assert_ne!(reply["result"]["isError"], true, "{reply}");
        assert!(
            ledger::read_all(&path)
                .expect("reply ledger")
                .iter()
                .any(|event| {
                    event.kind == "chat.message"
                        && event.by == actor.id
                        && event.data["reply_to"] == event_id
                })
        );
        assert_eq!(accepted(), 1);
        drop(bridge.stdin.take());
        assert!(
            tokio::time::timeout(Duration::from_secs(10), bridge.wait())
                .await
                .expect("extension exit")
                .expect("bridge status")
                .success()
        );
    }
}
