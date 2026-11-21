//! Real production connect/reconnect builder and CCCC Hello over the test-owned network.
use super::super::{network, protocol};
use super::proxy_fixture as wire;
use cccc_contracts::session_node::*;
use iroh::SecretKey;
use std::time::Duration;
use tokio::io::AsyncReadExt;

pub(super) async fn run(case: &str) {
    let mut input = Vec::new();
    tokio::io::stdin()
        .read_to_end(&mut input)
        .await
        .expect("fixture input");
    let ticket: SessionNodeTicket = serde_json::from_slice(&input).expect("synthetic ticket");
    let token = format!(
        "cccc-session-node:{}:{}",
        ticket.invite_id,
        ticket.relay_token.as_deref().expect("capability")
    );
    let key = SecretKey::generate();
    let unsupported = case == "udp";
    let mut route = SessionNodeEndpoint {
        endpoint_id: ticket.endpoint_id.clone(),
        addresses: vec![],
        relay_url: ticket.relay_url.clone(),
    };
    if unsupported {
        route.addresses = ticket.addresses.clone();
    }
    let (builder, destination) = network::connect_builder(key.clone(), &route, Some(&token))
        .expect("production connect route");
    // Captive-portal detection uses fixed HTTP port 80. Keep this test entirely
    // on its owned ports while retaining HTTPS probes and actual relay traffic.
    let mut probes = iroh::NetReportConfig::default();
    probes.captive_portal_check = false;
    let builder = builder
        .dns_resolver(wire::dns())
        .ca_tls_config(wire::ca())
        .net_report_config(probes.clone());
    let endpoint = if unsupported {
        builder
    } else {
        builder.clear_ip_transports()
    }
    .bind()
    .await
    .expect("production endpoint");
    // HTTPS proxy and NO_PROXY need the real native relay handshake. HTTP below
    // covers QUIC + CCCC enrollment and the single complete reconnect.
    if matches!(case, "https" | "bypass") {
        tokio::time::timeout(Duration::from_secs(15), endpoint.online())
            .await
            .expect("real relay authentication deadline");
        endpoint.close().await;
        return;
    }
    let connect = async || {
        tokio::time::timeout(
            Duration::from_secs(15),
            endpoint.connect(destination.clone(), SESSION_NODE_ALPN),
        )
        .await
        .expect("connection deadline")
        .expect("real relay connection")
    };
    let connection = connect().await;
    let hello = protocol::call(
        &connection,
        &protocol::Request::Hello {
            version: SESSION_NODE_PROTOCOL,
            name: "Proxy fixture".into(),
            invite_id: Some(ticket.invite_id),
            secret: Some(ticket.secret),
        },
    )
    .await
    .expect("authorized Hello");
    let grant = hello["peer"]["grant_id"].clone();
    assert!(!grant.as_str().expect("grant").is_empty());
    connection.close(0u32.into(), b"reconnect fixture");
    endpoint.close().await;
    if case != "http" {
        return;
    }
    let (builder, destination) =
        network::connect_builder(key, &route, None).expect("production reconnect route");
    let builder = builder
        .dns_resolver(wire::dns())
        .ca_tls_config(wire::ca())
        .net_report_config(probes);
    let resumed = builder
        .clear_ip_transports()
        .bind()
        .await
        .expect("resume endpoint");
    let connection = tokio::time::timeout(
        Duration::from_secs(15),
        resumed.connect(destination, SESSION_NODE_ALPN),
    )
    .await
    .expect("resume deadline")
    .expect("resume transport");
    let hello = protocol::call(
        &connection,
        &protocol::Request::Hello {
            version: SESSION_NODE_PROTOCOL,
            name: "Proxy fixture".into(),
            invite_id: None,
            secret: None,
        },
    )
    .await
    .expect("resume authority");
    assert_eq!(
        hello["peer"]["grant_id"], grant,
        "reconnect retains the device grant"
    );
    resumed.close().await;
}
