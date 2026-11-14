use super::super::{
    network,
    protocol::{self, Request},
};
use super::fixture::{Fixture, advertised};
use cccc_contracts::{Actor, ActorRuntime, session_node::*};
use cccc_core::{GroupStore, actors, ledger, session_nodes};
use futures_util::StreamExt;
use iroh::SecretKey;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_only_facade_enrolls_transfers_a_real_tool_write_resumes_and_revokes() {
    let f = Fixture::new().await;
    let mut invalid = f.ticket.clone();
    invalid.v = SESSION_NODE_PROTOCOL;
    assert!(
        network::decode(&network::encode(&invalid).expect("encode")).is_err(),
        "Legacy tickets cannot smuggle relay routes"
    );
    let key = SecretKey::generate();
    let node_id = key.public().to_string();
    let endpoint = f.endpoint(key.clone(), Some(&f.ticket)).await;
    let (connection, hello) = f.hello(&endpoint, Some(&f.ticket)).await;
    assert_eq!(hello["peer"]["node_id"], node_id);
    let grant = hello["peer"]["grant_id"].clone();
    protocol::call(
        &connection,
        &Request::Sync {
            sources: vec![advertised()],
        },
    )
    .await
    .expect("source sync");
    let source = session_nodes::list_sources(&f.home)
        .expect("directory")
        .remove(0);
    let store = GroupStore::new(f.home.clone()).expect("store");
    let mut group = store.create("Relay isolated Group", "").expect("Group");
    let mut actor = Actor::new("remote-member");
    actor.runtime = ActorRuntime::Codex;
    actor.external_session_id = source.source.source_id.clone();
    actors::add(&mut group, actor).expect("explicit membership");
    store.save(&group).expect("save");
    let text = format!("relay-only-write:{}", "x".repeat(64 * 1024));
    let request = Request::Tool {
        source_id: "codex:relay-source".into(),
        revision: "relay-revision".into(),
        request: json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"cccc_message_send","arguments":{"group_id":group.group_id,"to":["user"],"text":text,"mode":"send"}}}),
    };
    let result = protocol::call(&connection, &request)
        .await
        .expect("tool response crosses relay");
    assert_ne!(result["result"]["isError"], true, "{result}");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let writes = || {
        ledger::read_all(&path)
            .expect("events")
            .iter()
            .filter(|event| {
                event
                    .data
                    .get("text")
                    .and_then(|text| text.as_str())
                    .is_some_and(|text| text.starts_with("relay-only-write:"))
            })
            .count()
    };
    assert_eq!(writes(), 1, "Only one write is committed");
    connection.close(0u32.into(), b"explicit fixture reconnect");
    endpoint.close().await;
    let resumed = f.endpoint(key.clone(), None).await;
    let (_connection, hello) = f.hello(&resumed, None).await;
    assert_eq!(
        hello["peer"]["grant_id"], grant,
        "Relay reconnect is not new admission"
    );
    assert_eq!(
        writes(),
        1,
        "Reconnect must not replay an application write"
    );
    assert!(resumed.bound_sockets().is_empty());
    let mut relay_connection = super::fixture::relay_client(
        &resumed,
        f.ticket.relay_url.as_deref().expect("relay"),
        key.clone(),
    )
    .connect()
    .await
    .expect("existing relay connection");
    super::super::hub::revoke(&f.home, &node_id)
        .await
        .expect("revoke");
    // Hard relay removal may discard a best-effort QUIC close packet. Observe
    // access loss at the relay instead of asserting a QUIC idle-close deadline.
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while matches!(relay_connection.next().await, Some(Ok(_))) {}
    })
    .await
    .expect("revocation terminates the existing relay stream");
    assert!(session_nodes::source(&f.home, &source.source.source_id).is_err());
    let denied =
        super::fixture::relay_client(&resumed, f.ticket.relay_url.as_deref().expect("relay"), key)
            .connect()
            .await;
    assert!(
        denied
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string().contains("Session-node relay authority")),
        "Unexpected revoked-device outcome: {denied:?}"
    );
    assert_eq!(writes(), 1);
    resumed.close().await;
    f.close().await;
}
