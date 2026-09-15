//! Bounded-lived peer navigation metadata. The daemon refreshes it; ports only read.
use crate::{HomeLayout, connect_peer, fs};
use cccc_contracts::connect::ConnectGroup;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerCatalog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_origin: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub local_device_id: String,
    pub remote_instance_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote_device_id: String,
    pub remote_origin: String,
    pub checked_at: String,
    pub groups: Vec<ConnectGroup>,
}

pub fn save(home: &HomeLayout, catalog: &PeerCatalog) -> io::Result<()> {
    // Direct grant deletion and cache commits share a lock, so late refreshes
    // cannot leave a deleted connection's cache behind.
    let save = || {
        validate(home, catalog).map_err(io::Error::other)?;
        fs::write_secret_json(
            &path(
                home,
                &catalog.remote_instance_id,
                catalog.connection_id.as_deref(),
            )?,
            catalog,
        )
    };
    if catalog
        .connection_id
        .as_deref()
        .is_some_and(cccc_contracts::direct::is_direct)
    {
        fs::with_exclusive_lock(&home.root().join("direct_connections.lock"), save)
    } else {
        save()
    }
}

pub fn load(home: &HomeLayout, remote_id: &str) -> io::Result<Option<PeerCatalog>> {
    load_scoped(home, remote_id, None)
}

pub fn load_scoped(
    home: &HomeLayout,
    remote_id: &str,
    connection_id: Option<&str>,
) -> io::Result<Option<PeerCatalog>> {
    let catalog: PeerCatalog = match fs::read_json(&path(home, remote_id, connection_id)?) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if catalog.remote_instance_id != remote_id
        || catalog.connection_id.as_deref() != connection_id
        || validate(home, &catalog).is_err()
    {
        return Ok(None);
    }
    Ok(Some(catalog))
}

/// Old navigation metadata can identify an offline recipient without claiming it
/// is online. The current account/device binding is still required on every read.
pub fn is_fresh(catalog: &PeerCatalog) -> bool {
    DateTime::parse_from_rfc3339(&catalog.checked_at)
        .is_ok_and(|checked| checked + chrono::Duration::seconds(120) > Utc::now())
}

fn path(
    home: &HomeLayout,
    instance_id: &str,
    connection_id: Option<&str>,
) -> io::Result<std::path::PathBuf> {
    if !(16..=128).contains(&instance_id.len())
        || !instance_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid instance ID",
        ));
    }
    let key = if let Some(id) = connection_id {
        uuid::Uuid::parse_str(id.strip_prefix("direct-").unwrap_or(id))
            .map_err(io::Error::other)?;
        if cccc_contracts::direct::is_direct(id) {
            id.into()
        } else {
            format!("group-{id}")
        }
    } else {
        instance_id.to_owned()
    };
    Ok(home
        .root()
        .join("state/connect/catalog")
        .join(format!("{key}.json")))
}

fn validate(home: &HomeLayout, catalog: &PeerCatalog) -> Result<(), String> {
    let binding = connect_peer::scoped_binding(
        home,
        &catalog.remote_instance_id,
        catalog.connection_id.as_deref(),
    )?;
    let checked =
        DateTime::parse_from_rfc3339(&catalog.checked_at).map_err(|_| "invalid catalog time")?;
    let now = Utc::now();
    if binding.group.as_ref().is_some_and(|g| {
        catalog.groups.len() != 1 || catalog.groups[0].group_id != g.remote_group_id
    }) || binding.account_origin != catalog.account_origin
        || binding.account_id != catalog.account_id
        || binding.local.device_id != catalog.local_device_id
        || binding.remote.device_id != catalog.remote_device_id
        || binding.remote.public_origin.as_deref().unwrap_or_default() != catalog.remote_origin
        || checked > now + chrono::Duration::seconds(30)
    {
        return Err("peer catalog belongs to a previous binding or has invalid timing".into());
    }
    Ok(())
}
