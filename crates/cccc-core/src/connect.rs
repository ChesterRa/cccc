//! Account-verified directory snapshots. Readers never refresh the account or start runtimes.
use crate::{HomeLayout, fs, membership};
use cccc_contracts::connect::{CONNECT_PROTOCOL_VERSION, ConnectDirectory};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io};

pub const DIRECTORY_TTL_SECONDS: i64 = 120;

#[cfg(test)]
#[path = "connect_tests.rs"]
mod tests;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ConnectGroupSync {
    pub checked_at: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ConnectSnapshot {
    pub account_origin: String,
    pub device_id: String,
    pub instance_id: String,
    pub directory: Option<ConnectDirectory>,
    pub checked_at: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    /// Diagnostics only; Group authority remains in the expiring issuer grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_sync: Option<ConnectGroupSync>,
}

pub fn save(home: &HomeLayout, snapshot: &ConnectSnapshot) -> io::Result<()> {
    fs::write_secret_json(&home.root().join("secrets/connect.json"), snapshot)
}

/// The current local binding and bounded issuer lifetime are checked on every use.
pub fn load(home: &HomeLayout) -> io::Result<Option<ConnectSnapshot>> {
    let state = membership::load(home)?;
    if !state.logged_in || state.disabled || state.device_token.as_deref().unwrap_or("").is_empty()
    {
        return Ok(None);
    }
    let path = home.root().join("secrets/connect.json");
    let mut snapshot: ConnectSnapshot = match fs::read_json(&path) {
        Ok(snapshot) => snapshot,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if state.device_id.as_deref() != Some(snapshot.device_id.as_str())
        || state
            .account_origin
            .as_deref()
            .map(membership::canonical_account_origin)
            .as_deref()
            != Some(snapshot.account_origin.as_str())
    {
        return Ok(None);
    }
    if snapshot.directory.as_ref().is_some_and(|directory| {
        validate_directory(
            directory,
            &snapshot.device_id,
            &snapshot.instance_id,
            Utc::now(),
        )
        .is_err()
    }) {
        snapshot.directory = None;
        if snapshot.error_code.is_none() {
            snapshot.error_code = Some("connect_directory_expired".into());
            snapshot.error_message =
                Some("Account confirmation expired; waiting to reconnect.".into());
        }
    }
    Ok(Some(snapshot))
}

pub fn validate_directory(
    directory: &ConnectDirectory,
    device_id: &str,
    instance_id: &str,
    now: DateTime<Utc>,
) -> io::Result<()> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid or expired Connect directory",
        )
    };
    let issued = DateTime::parse_from_rfc3339(&directory.issued_at).map_err(|_| invalid())?;
    let expires = DateTime::parse_from_rfc3339(&directory.expires_at).map_err(|_| invalid())?;
    if directory.protocol_version != CONNECT_PROTOCOL_VERSION
        || directory.device_id != device_id
        || directory.account_id.is_empty()
        || directory.account_id.len() > 128
        || expires <= now
        || issued > now + chrono::Duration::seconds(30)
        || expires <= issued
        || expires - issued > chrono::Duration::seconds(DIRECTORY_TTL_SECONDS)
        || directory.instances.len() > 256
    {
        return Err(invalid());
    }
    let mut instance_ids = HashSet::new();
    let mut device_ids = HashSet::new();
    let mut has_self = false;
    for instance in &directory.instances {
        if instance.device_id.is_empty()
            || instance.device_id.len() > 128
            || instance.display_name.len() > 512
            || instance.client_version.len() > 80
            || !instance_ids.insert(instance.instance_id.as_str())
            || !device_ids.insert(instance.device_id.as_str())
            || crate::instance_identity::peer_id_from_public_key(&instance.public_key).as_deref()
                != Some(instance.instance_id.as_str())
        {
            return Err(invalid());
        }
        if let Some(origin) = &instance.public_origin {
            canonical_public_origin(origin).ok_or_else(invalid)?;
        }
        if instance.device_id == device_id {
            if instance.instance_id != instance_id {
                return Err(invalid());
            }
            has_self = true;
        }
    }
    if !has_self {
        return Err(invalid());
    }
    Ok(())
}

pub fn canonical_public_origin(value: &str) -> Option<String> {
    if value.len() > 512 {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    let local = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !(url.scheme() == "https" || url.scheme() == "http" && local)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.origin().ascii_serialization() != value
    {
        return None;
    }
    Some(value.to_owned())
}
