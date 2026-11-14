//! Persistent standalone grants. Reads do not create identities, listeners or jobs.
use crate::{
    GroupStore, HomeLayout, fs,
    instance_identity::{InstanceIdentity, peer_id_from_public_key},
};
#[cfg(test)]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cccc_contracts::{connect::ConnectInstance, direct::*};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::io;
#[path = "direct_address.rs"]
mod addresses;
#[path = "direct_management.rs"]
mod management;
pub use addresses::address;

#[path = "direct_pairing.rs"]
mod pairing;
#[path = "direct_shared.rs"]
pub mod shared;
pub use management::{remove, retire_group, set_state};
use pairing::encode_invitation;
pub use pairing::{hello, invite, invite_checked, join};

#[cfg(test)]
#[path = "direct_tests.rs"]
mod tests;

pub fn load(home: &HomeLayout) -> io::Result<DirectStore> {
    match fs::read_json(&home.root().join("direct_connections.json")) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(DirectStore::default()),
        result => result,
    }
}
pub fn update<T>(
    home: &HomeLayout,
    operation: impl FnOnce(&mut DirectStore) -> io::Result<T>,
) -> io::Result<T> {
    fs::with_exclusive_lock(&home.root().join("direct_connections.lock"), || {
        let mut store = load(home)?;
        let previous = store.clone();
        let result = operation(&mut store)?;
        if store != previous {
            fs::write_secret_json(&home.root().join("direct_connections.json"), &store)?;
        }
        Ok(result)
    })
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
pub fn unexpired(time: &str) -> bool {
    time == DIRECT_PERMANENT_EXPIRY
        || DateTime::parse_from_rfc3339(time).is_ok_and(|time| time > Utc::now())
}
pub fn configure(
    home: &HomeLayout,
    listener: Option<DirectListener>,
    name: Option<&str>,
) -> io::Result<()> {
    configure_checked(home, listener, name, None)
}
pub fn configure_checked(
    home: &HomeLayout,
    listener: Option<DirectListener>,
    name: Option<&str>,
    expected: Option<&Option<DirectListener>>,
) -> io::Result<()> {
    let name = name.map(str::trim);
    if name.is_some_and(|name| {
        name.chars().count() > 60
            || name.chars().any(|c| {
                c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
    }) {
        return Err(invalid(
            "Instance name must have at most 60 characters without control characters",
        ));
    }
    if let Some(listener) = &listener {
        let bind: std::net::SocketAddr = listener
            .bind
            .parse()
            .map_err(|_| invalid("Listener requires an IP address and port"))?;
        if bind.port() == 0 {
            return Err(invalid("Listener port must not be zero"));
        }
        address(&listener.address)?;
        InstanceIdentity::load_or_create(home)?;
    }
    update(home, |store| {
        if expected.is_some_and(|expected| expected != &store.listener) {
            return Err(invalid(
                "Receiving settings changed; review the current settings and try again",
            ));
        }
        store.listener = listener;
        if let Some(name) = name {
            store.display_name = name.into();
        }
        Ok(())
    })
}
pub fn endpoint(home: &HomeLayout, group_id: &str) -> io::Result<DirectEndpoint> {
    let identity = InstanceIdentity::load_or_create(home)?;
    let group = GroupStore::new(home.clone())?.load(group_id)?;
    Ok(DirectEndpoint {
        instance_id: identity.peer_id.clone(),
        public_key: identity.public_key_b64,
        client_version: env!("CARGO_PKG_VERSION").into(),
        name: {
            let store = load(home)?;
            if store.display_name.is_empty() {
                format!("CCCC · {}", &identity.peer_id[identity.peer_id.len() - 6..])
            } else {
                store.display_name
            }
        },
        generation: crate::connect_groups::generation(&group),
        group_id: group.group_id,
        title: group.title.chars().take(256).collect(),
    })
}
pub fn current(home: &HomeLayout, endpoint: &DirectEndpoint) -> io::Result<bool> {
    let identity = InstanceIdentity::load(home)?;
    if identity.peer_id != endpoint.instance_id || identity.public_key_b64 != endpoint.public_key {
        return Ok(false);
    }
    let group = match GroupStore::new(home.clone())?.load(&endpoint.group_id) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        result => result?,
    };
    Ok(crate::connect_groups::generation(&group) == endpoint.generation)
}
fn validate_endpoint(endpoint: &DirectEndpoint) -> io::Result<()> {
    if peer_id_from_public_key(&endpoint.public_key).as_deref()
        != Some(endpoint.instance_id.as_str())
        || endpoint.group_id.is_empty()
        || endpoint.group_id.len() > 128
        || endpoint.group_id.contains(['/', '\\'])
        || endpoint.generation.is_empty()
        || endpoint.generation.len() > 128
        || endpoint.title.len() > 1024
        || endpoint.name.len() > 512
        || endpoint.client_version.is_empty()
        || endpoint.client_version.len() > 64
    {
        return Err(invalid("Invalid peer Group identity"));
    }
    Ok(())
}
pub fn instance(endpoint: &DirectEndpoint, created: &str) -> ConnectInstance {
    ConnectInstance {
        instance_id: endpoint.instance_id.clone(),
        device_id: String::new(),
        public_key: endpoint.public_key.clone(),
        client_version: endpoint.client_version.clone(),
        public_origin: None,
        display_name: endpoint.name.clone(),
        registered_at: created.into(),
    }
}
pub fn binding(
    home: &HomeLayout,
    remote_id: &str,
    id: &str,
) -> Result<crate::connect_peer::PeerBinding, String> {
    let store = load(home).map_err(|e| e.to_string())?;
    let relation = store
        .relations
        .iter()
        .find(|r| r.id == id)
        .ok_or("Direct connection was removed")?;
    let remote = relation
        .remote
        .as_ref()
        .ok_or("Direct connection is awaiting approval")?;
    if relation.state != DirectState::Active
        || remote.instance_id != remote_id
        || !current(home, &relation.local).map_err(|e| e.to_string())?
    {
        return Err("Direct connection is no longer authorized".into());
    }
    Ok(crate::connect_peer::PeerBinding {
        account_origin: String::new(),
        account_id: String::new(),
        local: instance(&relation.local, &relation.created_at),
        remote: instance(remote, &relation.created_at),
        group: Some(crate::connect_peer::GroupBinding {
            id: id.into(),
            local_group_id: relation.local.group_id.clone(),
            remote_group_id: remote.group_id.clone(),
        }),
    })
}

fn new_invitation(
    local: DirectEndpoint,
    listener: &DirectListener,
    version: u32,
    lifetime: chrono::Duration,
) -> DirectInvitation {
    DirectInvitation {
        v: version,
        id: format!("direct-{}", uuid::Uuid::new_v4()),
        address: listener.address.clone(),
        host: local,
        expires_at: (Utc::now() + lifetime).to_rfc3339(),
        secret: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
    }
}
