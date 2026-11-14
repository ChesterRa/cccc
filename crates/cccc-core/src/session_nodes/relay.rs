//! Device-key access to the private relay; browser administrator tokens are not relay grants.
use super::store;
use crate::{HomeLayout, settings};
use chrono::Utc;
use std::{collections::BTreeSet, io, net::SocketAddr};

fn invalid_url() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Device relay requires an HTTPS public Web URL at the origin root",
    )
}

pub fn public_url(home: &HomeLayout) -> io::Result<Option<String>> {
    let settings = settings::load(home)?;
    let config = &settings.remote_access;
    if config.get("enabled").and_then(serde_json::Value::as_bool) != Some(true)
        || !matches!(
            config.get("provider").and_then(serde_json::Value::as_str),
            Some("manual" | "reach")
        )
    {
        return Ok(None);
    }
    let raw = config
        .get("web_public_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if raw.is_empty() {
        return Ok(None);
    }
    // Relay support is optional. Existing HTTP or prefixed Web access settings
    // must not disable direct pairing or restoration of already-paired devices.
    Ok(normalize_url(raw).ok())
}

pub fn normalize_url(raw: &str) -> io::Result<String> {
    let mut url = url::Url::parse(raw).map_err(|_| invalid_url())?;
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || url.host().is_none()
        || url.port() == Some(0)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/ui" | "/ui/")
    {
        return Err(invalid_url());
    }
    url.set_path("/");
    Ok(url.to_string())
}

pub fn register(home: &HomeLayout, gateway: &str, address: SocketAddr) -> io::Result<()> {
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(store::denied());
    }
    store::update(home, |state| {
        store::check_gateway(state, gateway)?;
        state.hub.as_mut().ok_or_else(store::denied)?.relay_address = Some(address);
        Ok(())
    })
}

pub fn address(home: &HomeLayout) -> io::Result<Option<SocketAddr>> {
    Ok(store::read(home)?
        .hub
        .filter(|hub| hub.owner.alive())
        .and_then(|hub| hub.relay_address)
        .filter(|address| address.ip().is_loopback() && address.port() != 0))
}

pub fn authorities(home: &HomeLayout, gateway: &str) -> io::Result<BTreeSet<String>> {
    if public_url(home)?.is_none() {
        return Err(store::denied());
    }
    let state = store::read(home)?;
    store::check_gateway(&state, gateway)?;
    let mut nodes = state
        .peers
        .into_values()
        .filter(|peer| peer.enabled)
        .map(|peer| peer.node_id)
        .collect::<BTreeSet<_>>();
    nodes.extend(
        state
            .invites
            .into_values()
            .filter(|invite| valid_deadline(&invite.expires_at))
            .filter_map(|invite| invite.relay_node_id),
    );
    Ok(nodes)
}

/// Called only after iroh proves possession of the connecting endpoint's private key.
/// The first relay use binds an unconsumed invitation to that identity.
pub fn authorize(
    home: &HomeLayout,
    gateway: &str,
    node_id: &str,
    invite: Option<(&str, &str)>,
) -> io::Result<()> {
    if public_url(home)?.is_none() {
        return Err(store::denied());
    }
    let state = store::read(home)?;
    store::check_gateway(&state, gateway)?;
    if state.peers.get(node_id).is_some_and(|peer| peer.enabled) {
        return Ok(());
    }
    let (id, secret) = invite.ok_or_else(store::denied)?;
    store::update(home, |state| {
        store::check_gateway(state, gateway)?;
        let pending = state.invites.get_mut(id).ok_or_else(store::denied)?;
        if pending.relay_secret_hash.as_deref() != Some(store::hash(secret).as_str())
            || !valid_deadline(&pending.expires_at)
            || pending
                .relay_node_id
                .as_deref()
                .is_some_and(|bound| bound != node_id)
        {
            return Err(store::denied());
        }
        pending.relay_node_id = Some(node_id.into());
        Ok(())
    })
}

fn valid_deadline(value: &str) -> bool {
    value
        .parse::<chrono::DateTime<Utc>>()
        .is_ok_and(|deadline| deadline > Utc::now())
}
