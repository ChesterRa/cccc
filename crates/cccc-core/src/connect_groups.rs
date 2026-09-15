//! Resource selections and short-lived account grants for external Group pairs.
use crate::{
    HomeLayout, connect, fs,
    group::GroupStore,
    instance_identity::{InstanceIdentity, verify_signature},
};
use cccc_contracts::connect_groups::{
    ConnectGroupCheck, ConnectGroupCheckResult, ConnectGroupEndpoint, ConnectGroupLink,
    ConnectGroupLinks, ConnectGroupTicket,
};
use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use std::io;

#[cfg(test)]
#[path = "connect_groups_tests.rs"]
mod tests;

fn timestamp(time: chrono::DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn generation(group: &crate::GroupDoc) -> String {
    if group.generation.is_empty() {
        format!("legacy:{}", group.created_at)
    } else {
        group.generation.clone()
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SelectionError {
    #[error("Group selection expired or belongs to a previous resource binding")]
    Invalid,
    #[error("{0}")]
    Unavailable(&'static str),
}

pub fn ticket(home: &HomeLayout, group_id: &str) -> Result<ConnectGroupTicket, SelectionError> {
    use SelectionError::{Invalid, Unavailable};
    let snapshot = connect::load(home)
        .map_err(|_| Unavailable("Could not read Connect confirmation"))?
        .ok_or(Unavailable("Connect is not linked"))?;
    let directory = snapshot
        .directory
        .ok_or(Unavailable("Connect account confirmation expired"))?;
    let own = directory
        .instances
        .iter()
        .find(|i| i.instance_id == snapshot.instance_id)
        .ok_or(Unavailable("Instance is not registered"))?;
    if own.public_origin.is_none() {
        return Err(Unavailable(
            "Enable Remote Access before connecting an external Group",
        ));
    }
    let store =
        GroupStore::new(home.clone()).map_err(|_| Unavailable("Could not read local Groups"))?;
    let group = store.load(group_id).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            Invalid
        } else {
            Unavailable("Could not read the selected Group")
        }
    })?;
    let identity = InstanceIdentity::load(home)
        .map_err(|_| Unavailable("Could not read instance identity"))?;
    if identity.peer_id != snapshot.instance_id {
        return Err(Invalid);
    }
    let now = Utc::now();
    let mut ticket = ConnectGroupTicket {
        account_origin: snapshot.account_origin,
        account_id: directory.account_id,
        instance_id: snapshot.instance_id,
        instance_name: own.display_name.clone(),
        device_id: snapshot.device_id,
        group_generation: generation(&group),
        group_id: group.group_id,
        title: group.title.chars().take(256).collect(),
        issued_at: timestamp(now),
        expires_at: timestamp(now + chrono::Duration::hours(24)),
        signature: String::new(),
    };
    ticket.signature = identity
        .sign(&ticket.signing_material())
        .map_err(|_| Unavailable("Could not sign Group selection"))?;
    Ok(ticket)
}

pub fn check(
    home: &HomeLayout,
    request: &ConnectGroupCheck,
) -> Result<ConnectGroupCheckResult, SelectionError> {
    use SelectionError::{Invalid, Unavailable};
    let t = &request.ticket;
    let issued = DateTime::parse_from_rfc3339(&t.issued_at).map_err(|_| Invalid)?;
    let expires = DateTime::parse_from_rfc3339(&t.expires_at).map_err(|_| Invalid)?;
    let identity = InstanceIdentity::load(home)
        .map_err(|_| Unavailable("Could not read instance identity"))?;
    // Authenticate before touching the requested Group, including for nonexistent IDs.
    // The public check must not become an anonymous resource-existence probe.
    if uuid::Uuid::parse_str(&request.nonce).is_err()
        || issued > Utc::now() + chrono::Duration::seconds(30)
        || expires <= Utc::now()
        || expires <= issued
        || expires - issued > chrono::Duration::hours(24)
        || !verify_signature(
            &identity.peer_id,
            &identity.public_key_b64,
            &t.signature,
            &t.signing_material(),
        )
    {
        return Err(Invalid);
    }
    let current = ticket(home, &t.group_id)?;
    if t.account_origin != current.account_origin
        || t.account_id != current.account_id
        || t.instance_id != current.instance_id
        || t.device_id != current.device_id
        || t.group_generation != current.group_generation
    {
        return Err(Invalid);
    }
    let mut result = ConnectGroupCheckResult {
        ticket_sha256: format!("{:x}", Sha256::digest(t.signing_material())),
        nonce: request.nonce.clone(),
        title: current.title,
        expires_at: timestamp(Utc::now() + chrono::Duration::seconds(30)),
        signature: String::new(),
    };
    result.signature = identity
        .sign(&result.signing_material())
        .map_err(|_| Unavailable("Could not sign Group confirmation"))?;
    Ok(result)
}

pub fn local_endpoint<'a>(
    links: &ConnectGroupLinks,
    link: &'a ConnectGroupLink,
) -> Option<(&'a ConnectGroupEndpoint, &'a ConnectGroupEndpoint)> {
    if link.source.account_id == links.account_id
        && link.source.instance.device_id == links.device_id
    {
        Some((&link.source, &link.target))
    } else if link.target.account_id == links.account_id
        && link.target.instance.device_id == links.device_id
    {
        Some((&link.target, &link.source))
    } else {
        None
    }
}

/// Only a missing or replaced Group proves retirement. Read errors are indeterminate.
pub fn resource_current(home: &HomeLayout, endpoint: &ConnectGroupEndpoint) -> io::Result<bool> {
    let store = GroupStore::new(home.clone())?;
    match store.load(&endpoint.group_id) {
        Ok(group) => Ok(generation(&group) == endpoint.group_generation),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn save(home: &HomeLayout, links: &ConnectGroupLinks) -> io::Result<()> {
    validate(home, links)?;
    fs::write_secret_json(&home.root().join("secrets/connect_groups.json"), links)?;
    // Connection IDs change after reconnection. Retire their disposable catalogues
    // during issuer refresh so repeated invitations cannot accumulate old caches.
    let directory = match std::fs::read_dir(home.root().join("state/connect/catalog")) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in directory {
        let entry = entry?;
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|n| n.strip_prefix("group-"))
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err() || links.links.iter().any(|link| link.id == id) {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub fn load(home: &HomeLayout) -> io::Result<Option<ConnectGroupLinks>> {
    let links: ConnectGroupLinks =
        match fs::read_json(&home.root().join("secrets/connect_groups.json")) {
            Ok(links) => links,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
    if validate(home, &links).is_err() {
        return Ok(None);
    }
    Ok(Some(links))
}

/// Lack of a fresh lease is unavailable; a fresh list without the ID is an authoritative revocation.
pub fn retired(home: &HomeLayout, id: &str) -> io::Result<bool> {
    if cccc_contracts::direct::is_direct(id) {
        let store = crate::direct::load(home)?;
        let Some(relation) = store.relations.iter().find(|r| r.id == id) else {
            return Ok(true);
        };
        return if matches!(
            relation.state,
            cccc_contracts::direct::DirectState::Revoked
                | cccc_contracts::direct::DirectState::Expired
        ) {
            Ok(true)
        } else {
            crate::direct::current(home, &relation.local).map(|current| !current)
        };
    }
    let Some(links) = load(home)? else {
        return Ok(false);
    };
    let Some(link) = links.links.iter().find(|link| link.id == id) else {
        return Ok(true);
    };
    let Some((local, _)) = local_endpoint(&links, link) else {
        return Ok(true);
    };
    resource_current(home, local).map(|current| !current)
}

fn validate(home: &HomeLayout, links: &ConnectGroupLinks) -> io::Result<()> {
    let invalid = || io::Error::other("invalid or expired external Group confirmation");
    let snapshot = connect::load(home)?.ok_or_else(invalid)?;
    let directory = snapshot.directory.ok_or_else(invalid)?;
    let issued = DateTime::parse_from_rfc3339(&links.issued_at).map_err(|_| invalid())?;
    let expires = DateTime::parse_from_rfc3339(&links.expires_at).map_err(|_| invalid())?;
    if links.protocol_version != 1
        || links.account_origin != snapshot.account_origin
        || links.account_id != directory.account_id
        || links.device_id != snapshot.device_id
        || issued > Utc::now() + chrono::Duration::seconds(30)
        || expires <= Utc::now()
        || expires <= issued
        || expires - issued > chrono::Duration::seconds(120)
        || links.links.len() > 256
    {
        return Err(invalid());
    }
    let own = directory
        .instances
        .iter()
        .find(|i| i.instance_id == snapshot.instance_id)
        .ok_or_else(invalid)?;
    let mut ids = std::collections::HashSet::new();
    for link in &links.links {
        let (local, remote) = local_endpoint(links, link).ok_or_else(invalid)?;
        if uuid::Uuid::parse_str(&link.id).is_err()
            || !ids.insert(&link.id)
            || local.instance.instance_id != own.instance_id
            || local.instance.public_key != own.public_key
            || local.account_id == remote.account_id
            || remote.account_id.is_empty()
            || local.group_id.is_empty()
            || remote.group_id.is_empty()
            || local.group_generation.is_empty()
            || remote.group_generation.is_empty()
            || [local, remote].iter().any(|e| {
                e.group_id.len() > 128
                    || e.group_id.contains(['/', '\\'])
                    || e.group_generation.len() > 128
            })
            || crate::instance_identity::peer_id_from_public_key(&remote.instance.public_key)
                .as_deref()
                != Some(remote.instance.instance_id.as_str())
            || remote
                .instance
                .public_origin
                .as_ref()
                .is_some_and(|o| connect::canonical_public_origin(o).is_none())
        {
            return Err(invalid());
        }
    }
    Ok(())
}
