//! Persistent standalone grants. Reads do not create identities, listeners or jobs.
use crate::{
    GroupStore, HomeLayout, fs,
    instance_identity::{InstanceIdentity, peer_id_from_public_key},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cccc_contracts::{connect::ConnectInstance, direct::*};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::io;
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
    DateTime::parse_from_rfc3339(time).is_ok_and(|time| time > Utc::now())
}
pub fn address(value: &str) -> io::Result<String> {
    let url = url::Url::parse(&format!("tcp://{value}"))
        .map_err(|_| invalid("Use a hostname or IP address with a port"))?;
    if url.host_str().is_none()
        || url
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_unspecified())
        || url.port().is_none_or(|port| port == 0)
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "Use a reachable hostname or IP address with a port, not 0.0.0.0 or ::",
        ));
    }
    Ok(value.to_owned())
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
pub fn invite(home: &HomeLayout, group: &str) -> io::Result<String> {
    invite_checked(home, group, None)
}
pub fn invite_checked(
    home: &HomeLayout,
    group: &str,
    expected: Option<&DirectListener>,
) -> io::Result<String> {
    let local = endpoint(home, group)?;
    update(home, |store| {
        if store.relations.len() >= 64 {
            return Err(invalid(
                "Remove unused connections before creating another invitation",
            ));
        }
        let listener = store
            .listener
            .as_ref()
            .ok_or_else(|| invalid("Enable the direct listener first"))?;
        if expected.is_some_and(|expected| expected != listener) {
            return Err(invalid(
                "Receiving settings changed; review the current settings and try again",
            ));
        }
        let invitation = DirectInvitation {
            v: 1,
            id: format!("direct-{}", uuid::Uuid::new_v4()),
            address: listener.address.clone(),
            host: local.clone(),
            expires_at: (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339(),
            secret: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        };
        store.relations.push(DirectRelation {
            id: invitation.id.clone(),
            local,
            remote: None,
            state: DirectState::Invited,
            created_at: cccc_contracts::utc_now(),
            expires_at: invitation.expires_at.clone(),
            secret_sha256: hash(&invitation.secret),
            invitation: None,
        });
        Ok(format!(
            "cccc-direct:{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&invitation).map_err(io::Error::other)?)
        ))
    })
}
pub fn join(home: &HomeLayout, group: &str, text: &str) -> io::Result<String> {
    if text.len() > 8192 {
        return Err(invalid("Invitation is too large"));
    }
    let raw = URL_SAFE_NO_PAD
        .decode(
            text.trim()
                .strip_prefix("cccc-direct:")
                .ok_or_else(|| invalid("Invalid direct invitation"))?,
        )
        .map_err(|_| invalid("Invalid direct invitation"))?;
    let invitation: DirectInvitation =
        serde_json::from_slice(&raw).map_err(|_| invalid("Invalid direct invitation"))?;
    validate_endpoint(&invitation.host)?;
    address(&invitation.address)?;
    let local = endpoint(home, group)?;
    if invitation.v != 1
        || !is_direct(&invitation.id)
        || !unexpired(&invitation.expires_at)
        || invitation.secret.len() != 64
        || local.instance_id == invitation.host.instance_id
    {
        return Err(invalid("Invitation expired or belongs to this instance"));
    }
    update(home, |store| {
        if store.retired.contains(&invitation.id) {
            return Err(invalid(
                "This invitation was already retired; request a new invitation",
            ));
        }
        if let Some(existing) = store.relations.iter().find(|r| r.id == invitation.id) {
            if existing.local == local
                && existing
                    .invitation
                    .as_ref()
                    .is_some_and(|i| i.secret == invitation.secret)
                && !matches!(existing.state, DirectState::Revoked | DirectState::Expired)
            {
                return Ok(existing.id.clone());
            }
            return Err(invalid("Invitation was already used"));
        }
        if store.relations.len() >= 64
            || store.relations.iter().any(|r| {
                r.local.group_id == local.group_id
                    && r.remote.as_ref().is_some_and(|remote| {
                        remote.instance_id == invitation.host.instance_id
                            && remote.group_id == invitation.host.group_id
                    })
            })
        {
            return Err(invalid(
                "A direct connection for this Group pair already exists",
            ));
        }
        let id = invitation.id.clone();
        store.relations.push(DirectRelation {
            id: id.clone(),
            local,
            remote: Some(invitation.host.clone()),
            state: DirectState::Pending,
            created_at: cccc_contracts::utc_now(),
            expires_at: invitation.expires_at.clone(),
            secret_sha256: String::new(),
            invitation: Some(invitation),
        });
        Ok(id)
    })
}
/// Called only after mutual TLS; the submitted key must equal the TLS identity.
pub fn hello(
    home: &HomeLayout,
    id: &str,
    secret: &str,
    remote: &DirectEndpoint,
    tls_key: &str,
) -> io::Result<DirectState> {
    validate_endpoint(remote)?;
    if remote.public_key != tls_key {
        return Err(invalid("Peer identity mismatch"));
    }
    update(home, |store| {
        let Some(position) = store
            .relations
            .iter()
            .position(|r| r.id == id && r.invitation.is_none())
        else {
            // A pinned receiver can definitively refuse an unknown or retired ID.
            // This negative answer grants nothing and discloses no stored content.
            return Ok(DirectState::Revoked);
        };
        if store.relations.iter().enumerate().any(|(i, r)| {
            i != position
                && r.local.group_id == store.relations[position].local.group_id
                && r.remote.as_ref().is_some_and(|other| {
                    other.instance_id == remote.instance_id && other.group_id == remote.group_id
                })
        }) {
            return Err(invalid(
                "A direct connection for this Group pair already exists",
            ));
        }
        let relation = &mut store.relations[position];
        if relation.secret_sha256 != hash(secret)
            || relation.remote.as_ref().is_some_and(|r| r != remote)
            || !current(home, &relation.local)?
        {
            return Err(invalid("Invitation no longer matches this Group pair"));
        }
        if relation.state == DirectState::Revoked {
            return Ok(DirectState::Revoked);
        }
        if relation.state == DirectState::Expired
            || (relation.state != DirectState::Active && !unexpired(&relation.expires_at))
        {
            relation.state = DirectState::Expired;
            return Ok(DirectState::Expired);
        }
        if relation.remote.is_none() {
            relation.remote = Some(remote.clone());
            relation.state = DirectState::Pending;
        }
        Ok(relation.state.clone())
    })
}
pub fn set_state(home: &HomeLayout, group: &str, id: &str, approve: bool) -> io::Result<()> {
    update(home, |store| {
        let relation = store
            .relations
            .iter_mut()
            .find(|r| r.id == id && r.local.group_id == group)
            .ok_or_else(|| invalid("Unknown connection"))?;
        if approve {
            if relation.invitation.is_some()
                || relation.remote.is_none()
                || !matches!(relation.state, DirectState::Pending | DirectState::Active)
                || (relation.state != DirectState::Active && !unexpired(&relation.expires_at))
                || !current(home, &relation.local)?
            {
                return Err(invalid("Connection cannot be approved"));
            }
            relation.state = DirectState::Active;
        } else {
            relation.state = DirectState::Revoked;
        }
        Ok(())
    })
}
pub fn remove(home: &HomeLayout, group: &str, id: &str) -> io::Result<()> {
    if !is_direct(id) {
        return Err(invalid("Invalid direct connection ID"));
    }
    update(home, |store| {
        if store
            .relations
            .iter()
            .any(|r| r.id == id && r.local.group_id != group)
        {
            return Err(invalid("Connection belongs to another Group"));
        }
        if store.relations.iter().any(|r| {
            r.id == id
                && r.local.group_id == group
                && !matches!(r.state, DirectState::Revoked | DirectState::Expired)
                && (r.state == DirectState::Active
                    || r.invitation.is_some()
                    || unexpired(&r.expires_at))
        }) {
            return Err(invalid("Disconnect before removing this connection"));
        }
        let existed = store.relations.iter().any(|r| r.id == id);
        store
            .relations
            .retain(|r| !(r.id == id && r.local.group_id == group));
        if existed {
            store.retired.insert(id.into());
        }
        Ok(())
    })?;
    remove_catalog(home, id)
}

/// Called after Group deletion commits. No grants, secrets or quota survive it.
pub fn retire_group(home: &HomeLayout, group: &str) -> io::Result<()> {
    update(home, |store| {
        let ids = store
            .relations
            .iter()
            .filter(|r| r.local.group_id == group)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        // Remove disposable caches before retiring the records, so a filesystem
        // failure leaves IDs available for an explicit cleanup retry.
        for id in &ids {
            remove_catalog(home, id)?;
        }
        store.relations.retain(|r| r.local.group_id != group);
        store.retired.extend(ids);
        Ok(())
    })
}

fn remove_catalog(home: &HomeLayout, id: &str) -> io::Result<()> {
    if !is_direct(id) {
        return Err(invalid("Invalid direct connection ID"));
    }
    match std::fs::remove_file(
        home.root()
            .join("state/connect/catalog")
            .join(format!("{id}.json")),
    ) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
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
