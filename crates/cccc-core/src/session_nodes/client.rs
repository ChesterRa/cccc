use super::store;
use crate::{HomeLayout, fs, local_sessions::NativeHost};
use cccc_contracts::session_node::{SessionNodeEndpoint, SessionNodeMembership};
use serde::{Deserialize, Serialize};
use std::{io, net::SocketAddr};

/// Local-only descriptor. The broker secret never crosses the iroh connection.
#[derive(Clone, Serialize, Deserialize)]
pub struct ClientDescriptor {
    pub endpoint: SessionNodeEndpoint,
    pub node_id: String,
    pub grant_id: String,
    pub owner: NativeHost,
    pub address: SocketAddr,
    pub secret: String,
    pub connected: bool,
    pub memberships: Vec<SessionNodeMembership>,
}

pub fn saved_client(home: &HomeLayout) -> io::Result<Option<ClientDescriptor>> {
    let path = store::directory(home).join("client.json");
    if !path.exists() {
        return Ok(None);
    }
    fs::read_json(&path).map(Some)
}

pub fn client(home: &HomeLayout) -> io::Result<Option<ClientDescriptor>> {
    Ok(saved_client(home)?.filter(|client| {
        client.owner.alive() && client.connected && client.address.ip().is_loopback()
    }))
}

pub fn set_client(home: &HomeLayout, descriptor: &ClientDescriptor) -> io::Result<()> {
    if !descriptor.address.ip().is_loopback() {
        return Err(store::denied());
    }
    store::update(home, |_| {
        if saved_client(home)?.is_some_and(|old| old.owner.alive() && old.owner != descriptor.owner)
        {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "Another remote-session connector owns this Home",
            ));
        }
        fs::write_secret_json(&store::directory(home).join("client.json"), descriptor)
    })
}

pub fn update_memberships(
    home: &HomeLayout,
    secret: &str,
    connected: bool,
    memberships: Vec<SessionNodeMembership>,
) -> io::Result<()> {
    if memberships.len() > 2048 {
        return Err(io::Error::other("Too many remote memberships"));
    }
    store::update(home, |_| {
        let mut current = saved_client(home)?
            .filter(|client| client.owner.alive() && client.secret == secret)
            .ok_or_else(store::denied)?;
        if memberships
            .iter()
            .any(|membership| membership.hub_id != current.endpoint.endpoint_id)
        {
            return Err(store::denied());
        }
        current.connected = connected;
        current.memberships = memberships;
        fs::write_secret_json(&store::directory(home).join("client.json"), &current)
    })
}

pub fn clear_client(home: &HomeLayout, secret: &str) -> io::Result<()> {
    store::update(home, |_| {
        if saved_client(home)?.is_some_and(|client| client.secret == secret) {
            std::fs::remove_file(store::directory(home).join("client.json"))?;
        }
        Ok(())
    })
}
