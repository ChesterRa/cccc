use super::fixture::{Fixture, call, command, until};
use cccc_core::session_nodes;
use serde_json::json;
use std::process::Stdio;

pub async fn verify(f: &mut Fixture, group_id: &str, hub_id: &str, alias: &str) {
    let before = session_nodes::endpoint(&f.hub)
        .expect("endpoint")
        .expect("live gateway");
    let peer = session_nodes::peers(&f.hub).expect("peers").remove(0);
    cccc_web::session_node::stop(&f.hub).await;
    assert!(
        session_nodes::source(&f.hub, alias).is_err(),
        "No gateway means no live remote authority"
    );
    cccc_web::session_node::restore(&f.hub)
        .await
        .expect("restore paired gateway");
    let after = session_nodes::endpoint(&f.hub)
        .expect("restored endpoint")
        .expect("gateway");
    assert_eq!(after.endpoint_id, before.endpoint_id);
    assert_eq!(
        after
            .addresses
            .last()
            .expect("restored address")
            .parse::<std::net::SocketAddr>()
            .expect("socket")
            .port(),
        before
            .addresses
            .last()
            .expect("original address")
            .parse::<std::net::SocketAddr>()
            .expect("socket")
            .port(),
        "A gateway restart preserves the cached UDP route"
    );
    until(async || {
        session_nodes::peers(&f.hub)
            .expect("peers")
            .iter()
            .any(|current| current.node_id == peer.node_id && current.connected)
    })
    .await;

    // A normal connector stop is observable without waiting for an arbitrary
    // UDP failure timeout. The native host and memberships remain in place.
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(f.bridge.id().expect("own bridge PID") as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .expect("stop own bridge");
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(10), f.bridge.wait())
            .await
            .expect("A single exit signal must stop the connector, including during Hello")
            .expect("bridge exit")
            .success()
    );
    until(async || {
        session_nodes::peers(&f.hub)
            .expect("peers")
            .iter()
            .all(|current| current.node_id != peer.node_id || !current.connected)
    })
    .await;
    f.bridge = command(f._temp.path(), &f.node)
        .args(["sessions", "node", "connect", "--name", "Test laptop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("resume connector with its saved key");
    until(async || {
        call(&f.hub_client, "local_session_list", json!({})).await["sources"]
            .as_array()
            .is_some_and(|sources| sources.iter().any(|source| source["source_id"] == alias))
    })
    .await;
    until(async || {
        session_nodes::saved_client(&f.node)
            .expect("local descriptor")
            .is_some_and(|client| {
                client.connected
                    && client.owner.alive()
                    && client
                        .memberships
                        .iter()
                        .any(|member| member.group_id == group_id)
            })
    })
    .await;
    let current = session_nodes::peers(&f.hub).expect("peers").remove(0);
    assert_eq!(
        current.grant_id, peer.grant_id,
        "Resume is not new admission"
    );
    let bootstrap = f
        .mcp(
            "cccc_bootstrap",
            json!({"group_id":group_id,"hub_id":hub_id}),
        )
        .await;
    assert_ne!(bootstrap["result"]["isError"], true, "{bootstrap}");
    assert!(f.source.host.alive());
    assert_eq!(
        std::fs::read_to_string(f._temp.path().join("codex/calls")).expect("queue calls"),
        "submitted\n",
        "Accepted native work is not replayed by either restart"
    );
}
