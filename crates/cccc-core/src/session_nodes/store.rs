use crate::{HomeLayout, fs, local_sessions::NativeHost};
use cccc_contracts::session_node::{RemoteSessionSource, SessionNodeEndpoint, SessionNodePeer};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Hub {
    pub endpoint: SessionNodeEndpoint,
    pub owner: NativeHost,
    pub token_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_address: Option<std::net::SocketAddr>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Invitation {
    pub secret_hash: String,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_secret_hash: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct State {
    pub keys: BTreeMap<String, String>,
    pub hub: Option<Hub>,
    pub saved_endpoint: Option<SessionNodeEndpoint>,
    pub invites: BTreeMap<String, Invitation>,
    pub peers: BTreeMap<String, SessionNodePeer>,
    pub sources: BTreeMap<String, RemoteSessionSource>,
}

pub(super) fn directory(home: &HomeLayout) -> PathBuf {
    home.root().join("state/session_nodes")
}

pub(super) fn read(home: &HomeLayout) -> io::Result<State> {
    let path = directory(home).join("state.json");
    if !path.exists() {
        return Ok(State::default());
    }
    fs::read_json(&path)
}

pub(super) fn update<T>(
    home: &HomeLayout,
    action: impl FnOnce(&mut State) -> io::Result<T>,
) -> io::Result<T> {
    let root = directory(home);
    std::fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    fs::with_exclusive_lock(&root.join("state.lock"), || {
        let mut state = read(home)?;
        let result = action(&mut state)?;
        fs::write_secret_json(&root.join("state.json"), &state)?;
        Ok(result)
    })
}

pub(super) fn hash(value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(super) fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Session-node authority is unavailable or changed",
    )
}

pub(super) fn check_gateway(state: &State, token: &str) -> io::Result<()> {
    if state
        .hub
        .as_ref()
        .is_some_and(|hub| hub.owner.alive() && hub.token_hash == hash(token))
    {
        Ok(())
    } else {
        Err(denied())
    }
}

pub fn identity(
    home: &HomeLayout,
    role: &str,
    create: impl FnOnce() -> String,
) -> io::Result<String> {
    if !matches!(role, "hub" | "client") {
        return Err(denied());
    }
    update(home, |state| {
        Ok(state.keys.entry(role.into()).or_insert_with(create).clone())
    })
}

pub fn endpoint(home: &HomeLayout) -> io::Result<Option<SessionNodeEndpoint>> {
    Ok(read(home)?
        .hub
        .filter(|hub| hub.owner.alive())
        .map(|hub| hub.endpoint))
}

pub fn saved_endpoint(home: &HomeLayout) -> io::Result<Option<SessionNodeEndpoint>> {
    Ok(read(home)?.saved_endpoint)
}

pub fn register_endpoint(
    home: &HomeLayout,
    endpoint: SessionNodeEndpoint,
    owner: NativeHost,
    token: &str,
) -> io::Result<()> {
    update(home, |state| {
        if state
            .hub
            .as_ref()
            .is_some_and(|old| old.owner.alive() && old.owner != owner)
        {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "Another session-node gateway owns this Home",
            ));
        }
        state.saved_endpoint = Some(endpoint.clone());
        state.hub = Some(Hub {
            endpoint,
            owner,
            token_hash: hash(token),
            relay_address: None,
        });
        for peer in state.peers.values_mut() {
            peer.connected = false;
        }
        Ok(())
    })
}

pub fn clear_endpoint(home: &HomeLayout, token: &str) -> io::Result<()> {
    update(home, |state| {
        if state
            .hub
            .as_ref()
            .is_some_and(|hub| hub.token_hash == hash(token))
        {
            state.hub = None;
            for peer in state.peers.values_mut() {
                peer.connected = false;
            }
        }
        Ok(())
    })
}

pub fn peers(home: &HomeLayout) -> io::Result<Vec<SessionNodePeer>> {
    let state = read(home)?;
    let live = state.hub.as_ref().is_some_and(|hub| hub.owner.alive());
    Ok(state
        .peers
        .into_values()
        .map(|mut peer| {
            peer.connected &= live;
            peer
        })
        .collect())
}
