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

#[derive(Clone)]
pub(crate) struct PeerClient {
    pub(crate) http: Option<reqwest::Client>,
    pub(crate) direct: crate::direct_channel::Channels,
}
impl From<reqwest::Client> for PeerClient {
    fn from(http: reqwest::Client) -> Self {
        Self {
            http: Some(http),
            direct: Default::default(),
        }
    }
}
impl PeerClient {
    fn http(&self) -> Result<&reqwest::Client, String> {
        self.http
            .as_ref()
            .ok_or_else(|| "Account HTTP transport is unavailable".into())
    }
}

const MAX_ACTIVE_PEERS: usize = 8;
const MAX_CATALOG_PAGES: usize = 16;

#[cfg(test)]
#[path = "connect_transport_group_tests.rs"]
mod group_tests;
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
pub(crate) mod delivery;

pub(crate) async fn run(home: HomeLayout, locks: crate::dispatch_concurrency::DispatchLocks) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => Some(client),
        Err(_) => {
            tracing::warn!(
                "Connect peer HTTP initialization failed; Direct connections remain available"
            );
            None
        }
    };
    let channels = crate::direct_channel::Channels::default();
    let client = PeerClient {
        http: client,
        direct: channels.clone(),
    };
    tokio::join!(
        crate::direct_channel::run(home.clone(), locks.clone(), channels),
        run_catalogs(home.clone(), client.clone()),
        delivery::run(home, client, locks)
    );
}

async fn run_catalogs(home: HomeLayout, client: PeerClient) {
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
                let snapshot=connect::load(&home).ok().flatten();
                let mut present = HashSet::new();
                let mut peers=snapshot.as_ref().and_then(|s|s.directory.as_ref()).map(|d|d.instances.iter().filter(|p|snapshot.as_ref().is_some_and(|s|p.instance_id!=s.instance_id)).cloned().map(|p|(p,None)).collect::<Vec<_>>()).unwrap_or_default();
                if let Some(links)=cccc_core::connect_groups::load(&home).ok().flatten() {
                    for link in &links.links {
                        if let Some((local,remote))=cccc_core::connect_groups::local_endpoint(&links,link)
                            && cccc_core::connect_groups::resource_current(&home,local).unwrap_or(false) {
                            peers.push((remote.instance.clone(),Some(link.id.clone())));
                        }
                    }
                }
                if let Ok(store)=cccc_core::direct::load(&home) {
                    for relation in store.relations {
                        if let Some(remote)=&relation.remote && cccc_core::direct::binding(&home,&remote.instance_id,&relation.id).is_ok() {
                            peers.push((cccc_core::direct::instance(remote,&relation.created_at),Some(relation.id)));
                        }
                    }
                }
                for (peer,connection_id) in peers {
                    let Ok(binding)=connect_peer::scoped_binding(&home,&peer.instance_id,connection_id.as_deref()) else {continue;};
                    let route = format!("{}\0{}\0{}\0{}\0{}", binding.account_origin,binding.account_id,binding.local.device_id,peer.device_id,peer.public_origin.as_deref().unwrap_or("direct"));
                    let key=connection_id.clone().unwrap_or_else(||peer.instance_id.clone());
                    present.insert(key.clone());
                    let entry = schedule.entry(key.clone()).or_default();
                    if entry.route != route { *entry=PeerSchedule {route:route.clone(),..Default::default()}; }
                    if active.values().any(|id|id==&key) || active.len() >= MAX_ACTIVE_PEERS || entry.next.is_some_and(|next|next>Instant::now()) { continue; }
                    let instance_id=key.clone();
                    let home=home.clone(); let client=client.clone();
                    let task=work.spawn(async move {
                        let result=tokio::time::timeout(Duration::from_secs(30), refresh_catalog_scoped(&home,&client,&peer.instance_id,connection_id.as_deref())).await.unwrap_or_else(|_|Err("peer catalog refresh timed out".into()));
                        (key, route, result)
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

#[cfg(test)]
async fn refresh_catalog(
    home: &HomeLayout,
    client: &PeerClient,
    remote_id: &str,
) -> Result<(), String> {
    refresh_catalog_scoped(home, client, remote_id, None).await
}

pub(crate) async fn refresh_catalog_scoped(
    home: &HomeLayout,
    client: &PeerClient,
    remote_id: &str,
    connection_id: Option<&str>,
) -> Result<(), String> {
    let binding = confirm_peer_scoped(home, client, remote_id, connection_id).await?;
    let origin = binding.remote.public_origin.as_deref().unwrap_or_default();
    let mut groups = Vec::new();
    let mut after = None;
    let mut group_ids = HashSet::new();
    for _ in 0..MAX_CATALOG_PAGES {
        let response = exchange(
            home,
            client,
            &binding,
            ConnectPeerOperation::Catalog {
                connection_id: connection_id.map(str::to_owned),
                source_group_id: binding
                    .group
                    .as_ref()
                    .map(|g| g.local_group_id.clone())
                    .unwrap_or_default(),
                target_group_id: binding.group.as_ref().map(|g| g.remote_group_id.clone()),
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
                    connection_id: connection_id.map(str::to_owned),
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

async fn confirm_peer_scoped(
    home: &HomeLayout,
    client: &PeerClient,
    remote_id: &str,
    connection_id: Option<&str>,
) -> Result<connect_peer::PeerBinding, String> {
    let binding = connect_peer::scoped_binding(home, remote_id, connection_id)?;
    if connection_id.is_some_and(cccc_contracts::direct::is_direct) {
        return Ok(binding);
    }
    let origin = binding
        .remote
        .public_origin
        .as_deref()
        .ok_or("peer has no remote route")?;
    // Prove the actual endpoint before disclosing any source operation or content.
    let nonce = uuid::Uuid::new_v4().to_string();
    let proof: ConnectIdentityProof = read_json(
        client
            .http()?
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
    client: &PeerClient,
    binding: &connect_peer::PeerBinding,
    operation: ConnectPeerOperation,
    maximum: usize,
) -> Result<serde_json::Value, String> {
    if binding
        .group
        .as_ref()
        .is_some_and(|g| cccc_contracts::direct::is_direct(&g.id))
    {
        let envelope = connect_peer::sign_request(home, &binding.remote.instance_id, operation)?;
        let response = client.direct.exchange(envelope.clone()).await?;
        if serde_json::to_vec(&response)
            .map_err(|e| e.to_string())?
            .len()
            > maximum
        {
            return Err("Direct response exceeded its size limit".into());
        }
        let response: ConnectPeerResponse =
            serde_json::from_value(response["result"]["response"].clone())
                .map_err(|_| "Direct peer rejected the request")?;
        connect_peer::verify_response(home, &envelope, &response)?;
        return Ok(response.result);
    }
    let origin = binding
        .remote
        .public_origin
        .as_deref()
        .ok_or("peer has no remote route")?;
    let current = connect_peer::scoped_binding(
        home,
        &binding.remote.instance_id,
        binding.group.as_ref().map(|g| g.id.as_str()),
    )?;
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
            .http()?
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
