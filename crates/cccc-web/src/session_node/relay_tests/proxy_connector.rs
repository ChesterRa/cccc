//! Real production connect/reconnect builder and CCCC Hello over the test-owned network.
use super::super::{client_dial::Dialer, network, protocol};
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
    // The first bind cannot trust the fixture CA, and its only IP candidate is a
    // test-owned UDP sink. The replacement uses the same key, relay and HTTP proxy.
    let sink = tokio::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("UDP sink");
    route
        .addresses
        .push(sink.local_addr().expect("UDP address").to_string());
    let configure = || {
        let (builder, destination) = network::connect_builder(key.clone(), &route, None)?;
        let builder = builder
            .dns_resolver(wire::dns())
            .net_report_config(probes.clone());
        Ok::<_, anyhow::Error>((builder, destination))
    };
    let mut resumed = Dialer::bind(configure().expect("initial relay configuration"))
        .await
        .expect("resume endpoint");
    let connection = tokio::time::timeout(
        Duration::from_secs(45),
        resumed.connect(
            || {
                let (builder, destination) = configure()?;
                Ok((builder.ca_tls_config(wire::ca()), destination))
            },
            false,
        ),
    )
    .await
    .expect("fallback deadline")
    .expect("a fresh relay dial recovers the connection");
    assert!(
        connection
            .paths()
            .iter()
            .any(|p| p.is_selected() && p.is_relay()),
        "the resumed QUIC connection used the real relay"
    );
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
