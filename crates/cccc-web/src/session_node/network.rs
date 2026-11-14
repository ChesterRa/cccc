use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cccc_contracts::session_node::{
    SESSION_NODE_ALPN, SESSION_NODE_PROTOCOL, SESSION_NODE_RELAY_TICKET, SessionNodeEndpoint,
    SessionNodeTicket,
};
use cccc_core::{HomeLayout, session_nodes};
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};
use std::net::{Ipv4Addr, SocketAddr};

pub(super) fn key(home: &HomeLayout, role: &str) -> Result<SecretKey> {
    let encoded = session_nodes::identity(home, role, || {
        URL_SAFE_NO_PAD.encode(SecretKey::generate().to_bytes())
    })?;
    let bytes: [u8; 32] = URL_SAFE_NO_PAD
        .decode(encoded)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid session-node key"))?;
    Ok(SecretKey::from_bytes(&bytes))
}

pub(super) fn builder(
    key: SecretKey,
    port: u16,
    relay: Option<&str>,
    token: Option<&str>,
) -> Result<iroh::endpoint::Builder> {
    let mut builder = Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .alpns(vec![SESSION_NODE_ALPN.to_vec()])
        .clear_ip_transports()
        .bind_addr((Ipv4Addr::UNSPECIFIED, port))?;
    if let Some(relay) = relay {
        let relay = session_nodes::normalize_relay_url(relay)?;
        let mut config = iroh::RelayConfig::new(relay.parse()?, None);
        if let Some(token) = token {
            config = config.with_auth_token(token);
        }
        builder = builder.relay_mode(iroh::RelayMode::Custom(iroh::RelayMap::from_iter([config])));
    }
    Ok(builder)
}

pub(super) fn address(endpoint: &Endpoint) -> Result<SessionNodeEndpoint> {
    let port = endpoint
        .bound_sockets()
        .into_iter()
        .find(|addr| addr.is_ipv4())
        .context("LAN UDP socket is unavailable")?
        .port();
    let mut addresses = cccc_core::local_network::addresses()?
        .into_iter()
        .filter(|addr| addr.ip.is_ipv4())
        .map(|addr| SocketAddr::new(addr.ip, port).to_string())
        .collect::<Vec<_>>();
    // This also supports two isolated Homes on one host without advertising a
    // public relay. Other machines use the actual interface addresses above.
    addresses.push(SocketAddr::from((Ipv4Addr::LOCALHOST, port)).to_string());
    Ok(SessionNodeEndpoint {
        endpoint_id: endpoint.id().to_string(),
        addresses,
        relay_url: None,
    })
}

pub(super) fn destination(endpoint: &SessionNodeEndpoint) -> Result<EndpointAddr> {
    ensure!(
        (!endpoint.addresses.is_empty() || endpoint.relay_url.is_some())
            && endpoint.addresses.len() <= 32,
        "Invalid LAN endpoint addresses"
    );
    let mut addr = EndpointAddr::new(endpoint.endpoint_id.parse()?);
    for raw in &endpoint.addresses {
        let socket: SocketAddr = raw.parse()?;
        ensure!(
            !socket.ip().is_unspecified() && !socket.ip().is_multicast() && socket.port() != 0,
            "Invalid LAN address"
        );
        addr = addr.with_ip_addr(socket);
    }
    if let Some(relay) = &endpoint.relay_url {
        addr = addr.with_relay_url(session_nodes::normalize_relay_url(relay)?.parse()?);
    }
    Ok(addr)
}

pub fn encode(ticket: &SessionNodeTicket) -> Result<String> {
    Ok(format!(
        "cccc-session-node:{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(ticket)?)
    ))
}

pub fn decode(value: &str) -> Result<SessionNodeTicket> {
    ensure!(value.len() <= 16 * 1024, "Session-node ticket is too large");
    let encoded = value
        .trim()
        .strip_prefix("cccc-session-node:")
        .context("Expected a session-node pairing ticket, not a Group invitation")?;
    let ticket: SessionNodeTicket = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded)?)?;
    ensure!(
        matches!(ticket.v, SESSION_NODE_PROTOCOL | SESSION_NODE_RELAY_TICKET)
            && (ticket.v == SESSION_NODE_RELAY_TICKET) == ticket.relay_url.is_some()
            && ticket.relay_url.is_some() == ticket.relay_token.is_some(),
        "Unsupported session-node protocol version"
    );
    ensure!(
        !ticket.secret.is_empty()
            && !ticket.invite_id.is_empty()
            && ticket.relay_token.as_ref().is_none_or(
                |token| token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
            ),
        "Incomplete session-node ticket"
    );
    destination(&SessionNodeEndpoint {
        endpoint_id: ticket.endpoint_id.clone(),
        addresses: ticket.addresses.clone(),
        relay_url: ticket.relay_url.clone(),
    })?;
    Ok(ticket)
}
