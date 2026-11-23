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
    pub gateway_stopped: bool,
    pub relay_enabled: bool,
    // Expire development tickets written before browser approval was removed.
    // A previously identity-bound ticket must never become a bearer ticket.
    #[serde(rename = "pairing_tickets")]
    pub invites: BTreeMap<String, Invitation>,
    pub peers: BTreeMap<String, SessionNodePeer>,
    pub sources: BTreeMap<String, RemoteSessionSource>,
}

pub(super) fn directory(home: &HomeLayout) -> PathBuf {
    home.root().join("state/session_nodes")
}

pub(super) fn read(home: &HomeLayout) -> io::Result<State> {
    Ok(read_file(home, "state.json")?.unwrap_or_default())
}

/// Safe diagnostic context for device state reads, separate from authority failures.
#[derive(Debug)]
pub struct StateReadError {
    name: &'static str,
    kind: io::ErrorKind,
}

impl std::fmt::Display for StateReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Could not read state/session_nodes/{} ({:?})",
            self.name, self.kind
        )
    }
}

impl std::error::Error for StateReadError {}

pub(super) fn read_file<T: serde::de::DeserializeOwned>(
    home: &HomeLayout,
    name: &'static str,
) -> io::Result<Option<T>> {
    let path = directory(home).join(name);
    match fs::read_json(&path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            let kind = if error
                .get_ref()
                .is_some_and(|cause| cause.is::<serde_json::Error>())
            {
                io::ErrorKind::InvalidData
            } else {
                error.kind()
            };
            Err(io::Error::new(kind, StateReadError { name, kind }))
        }
    }
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

pub fn gateway_stopped(home: &HomeLayout) -> io::Result<bool> {
    Ok(read(home)?.gateway_stopped)
}

pub fn relay_enabled(home: &HomeLayout) -> io::Result<bool> {
    Ok(read(home)?.relay_enabled)
}

pub fn set_relay_enabled(home: &HomeLayout, enabled: bool) -> io::Result<()> {
    update(home, |state| {
        state.relay_enabled = enabled;
        if !enabled {
            if let Some(hub) = &mut state.hub {
                hub.relay_address = None;
            }
            for invite in state.invites.values_mut() {
                invite.relay_node_id = None;
                invite.relay_secret_hash = None;
            }
        }
        Ok(())
    })
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
            clear_live_gateway(state);
        }
        Ok(())
    })
}

/// Persist an administrator stop while retaining all existing device grants.
pub fn stop_gateway(home: &HomeLayout, token: Option<&str>) -> io::Result<()> {
    update(home, |state| {
        if state.hub.as_ref().is_some_and(|hub| {
            hub.owner.alive() && token.is_none_or(|token| hub.token_hash != hash(token))
        }) {
            return Err(denied());
        }
        state.gateway_stopped = true;
        state.invites.clear();
        clear_live_gateway(state);
        Ok(())
    })
}

fn clear_live_gateway(state: &mut State) {
    state.hub = None;
    for peer in state.peers.values_mut() {
        peer.connected = false;
    }
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
