//! Daemon-owned peer discovery. No browser, Web token, or dispatcher permit is involved.
use base64::Engine;
use cccc_contracts::connect::{
    ConnectCatalogPage, ConnectIdentityProof, ConnectPeerOperation, ConnectPeerResponse,
};
use cccc_core::{
    HomeLayout, connect,
    connect_catalog::{self, PeerCatalog},
    connect_peer,
};
use serde::de::DeserializeOwned;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
use tokio::{task::JoinSet, time::Instant};

const MAX_ACTIVE_PEERS: usize = 8;
const MAX_CATALOG_PAGES: usize = 16;

#[cfg(test)]
#[path = "connect_transport_tests.rs"]
pub(crate) mod tests;

#[derive(Default)]
struct PeerSchedule {
    route: String,
    next: Option<Instant>,
    failures: u32,
}

#[path = "connect_transport_delivery.rs"]
mod delivery;

pub(crate) async fn run(home: HomeLayout, locks: crate::dispatch_concurrency::DispatchLocks) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error,"Connect peer HTTP initialization failed");
            return;
        }
    };
    tokio::join!(
        run_catalogs(home.clone(), client.clone()),
        delivery::run(home, client, locks)
    );
}

async fn run_catalogs(home: HomeLayout, client: reqwest::Client) {
    let mut work = JoinSet::new();
    let mut active = HashMap::new();
    let mut schedule: HashMap<String, PeerSchedule> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            Some(completed) = work.join_next_with_id(), if !work.is_empty() => {
                let task_id = completed.as_ref().map_or_else(|error|error.id(),|(id,_)|*id);
                active.remove(&task_id);
                if let Ok((_,(instance_id, route, result))) = completed {
                    if let Some(entry) = schedule.get_mut(&instance_id) {
                        if entry.route != route { continue; }
                        let delay = if let Err(error) = result {
                            tracing::debug!(%instance_id, %error,"Connect peer directory refresh failed");
                            entry.failures = entry.failures.saturating_add(1);
                            (5_u64 << entry.failures.saturating_sub(1).min(4)).min(60)
                        } else { entry.failures=0; 60 };
                        entry.next=Some(Instant::now()+Duration::from_secs(delay));
                    }
                }
            }
            _ = tick.tick() => {
                let Some(snapshot) = connect::load(&home).ok().flatten() else { schedule.clear(); continue; };
                let Some(directory) = snapshot.directory else { schedule.clear(); continue; };
                let mut present = HashSet::new();
                for peer in directory.instances {
                    if peer.instance_id == snapshot.instance_id || peer.public_origin.is_none() { continue; }
                    let route = format!("{}\0{}\0{}\0{}\0{}", snapshot.account_origin,directory.account_id,snapshot.device_id,peer.device_id,peer.public_origin.as_deref().unwrap_or(""));
                    present.insert(peer.instance_id.clone());
                    let entry = schedule.entry(peer.instance_id.clone()).or_default();
                    if entry.route != route { *entry=PeerSchedule {route:route.clone(),..Default::default()}; }
                    if active.values().any(|id|id==&peer.instance_id) || active.len() >= MAX_ACTIVE_PEERS || entry.next.is_some_and(|next|next>Instant::now()) { continue; }
                    let instance_id=peer.instance_id.clone();
                    let home=home.clone(); let client=client.clone();
                    let task=work.spawn(async move {
                        let result=tokio::time::timeout(Duration::from_secs(30), refresh_catalog(&home,&client,&peer.instance_id)).await.unwrap_or_else(|_|Err("peer catalog refresh timed out".into()));
                        (peer.instance_id, route, result)
                    });
                    active.insert(task.id(),instance_id);
                }
                schedule.retain(|key,_|present.contains(key));
            }
        }
    }
}

async fn read_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
    maximum: usize,
) -> Result<T, String> {
    if !response.status().is_success() {
        return Err(format!("peer HTTP status {}", response.status()));
    }
    let mut raw = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "peer response was interrupted")?
    {
        if raw.len() + chunk.len() > maximum {
            return Err("peer response exceeded its size limit".into());
        }
        raw.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&raw).map_err(|_| "invalid peer response".into())
}

async fn refresh_catalog(
    home: &HomeLayout,
    client: &reqwest::Client,
    remote_id: &str,
) -> Result<(), String> {
    let binding = confirm_peer(home, client, remote_id).await?;
    let origin = binding
        .remote
        .public_origin
        .as_deref()
        .ok_or("peer has no remote route")?;
    let mut groups = Vec::new();
    let mut after = None;
    let mut group_ids = HashSet::new();
    for _ in 0..MAX_CATALOG_PAGES {
        let response = exchange(
            home,
            client,
            &binding,
            ConnectPeerOperation::Catalog {
                source_group_id: String::new(),
                target_group_id: None,
                after: after.clone(),
            },
            1024 * 1024 + 4096,
        )
        .await?;
        if response["ok"] != true {
            return Err("peer catalog is unavailable".into());
        }
        let page: ConnectCatalogPage = serde_json::from_value(response["result"].clone())
            .map_err(|_| "invalid peer catalog")?;
        if page.groups.len() > 64 {
            return Err("peer catalog page exceeded 64 Groups".into());
        }
        for group in page.groups {
            if group.group_id.is_empty() || !group_ids.insert(group.group_id.clone()) {
                return Err("peer catalog repeated a Group".into());
            }
            groups.push(group);
        }
        if let Some(next) = page.next {
            if after.as_ref().is_some_and(|cursor| &next <= cursor) {
                return Err("peer catalog cursor did not advance".into());
            }
            after = Some(next);
        } else {
            if serde_json::to_vec(&groups)
                .map_err(|error| error.to_string())?
                .len()
                > 4 * 1024 * 1024
            {
                return Err("peer catalog exceeded 4 MiB".into());
            }
            return connect_catalog::save(
                home,
                &PeerCatalog {
                    account_origin: binding.account_origin,
                    account_id: binding.account_id,
                    local_device_id: binding.local.device_id,
                    remote_instance_id: binding.remote.instance_id,
                    remote_device_id: binding.remote.device_id,
                    remote_origin: origin.into(),
                    checked_at: cccc_contracts::utc_now(),
                    groups,
                },
            )
            .map_err(|error| error.to_string());
        }
    }
    Err("peer catalog exceeded 1024 Groups".into())
}

async fn confirm_peer(
    home: &HomeLayout,
    client: &reqwest::Client,
    remote_id: &str,
) -> Result<connect_peer::PeerBinding, String> {
    let binding = connect_peer::binding(home, remote_id)?;
    let origin = binding
        .remote
        .public_origin
        .as_deref()
        .ok_or("peer has no remote route")?;
    // Prove the actual endpoint before disclosing any source operation or content.
    let nonce = uuid::Uuid::new_v4().to_string();
    let proof: ConnectIdentityProof = read_json(
        client
            .get(format!("{origin}/api/v1/connect/identity"))
            .query(&[("nonce", &nonce)])
            .send()
            .await
            .map_err(|_| "peer is not reachable")?,
        4096,
    )
    .await?;
    connect_peer::verify_identity(&binding.remote, origin, &nonce, &proof)?;
    Ok(binding)
}

async fn exchange(
    home: &HomeLayout,
    client: &reqwest::Client,
    binding: &connect_peer::PeerBinding,
    operation: ConnectPeerOperation,
    maximum: usize,
) -> Result<serde_json::Value, String> {
    let origin = binding
        .remote
        .public_origin
        .as_deref()
        .ok_or("peer has no remote route")?;
    let current = connect_peer::binding(home, &binding.remote.instance_id)?;
    if current.account_origin != binding.account_origin
        || current.account_id != binding.account_id
        || current.local.device_id != binding.local.device_id
        || current.remote.device_id != binding.remote.device_id
        || current.remote.public_origin.as_deref() != Some(origin)
    {
        return Err("peer binding changed after endpoint confirmation".into());
    }
    let envelope = connect_peer::sign_request(home, &binding.remote.instance_id, operation)?;
    let response: serde_json::Value = read_json(
        client
            .post(format!("{origin}/api/v1/connect/peer"))
            .header(
                cccc_contracts::connect::CONNECT_PROOF_HEADER,
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                    serde_json::to_vec(&envelope.proof).map_err(|error| error.to_string())?,
                ),
            )
            .json(&envelope.operation)
            .send()
            .await
            .map_err(|_| "peer request was interrupted")?,
        maximum,
    )
    .await?;
    let response: ConnectPeerResponse =
        serde_json::from_value(response["result"]["response"].clone())
            .map_err(|_| "peer rejected the request")?;
    connect_peer::verify_response(home, &envelope, &response)?;
    Ok(response.result)
}
