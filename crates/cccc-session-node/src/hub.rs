//! The hub gateway, owned by the daemon process. One live gateway per Home; the loopback relay
//! runs only while the administrator's relay switch is on and Remote Access offers a public origin.
use super::{network, relay_runtime::LocalRelay};
use anyhow::{Result, bail};
use cccc_client::DaemonClient;
use cccc_contracts::session_node::*;
use cccc_core::{HomeLayout, local_sessions::NativeHost, session_nodes};
use iroh::{Endpoint, RelayConfig, RelayUrl, endpoint::Connection};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::sync::Mutex;

pub(crate) struct Hub {
    pub home: HomeLayout,
    pub daemon: DaemonClient,
    pub endpoint: Endpoint,
    pub gateway: String,
    pub relay: Mutex<Option<(RelayUrl, LocalRelay)>>,
    pub(crate) connections: Mutex<HashMap<String, (String, Connection)>>,
}

/// A live gateway on another port cannot be rebound while devices use it.
#[derive(Debug)]
pub struct PortConflict;

impl std::fmt::Display for PortConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The device gateway is already running on another port")
    }
}

impl std::error::Error for PortConflict {}

pub(crate) fn hubs() -> &'static Mutex<Hubs> {
    static HUBS: OnceLock<Mutex<Hubs>> = OnceLock::new();
    HUBS.get_or_init(Mutex::default)
}

type Hubs = HashMap<PathBuf, Arc<Hub>>;

/// The only manual way to open a stopped gateway: start it, then record one ten-minute ticket.
/// The gateway lock covers both steps, so an explicit stop sees either nothing or a ticket.
pub async fn create_invitation(home: &HomeLayout, port: u16) -> Result<SessionNodeTicket> {
    let mut hubs = hubs().lock().await;
    let opened = ensure_gateway(&mut hubs, home, port).await?;
    match session_nodes::create_invite(home) {
        Ok(ticket) => Ok(ticket),
        Err(error) => {
            if opened && let Some(hub) = hubs.remove(home.root()) {
                let _ = session_nodes::clear_endpoint(home, &hub.gateway);
                close(&hub).await;
            }
            Err(error.into())
        }
    }
}

/// Reopen a configured gateway on daemon start, unless the administrator stopped it. The stop
/// flag is read under the gateway lock, in the same order as an explicit stop.
pub async fn restore(home: &HomeLayout) -> Result<()> {
    let mut hubs = hubs().lock().await;
    if !session_nodes::gateway_stopped(home)?
        && session_nodes::saved_endpoint(home)?.is_some()
        && session_nodes::peers(home)?.iter().any(|peer| peer.enabled)
    {
        ensure_gateway(&mut hubs, home, 0).await?;
    }
    Ok(())
}

fn saved_port(home: &HomeLayout) -> Result<u16> {
    Ok(session_nodes::saved_endpoint(home)?
        .and_then(|endpoint| {
            endpoint
                .addresses
                .first()
                .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
                .map(|address| address.port())
        })
        .unwrap_or(0))
}

/// Returns whether this call opened the gateway.
async fn ensure_gateway(hubs: &mut Hubs, home: &HomeLayout, port: u16) -> Result<bool> {
    if let Some(hub) = hubs.get(home.root()) {
        let bound = hub
            .endpoint
            .bound_sockets()
            .into_iter()
            .find(|address| address.is_ipv4())
            .map(|address| address.port());
        if port != 0 && bound != Some(port) {
            return Err(PortConflict.into());
        }
        sync_relay(hub).await?;
        return Ok(false);
    }
    if session_nodes::endpoint(home)?.is_some() {
        bail!("Another process already owns the device gateway for this Home");
    }
    let port = if port == 0 { saved_port(home)? } else { port };
    let gateway = uuid::Uuid::new_v4().to_string();
    let key = network::key(home, "hub")?;
    let endpoint = bind(key, port).await?;
    if let Err(error) = session_nodes::register_endpoint(
        home,
        network::address(&endpoint)?,
        NativeHost::capture(std::process::id())?,
        &gateway,
    ) {
        endpoint.close().await;
        return Err(error.into());
    }
    let hub = Arc::new(Hub {
        home: home.clone(),
        daemon: DaemonClient::new(home.clone()).without_voice_origin(),
        endpoint,
        gateway,
        relay: Mutex::default(),
        connections: Mutex::default(),
    });
    if let Err(error) = sync_relay(&hub).await {
        let _ = session_nodes::clear_endpoint(home, &hub.gateway);
        close(&hub).await;
        return Err(error);
    }
    hubs.insert(home.root().to_owned(), Arc::clone(&hub));
    tokio::spawn(hub.accept());
    Ok(true)
}

/// A saved UDP port may still be held by the daemon this one replaces during an update handover.
async fn bind(key: iroh::SecretKey, port: u16) -> Result<Endpoint> {
    let mut attempt = 0;
    loop {
        match network::hub_builder(key.clone(), port)?.bind().await {
            Ok(endpoint) => return Ok(endpoint),
            Err(error) if port != 0 && attempt < 10 => {
                attempt += 1;
                tracing::debug!(%error, attempt, "Device gateway port is busy; retrying");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Start or stop the loopback relay to match the switch and the Remote Access origin.
async fn sync_relay(hub: &Hub) -> Result<()> {
    let wanted = session_nodes::relay_enabled(&hub.home)?
        && session_nodes::relay_public_url(&hub.home)?.is_some();
    let mut relay = hub.relay.lock().await;
    match (wanted, relay.is_some()) {
        (true, false) => {
            let local = LocalRelay::start(&hub.home, &hub.gateway, hub.endpoint.id()).await?;
            let url: RelayUrl = format!("http://{}/", local.address).parse()?;
            hub.endpoint
                .insert_relay(url.clone(), Arc::new(RelayConfig::new(url.clone(), None)))
                .await;
            if let Err(error) =
                session_nodes::register_relay(&hub.home, &hub.gateway, local.address)
            {
                hub.endpoint.remove_relay(&url).await;
                local.stop().await;
                return Err(error.into());
            }
            *relay = Some((url, local));
        }
        (false, true) => {
            // Withdraw the registered address first, so status never points at a closed listener.
            session_nodes::unregister_relay(&hub.home, &hub.gateway)?;
            if let Some((url, local)) = relay.take() {
                hub.endpoint.remove_relay(&url).await;
                local.stop().await;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn close(hub: &Hub) {
    hub.endpoint.close().await;
    if let Some((_, local)) = hub.relay.lock().await.take() {
        local.stop().await;
    }
}

/// Release this process's gateway on daemon shutdown; the restore intent stays.
pub async fn stop(home: &HomeLayout) {
    if let Some(hub) = hubs().lock().await.remove(home.root()) {
        let _ = session_nodes::clear_endpoint(home, &hub.gateway);
        close(&hub).await;
    }
}

/// The administrator's explicit stop: persisted across restarts until the next invitation.
pub async fn stop_explicitly(home: &HomeLayout) -> Result<()> {
    let mut hubs = hubs().lock().await;
    let token = hubs.get(home.root()).map(|hub| hub.gateway.clone());
    session_nodes::stop_gateway(home, token.as_deref())?;
    if let Some(hub) = hubs.remove(home.root()) {
        close(&hub).await;
    }
    Ok(())
}

pub async fn revoke(home: &HomeLayout, node_id: &str) -> Result<()> {
    session_nodes::revoke(home, node_id)?;
    if let Some(hub) = hubs().lock().await.get(home.root()) {
        if let Some((_, connection)) = hub.connections.lock().await.remove(node_id) {
            connection.close(0u32.into(), b"device revoked");
        }
        if let (Ok(id), Some((_, local))) = (node_id.parse(), hub.relay.lock().await.as_ref()) {
            local.service.clients().disconnect(id, None);
        }
    }
    Ok(())
}

/// Save the relay switch, then bring a live gateway in line with it.
pub async fn set_relay(home: &HomeLayout, enabled: bool) -> Result<()> {
    if enabled && session_nodes::relay_public_url(home)?.is_none() {
        return Err(RelayUnavailable.into());
    }
    let previous = session_nodes::relay_enabled(home)?;
    session_nodes::set_relay_enabled(home, enabled)?;
    if let Some(hub) = hubs().lock().await.get(home.root())
        && let Err(error) = sync_relay(hub).await
    {
        // A relay that could not start leaves the switch as it was.
        let _ = session_nodes::set_relay_enabled(home, previous);
        let _ = sync_relay(hub).await;
        return Err(error);
    }
    Ok(())
}

/// Remote Access settings changed: start or stop a live gateway's relay to match. A failure keeps
/// the previous relay state and is logged; the next change or restart tries again.
pub async fn converge_relay(home: &HomeLayout) {
    if let Some(hub) = hubs().lock().await.get(home.root())
        && let Err(error) = sync_relay(hub).await
    {
        tracing::warn!(%error, "Could not follow the Remote Access change with the device relay");
    }
}

/// Enabling the relay needs an active Remote Access origin that relays can use.
#[derive(Debug)]
pub struct RelayUnavailable;

impl std::fmt::Display for RelayUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "Turn on Remote Access with a supported HTTPS address before enabling the public relay",
        )
    }
}

impl std::error::Error for RelayUnavailable {}
