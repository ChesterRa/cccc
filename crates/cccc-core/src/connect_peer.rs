//! Current membership proof for bounded peer operations, independent of Web logins.
use crate::{
    HomeLayout, connect,
    instance_identity::{InstanceIdentity, verify_signature},
};
use cccc_contracts::connect::{
    ConnectIdentityProof, ConnectInstance, ConnectPeerAuthorization, ConnectPeerOperation,
    ConnectPeerRequest, ConnectPeerResponse,
};
use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

const REQUEST_LIFETIME_SECONDS: i64 = 60;

#[cfg(test)]
#[path = "connect_peer_tests.rs"]
pub(crate) mod tests;

/// The transport authenticates a peer; resource authorization is still explicit.
/// External connections admit only their exact Group pair; account peers retain instance-wide scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerScope {
    SameAccount,
    GroupPair {
        source_group_id: String,
        target_group_id: String,
    },
}

impl PeerScope {
    pub fn allows(&self, source_group_id: &str, target_group_id: Option<&str>) -> bool {
        match self {
            Self::SameAccount => true,
            Self::GroupPair {
                source_group_id: source,
                target_group_id: target,
            } => {
                !source_group_id.is_empty()
                    && source == source_group_id
                    && target_group_id == Some(target.as_str())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeerBinding {
    pub group: Option<GroupBinding>,
    pub account_origin: String,
    pub account_id: String,
    pub local: ConnectInstance,
    pub remote: ConnectInstance,
}

#[derive(Clone, Debug)]
pub struct GroupBinding {
    pub id: String,
    pub local_group_id: String,
    pub remote_group_id: String,
}

/// This is deliberately separate from instance-wide Workbench authority.
pub fn scoped_binding(
    home: &HomeLayout,
    remote_id: &str,
    connection_id: Option<&str>,
) -> Result<PeerBinding, String> {
    let Some(id) = connection_id else {
        return binding(home, remote_id);
    };
    if cccc_contracts::direct::is_direct(id) {
        return crate::direct::binding(home, remote_id, id);
    }
    let links = crate::connect_groups::load(home)
        .map_err(|e| e.to_string())?
        .ok_or("external Group confirmation expired")?;
    let link = links
        .links
        .iter()
        .find(|l| l.id == id)
        .ok_or("external Group connection was disconnected")?;
    let (local, remote) =
        crate::connect_groups::local_endpoint(&links, link).ok_or("invalid Group connection")?;
    if remote.instance.instance_id != remote_id
        || !crate::connect_groups::resource_current(home, local).map_err(|e| e.to_string())?
    {
        return Err("external Group connection belongs to another resource".into());
    }
    Ok(PeerBinding {
        account_origin: links.account_origin.clone(),
        account_id: link.source.account_id.clone(),
        local: local.instance.clone(),
        remote: remote.instance.clone(),
        group: Some(GroupBinding {
            id: id.into(),
            local_group_id: local.group_id.clone(),
            remote_group_id: remote.group_id.clone(),
        }),
    })
}

pub fn group_binding(
    home: &HomeLayout,
    remote_id: &str,
    source_group: &str,
    target_group: &str,
) -> Result<PeerBinding, String> {
    if let Some(relation) = crate::direct::load(home)
        .map_err(|e| e.to_string())?
        .relations
        .iter()
        .find(|r| {
            r.local.group_id == source_group
                && r.remote
                    .as_ref()
                    .is_some_and(|p| p.instance_id == remote_id && p.group_id == target_group)
        })
    {
        return crate::direct::binding(home, remote_id, &relation.id);
    }
    if let Ok(binding) = binding(home, remote_id) {
        return Ok(binding);
    }
    let links = crate::connect_groups::load(home)
        .map_err(|e| e.to_string())?
        .ok_or("no active external Group connections")?;
    let link = links
        .links
        .iter()
        .find(|link| {
            crate::connect_groups::local_endpoint(&links, link).is_some_and(|(a, b)| {
                a.group_id == source_group
                    && b.group_id == target_group
                    && b.instance.instance_id == remote_id
            })
        })
        .ok_or("this Group has no connection to the selected external Group")?;
    scoped_binding(home, remote_id, Some(&link.id))
}

pub fn binding(home: &HomeLayout, remote_id: &str) -> Result<PeerBinding, String> {
    let snapshot = connect::load(home)
        .map_err(|error| error.to_string())?
        .ok_or("Connect is not linked")?;
    let directory = snapshot
        .directory
        .ok_or("Connect account confirmation expired")?;
    if remote_id == snapshot.instance_id {
        return Err("Connect requires a different instance".into());
    }
    let local = directory
        .instances
        .iter()
        .find(|entry| entry.instance_id == snapshot.instance_id)
        .ok_or("local instance is no longer registered")?
        .clone();
    let remote = directory
        .instances
        .into_iter()
        .find(|entry| entry.instance_id == remote_id)
        .ok_or("peer is no longer registered in this account")?;
    Ok(PeerBinding {
        group: None,
        account_origin: snapshot.account_origin,
        account_id: directory.account_id,
        local,
        remote,
    })
}

pub fn sign_request(
    home: &HomeLayout,
    remote_id: &str,
    operation: ConnectPeerOperation,
) -> Result<ConnectPeerRequest, String> {
    let binding = scoped_binding(home, remote_id, operation.connection_id())?;
    if binding.group.as_ref().is_some_and(|g| {
        g.local_group_id != operation.source_group_id()
            || Some(g.remote_group_id.as_str()) != operation.target_group_id()
    }) {
        return Err("operation is outside the selected Group connection".into());
    }
    let identity = InstanceIdentity::load(home).map_err(|error| error.to_string())?;
    if identity.peer_id != binding.local.instance_id {
        return Err("local identity changed".into());
    }
    let now = Utc::now();
    let mut proof = ConnectPeerAuthorization {
        connection_id: operation.connection_id().map(str::to_owned),
        account_origin: binding.account_origin,
        account_id: binding.account_id,
        source_instance_id: binding.local.instance_id,
        source_device_id: binding.local.device_id,
        target_instance_id: binding.remote.instance_id,
        target_device_id: binding.remote.device_id,
        request_id: uuid::Uuid::new_v4().to_string(),
        issued_at: now.to_rfc3339_opts(SecondsFormat::Millis, true),
        expires_at: (now + chrono::Duration::seconds(REQUEST_LIFETIME_SECONDS))
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        operation_sha256: operation_digest(&operation),
        signature: String::new(),
    };
    proof.signature = identity
        .sign(&proof.signing_material())
        .map_err(|error| error.to_string())?;
    Ok(ConnectPeerRequest { proof, operation })
}

pub fn authenticate(home: &HomeLayout, request: &ConnectPeerRequest) -> Result<PeerScope, String> {
    if operation_digest(&request.operation) != request.proof.operation_sha256
        || request.operation.connection_id() != request.proof.connection_id.as_deref()
    {
        return Err("peer body does not match its signed digest".into());
    }
    let scope = authenticate_authorization(home, &request.proof)?;
    if !scope.allows(
        request.operation.source_group_id(),
        request.operation.target_group_id(),
    ) {
        return Err("peer operation is outside the selected Group connection".into());
    }
    Ok(scope)
}

pub fn operation_digest(operation: &ConnectPeerOperation) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(operation).expect("serializable peer operation"))
    )
}

pub fn authenticate_authorization(
    home: &HomeLayout,
    request: &ConnectPeerAuthorization,
) -> Result<PeerScope, String> {
    let binding = scoped_binding(
        home,
        &request.source_instance_id,
        request.connection_id.as_deref(),
    )?;
    let issued =
        DateTime::parse_from_rfc3339(&request.issued_at).map_err(|_| "invalid peer proof time")?;
    let expires =
        DateTime::parse_from_rfc3339(&request.expires_at).map_err(|_| "invalid peer proof time")?;
    let now = Utc::now();
    if binding.account_origin != request.account_origin
        || binding.account_id != request.account_id
        || binding.local.instance_id != request.target_instance_id
        || binding.local.device_id != request.target_device_id
        || binding.remote.device_id != request.source_device_id
        || uuid::Uuid::parse_str(&request.request_id).is_err()
        || request.operation_sha256.len() != 64
        || !request
            .operation_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || issued > now + chrono::Duration::seconds(30)
        || expires <= now
        || expires <= issued
        || expires - issued > chrono::Duration::seconds(REQUEST_LIFETIME_SECONDS)
        || !verify_signature(
            &binding.remote.instance_id,
            &binding.remote.public_key,
            &request.signature,
            &request.signing_material(),
        )
    {
        return Err("peer proof does not match the current account binding".into());
    }
    Ok(match binding.group {
        Some(group) => PeerScope::GroupPair {
            source_group_id: group.remote_group_id,
            target_group_id: group.local_group_id,
        },
        None => PeerScope::SameAccount,
    })
}

pub fn sign_response(
    home: &HomeLayout,
    request: &ConnectPeerRequest,
    result: serde_json::Value,
) -> Result<ConnectPeerResponse, String> {
    // Recheck after the operation: a result from a retired binding is not attributed to a new one.
    authenticate(home, request)?;
    let identity = InstanceIdentity::load(home).map_err(|error| error.to_string())?;
    if identity.peer_id != request.proof.target_instance_id {
        return Err("local identity changed".into());
    }
    let mut response = ConnectPeerResponse {
        request_id: request.proof.request_id.clone(),
        request_sha256: format!("{:x}", Sha256::digest(request.signing_material())),
        result,
        signature: String::new(),
    };
    response.signature = identity
        .sign(&response.signing_material())
        .map_err(|error| error.to_string())?;
    Ok(response)
}

pub fn verify_response(
    home: &HomeLayout,
    request: &ConnectPeerRequest,
    response: &ConnectPeerResponse,
) -> Result<(), String> {
    if operation_digest(&request.operation) != request.proof.operation_sha256 {
        return Err("peer body does not match its signed digest".into());
    }
    let binding = scoped_binding(
        home,
        &request.proof.target_instance_id,
        request.proof.connection_id.as_deref(),
    )?;
    if binding.account_origin != request.proof.account_origin
        || binding.account_id != request.proof.account_id
        || binding.local.instance_id != request.proof.source_instance_id
        || binding.local.device_id != request.proof.source_device_id
        || binding.remote.device_id != request.proof.target_device_id
        || response.request_id != request.proof.request_id
        || response.request_sha256 != format!("{:x}", Sha256::digest(request.signing_material()))
        || !verify_signature(
            &binding.remote.instance_id,
            &binding.remote.public_key,
            &response.signature,
            &response.signing_material(),
        )
    {
        return Err("peer response does not match the request and current binding".into());
    }
    Ok(())
}

pub fn identity_proof(home: &HomeLayout, nonce: &str) -> Result<ConnectIdentityProof, String> {
    if uuid::Uuid::parse_str(nonce).is_err() {
        return Err("invalid identity challenge".into());
    }
    let snapshot = connect::load(home)
        .map_err(|error| error.to_string())?
        .ok_or("Connect is not linked")?;
    let directory = snapshot
        .directory
        .ok_or("Connect is waiting for account confirmation")?;
    let own = directory
        .instances
        .iter()
        .find(|entry| entry.device_id == snapshot.device_id)
        .ok_or("instance not registered")?;
    let identity = InstanceIdentity::load(home).map_err(|error| error.to_string())?;
    if identity.peer_id != snapshot.instance_id {
        return Err("instance identity changed".into());
    }
    let mut proof = ConnectIdentityProof {
        instance_id: snapshot.instance_id,
        device_id: snapshot.device_id,
        public_origin: own.public_origin.clone(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        nonce: nonce.into(),
        expires_at: (Utc::now() + chrono::Duration::seconds(30))
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        signature: String::new(),
    };
    proof.signature = identity
        .sign(&proof.signing_material())
        .map_err(|error| error.to_string())?;
    Ok(proof)
}

pub fn verify_identity(
    target: &ConnectInstance,
    origin: &str,
    nonce: &str,
    proof: &ConnectIdentityProof,
) -> Result<(), String> {
    let expires = DateTime::parse_from_rfc3339(&proof.expires_at)
        .map_err(|_| "invalid target identity time")?;
    let now = Utc::now();
    if target.public_origin.as_deref() != Some(origin)
        || proof.instance_id != target.instance_id
        || proof.device_id != target.device_id
        || proof.public_origin.as_deref() != Some(origin)
        || proof.client_version != target.client_version
        || proof.nonce != nonce
        || expires <= now
        || expires > now + chrono::Duration::seconds(60)
        || !verify_signature(
            &target.instance_id,
            &target.public_key,
            &proof.signature,
            &proof.signing_material(),
        )
    {
        return Err("target identity does not match its account registration".into());
    }
    Ok(())
}
