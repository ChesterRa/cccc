//! Embedding authority is separate from the target's existing Web authentication.
use base64::Engine;
use cccc_contracts::connect::{ConnectDirectory, ConnectFrameProof, ConnectIdentityProof};
use cccc_core::{
    HomeLayout, connect,
    instance_identity::{InstanceIdentity, verify_signature},
};
use chrono::{DateTime, SecondsFormat, Utc};
use std::{collections::HashMap, sync::Mutex};

pub(crate) fn frame_ancestor(
    state: &crate::AppState,
    request: &axum::extract::Request,
) -> Result<Option<String>, crate::api::ApiError> {
    let path = request.uri().path();
    let query = request.uri().query().unwrap_or("");
    if matches!(path, "/ui/connect" | "/ui/connect/") {
        if state.web_mode.is_read_only() {
            return Err(crate::api::ApiError::forbidden(
                "an exhibit cannot join the Connect workbench",
            ));
        }
        if query.len() > 4096 {
            return Err(crate::api::ApiError::bad("frame proof is too large"));
        }
        let encoded = url::form_urlencoded::parse(query.as_bytes())
            .find_map(|(key, value)| (key == "proof").then(|| value.into_owned()))
            .ok_or_else(|| {
                crate::api::ApiError::forbidden("open this instance from its entry workbench")
            })?;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| crate::api::ApiError::bad("invalid frame proof encoding"))?;
        let proof: ConnectFrameProof = serde_json::from_slice(&raw)
            .map_err(|_| crate::api::ApiError::bad("invalid frame proof"))?;
        let ancestor = proof.parent_origin.clone();
        state
            .connect_frames
            .accept(&state.home, proof, true)
            .map_err(|message| {
                crate::api::ApiError::forbidden_code("connect_frame_invalid", message)
            })?;
        return Ok(Some(ancestor));
    }
    // Presentation's nested frames must allow the verified entry as an ancestor
    // too. The query selects an existing frame; it supplies no Web authorization.
    if path.starts_with("/api/v1/groups/")
        && let Some(frame_id) = url::form_urlencoded::parse(query.as_bytes())
            .find_map(|(key, value)| (key == "connect_frame").then(|| value.into_owned()))
    {
        let proof = state
            .connect_frames
            .current(&state.home, &frame_id)
            .map_err(|message| {
                crate::api::ApiError::forbidden_code("connect_frame_expired", message)
            })?;
        return Ok(Some(proof.parent_origin));
    }
    Ok(None)
}

const MAX_FRAMES: usize = 128;
const TTL_SECONDS: i64 = 120;

pub(crate) const LIVE_ACCESS_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Default, serde::Deserialize)]
pub(crate) struct ResourceFrameQuery {
    pub connect_frame: Option<String>,
}

pub(crate) fn live_group_access(
    state: &crate::AppState,
    principal: &crate::auth::Principal,
    group_id: &str,
    frame_id: Option<&str>,
) -> bool {
    live_access(state, principal, frame_id).is_some_and(|current| current.allows(group_id))
}

pub(crate) fn live_access(
    state: &crate::AppState,
    principal: &crate::auth::Principal,
    frame_id: Option<&str>,
) -> Option<crate::auth::Principal> {
    principal
        .current(&state.home)
        .ok()
        .flatten()
        .filter(|current| {
            frame_id.is_none_or(|id| {
                current.is_admin && state.connect_frames.current(&state.home, id).is_ok()
            })
        })
}

pub(crate) async fn while_group_access(
    state: &crate::AppState,
    principal: &crate::auth::Principal,
    group_id: &str,
    frame_id: Option<&str>,
    connection: impl std::future::Future<Output = ()>,
) {
    let mut poll = tokio::time::interval(LIVE_ACCESS_INTERVAL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tokio::pin!(connection);
    loop {
        tokio::select! {
            biased;
            _ = poll.tick() => {
                if !live_group_access(state, principal, group_id, frame_id) { return; }
            }
            () = &mut connection => return,
        }
    }
}

#[derive(Default)]
pub(crate) struct ConnectFrames(Mutex<HashMap<String, ConnectFrameProof>>);

pub(crate) use cccc_core::connect_peer::identity_proof;

pub(crate) async fn confirm_target(
    home: &HomeLayout,
    client: &reqwest::Client,
    target_id: &str,
    origin: &str,
) -> Result<(), String> {
    let snapshot = connect::load(home)
        .map_err(|error| error.to_string())?
        .ok_or("Connect is not linked")?;
    let directory = snapshot
        .directory
        .ok_or("Connect account confirmation expired")?;
    let target = directory
        .instances
        .iter()
        .find(|entry| entry.instance_id == target_id)
        .ok_or("target is no longer registered")?;
    if target.public_origin.as_deref() != Some(origin) {
        return Err("target route changed".into());
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let mut response = client
        .get(format!("{origin}/api/v1/connect/identity"))
        .query(&[("nonce", &nonce)])
        .send()
        .await
        .map_err(|_| "target instance is not reachable")?;
    if !response.status().is_success() {
        return Err(
            "target instance did not confirm Connect support; check its version and Remote Access"
                .into(),
        );
    }
    let mut raw = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "target identity response was interrupted")?
    {
        if raw.len() + chunk.len() > 4096 {
            return Err("target identity response is too large".into());
        }
        raw.extend_from_slice(&chunk);
    }
    let proof: ConnectIdentityProof =
        serde_json::from_slice(&raw).map_err(|_| "invalid target identity response")?;
    cccc_core::connect_peer::verify_identity(target, origin, &nonce, &proof)
}

pub(crate) fn http_client() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
}

pub(crate) fn issue(
    home: &HomeLayout,
    target_id: &str,
    parent_origin: &str,
    frame_id: &str,
) -> Result<(String, ConnectFrameProof), String> {
    if uuid::Uuid::parse_str(frame_id).is_err() {
        return Err("invalid frame identifier".into());
    }
    let snapshot = connect::load(home)
        .map_err(|error| error.to_string())?
        .ok_or("Connect is not linked")?;
    let directory = snapshot
        .directory
        .ok_or("Connect is waiting for account confirmation")?;
    let target = directory
        .instances
        .iter()
        .find(|target| target.instance_id == target_id && target.device_id != snapshot.device_id)
        .ok_or("target instance is not available in this account")?;
    let target_origin = target
        .public_origin
        .clone()
        .ok_or("target Remote Access is not enabled")?;
    distinct_web_hosts(&directory, target_id, parent_origin)?;
    let identity = InstanceIdentity::load(home).map_err(|error| error.to_string())?;
    if identity.peer_id != snapshot.instance_id {
        return Err("instance identity changed; waiting for account confirmation".into());
    }
    let now = Utc::now();
    let mut proof = ConnectFrameProof {
        account_origin: snapshot.account_origin,
        source_instance_id: snapshot.instance_id,
        source_device_id: snapshot.device_id,
        target_instance_id: target.instance_id.clone(),
        target_device_id: target.device_id.clone(),
        parent_origin: parent_origin.into(),
        frame_id: frame_id.into(),
        nonce: uuid::Uuid::new_v4().to_string(),
        issued_at: now.to_rfc3339_opts(SecondsFormat::Millis, true),
        expires_at: (now + chrono::Duration::seconds(TTL_SECONDS))
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        signature: String::new(),
    };
    proof.signature = identity
        .sign(&proof.signing_material())
        .map_err(|error| error.to_string())?;
    Ok((target_origin, proof))
}

fn verify(home: &HomeLayout, proof: &ConnectFrameProof) -> Result<(), String> {
    let snapshot = connect::load(home)
        .map_err(|error| error.to_string())?
        .ok_or("Connect is not linked")?;
    let directory = snapshot
        .directory
        .ok_or("Connect account confirmation expired")?;
    if proof.account_origin != snapshot.account_origin
        || proof.target_device_id != snapshot.device_id
        || proof.target_instance_id != snapshot.instance_id
    {
        return Err("frame proof is for another instance or binding".into());
    }
    let parent = cccc_core::web_login_grants::normalize_origin(&proof.parent_origin)
        .ok_or("invalid parent origin")?;
    if parent != proof.parent_origin
        || uuid::Uuid::parse_str(&proof.frame_id).is_err()
        || uuid::Uuid::parse_str(&proof.nonce).is_err()
    {
        return Err("invalid frame proof".into());
    }
    distinct_web_hosts(&directory, &proof.target_instance_id, &proof.parent_origin)?;
    let issued =
        DateTime::parse_from_rfc3339(&proof.issued_at).map_err(|_| "invalid frame time")?;
    let expires =
        DateTime::parse_from_rfc3339(&proof.expires_at).map_err(|_| "invalid frame time")?;
    let now = Utc::now();
    if expires <= now
        || issued > now + chrono::Duration::seconds(30)
        || expires <= issued
        || expires - issued > chrono::Duration::seconds(TTL_SECONDS)
    {
        return Err("frame proof expired; reopen the instance from the entry workbench".into());
    }
    let source = directory
        .instances
        .iter()
        .find(|source| {
            source.device_id == proof.source_device_id
                && source.instance_id == proof.source_instance_id
        })
        .ok_or("entry binding is no longer in this account")?;
    if !verify_signature(
        &source.instance_id,
        &source.public_key,
        &proof.signature,
        &proof.signing_material(),
    ) {
        return Err("invalid frame signature".into());
    }
    Ok(())
}

fn distinct_web_hosts(
    directory: &ConnectDirectory,
    target_id: &str,
    parent: &str,
) -> Result<(), String> {
    let host = |origin: &str| {
        url::Url::parse(origin)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
    };
    let target = directory
        .instances
        .iter()
        .find(|entry| entry.instance_id == target_id)
        .and_then(|entry| entry.public_origin.as_deref())
        .and_then(host)
        .ok_or("target Remote Access is not enabled")?;
    // HttpOnly cookies are host-scoped, not port-scoped. Distinct names prevent
    // accidental login reuse, but cannot keep the other port from receiving them.
    if host(parent).as_deref() == Some(&target)
        || directory.instances.iter().any(|entry| {
            entry.instance_id != target_id
                && entry.public_origin.as_deref().and_then(host).as_deref() == Some(&target)
        })
    {
        return Err("Connect administrator views require a different hostname for each instance. Use the device's Remote Access address or separate hostnames; different ports alone do not isolate browser credentials.".into());
    }
    Ok(())
}

impl ConnectFrames {
    pub(crate) fn accept(
        &self,
        home: &HomeLayout,
        proof: ConnectFrameProof,
        opening: bool,
    ) -> Result<(), String> {
        verify(home, &proof)?;
        let mut frames = self.0.lock().map_err(|_| "frame registry unavailable")?;
        let now = Utc::now();
        frames.retain(|_, entry| {
            DateTime::parse_from_rfc3339(&entry.expires_at).is_ok_and(|expires| expires > now)
        });
        if let Some(previous) = frames.get(&proof.frame_id) {
            if opening
                || previous.nonce == proof.nonce
                || previous.issued_at >= proof.issued_at
                || previous.account_origin != proof.account_origin
                || previous.source_device_id != proof.source_device_id
                || previous.source_instance_id != proof.source_instance_id
                || previous.parent_origin != proof.parent_origin
                || previous.target_device_id != proof.target_device_id
                || previous.target_instance_id != proof.target_instance_id
            {
                return Err("frame proof was replayed or belongs to another entry".into());
            }
        } else if !opening {
            return Err("frame is no longer active; reopen it from the entry workbench".into());
        }
        if frames.len() >= MAX_FRAMES && !frames.contains_key(&proof.frame_id) {
            return Err("too many active Connect frames".into());
        }
        frames.insert(proof.frame_id.clone(), proof);
        Ok(())
    }

    pub(crate) fn current(
        &self,
        home: &HomeLayout,
        frame_id: &str,
    ) -> Result<ConnectFrameProof, String> {
        let proof = self
            .0
            .lock()
            .map_err(|_| "frame registry unavailable")?
            .get(frame_id)
            .cloned()
            .ok_or("frame is no longer active")?;
        verify(home, &proof)?;
        Ok(proof)
    }
}
