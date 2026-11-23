use super::{
    PROMPT,
    homes::{Homes, output, paste, progress},
    ticket,
};
use cccc_contracts::session_node::SessionNodeEndpoint;
use cccc_core::{local_sessions::NativeHost, session_nodes};

#[tokio::test]
async fn private_input_stays_local() {
    let f = Homes::new();
    let (ok, text) = output(f.command(&["--help"]), &[]).await;
    assert!(
        ok && text
            .lines()
            .any(|line| line.trim_start().starts_with("connect "))
    );
    assert!(
        !text
            .lines()
            .any(|line| line.trim_start().starts_with("join "))
    );
    let (ok, text) = output(f.command(&["connect"]), &[]).await;
    assert!(!ok && text.contains("--ticket-stdin"));
    for input in ["", "private-input", &"x".repeat(16 * 1024)] {
        let mut child = f.command(&["connect", "--ticket-stdin", "--runtime", "codex"]);
        progress(&mut child, PROMPT, &["private-input"]).await;
        if !input.is_empty() {
            paste(&mut child, input).await;
        }
        let (ok, text) = output(child, &["private-input"]).await;
        assert!(!ok && text.contains("Invalid or missing session-node pairing ticket"));
    }
    assert!(
        !f.node.root().exists(),
        "invalid input must not create an identity or configure a runtime"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn waiting_for_a_ticket_preserves_a_newer_owner() {
    let mut f = Homes::new();
    f.start().await;
    let (wire, secret) = ticket(&f).await;
    let hub_id = cccc_session_node::decode_ticket(&wire)
        .expect("fixture hub")
        .endpoint_id;
    let live = NativeHost::capture(std::process::id()).expect("fixture owner");
    let mut retired = live.clone();
    retired.started.push_str("-retired");
    let mut old = session_nodes::ClientDescriptor {
        endpoint: SessionNodeEndpoint {
            endpoint_id: hub_id.clone(),
            addresses: vec!["127.0.0.1:1".into()],
            relay_url: None,
        },
        node_id: hub_id.clone(), // Another device key, even though the hub identity matches.
        grant_id: "prior-grant".into(),
        owner: retired,
        address: "127.0.0.1:1".parse().expect("loopback broker"),
        secret: "prior-broker-secret".into(),
        connected: false,
        memberships: vec![],
    };
    session_nodes::set_client(&f.node, &old).expect("saved descriptor from a different device key");
    let mut waiting = f.command(&["connect", "--ticket-stdin", "--runtime", "codex"]);
    let forbidden = [&wire[..], &secret[..]];
    progress(&mut waiting, PROMPT, &forbidden).await;
    assert!(
        session_nodes::peers(&f.hub)
            .expect("before ticket")
            .is_empty()
    );
    old.owner = live;
    old.connected = true;
    old.grant_id = "newer-live-grant".into();
    session_nodes::set_client(&f.node, &old).expect("new live connector");
    let saved = f.descriptor_bytes();
    paste(&mut waiting, &wire).await;
    let (ok, text) = output(waiting, &forbidden).await;
    assert!(
        !ok && text.contains("changed during pairing") && !text.contains("integration enabled")
    );
    let (ok, text) = output(f.command(&["connect", "--ticket-stdin"]), &forbidden).await;
    assert!(!ok && text.contains("already runs") && !text.contains(PROMPT));
    assert!(
        session_nodes::peers(&f.hub)
            .expect("new owner keeps its seat")
            .is_empty()
    );
    assert!(
        f.descriptor_bytes() == saved,
        "a pending or duplicate connector must not replace the newer owner or its grant"
    );
    f.close().await;
}
