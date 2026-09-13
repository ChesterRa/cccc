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
/// GroupPair is exercised by negative fixtures, not an enabled external-sharing API.
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
    pub account_origin: String,
    pub account_id: String,
    pub local: ConnectInstance,
    pub remote: ConnectInstance,
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
    let binding = binding(home, remote_id)?;
    let identity = InstanceIdentity::load(home).map_err(|error| error.to_string())?;
    if identity.peer_id != binding.local.instance_id {
        return Err("local identity changed".into());
    }
    let now = Utc::now();
    let mut proof = ConnectPeerAuthorization {
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
    if operation_digest(&request.operation) != request.proof.operation_sha256 {
        return Err("peer body does not match its signed digest".into());
    }
    authenticate_authorization(home, &request.proof)
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
    let binding = binding(home, &request.source_instance_id)?;
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
    Ok(PeerScope::SameAccount)
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
    let binding = binding(home, &request.proof.target_instance_id)?;
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
