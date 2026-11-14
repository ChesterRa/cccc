use super::{network, relay_runtime::LocalRelay};
use anyhow::{Result, ensure};
use cccc_client::DaemonClient;
use cccc_contracts::session_node::*;
use cccc_core::{HomeLayout, local_sessions::NativeHost, session_nodes};
use iroh::{Endpoint, endpoint::Connection};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::sync::Mutex;

pub(super) struct Hub {
    pub home: HomeLayout,
    pub daemon: DaemonClient,
    pub endpoint: Endpoint,
    pub gateway: String,
    pub relay: LocalRelay,
    pub(super) connections: Mutex<HashMap<String, (String, Connection)>>,
}

pub(super) fn hubs() -> &'static Mutex<HashMap<PathBuf, Arc<Hub>>> {
    static HUBS: OnceLock<Mutex<HashMap<PathBuf, Arc<Hub>>>> = OnceLock::new();
    HUBS.get_or_init(Mutex::default)
}

pub async fn create_invitation(home: &HomeLayout, port: u16) -> Result<SessionNodeTicket> {
    ensure_gateway(home, port).await?;
    ensure!(
        session_nodes::relay_public_url(home)?.is_none()
            || session_nodes::relay_address(home)?.is_some(),
        "The running device gateway has no relay support; update/restart that gateway explicitly before public pairing"
    );
    Ok(session_nodes::create_invite(home)?)
}

pub async fn restore(home: &HomeLayout) -> Result<()> {
    if session_nodes::saved_endpoint(home)?.is_some()
        && session_nodes::peers(home)?.iter().any(|peer| peer.enabled)
    {
        ensure_gateway(home, 0).await?;
    }
    Ok(())
}

async fn ensure_gateway(home: &HomeLayout, port: u16) -> Result<()> {
    super::capabilities::verify(home, session_nodes::relay_public_url(home)?.is_some()).await?;
    let mut hubs = hubs().lock().await;
    if let Some(hub) = hubs.get(home.root()) {
        session_nodes::register_relay(home, &hub.gateway, hub.relay.address)?;
    }
    if !hubs.contains_key(home.root()) && session_nodes::endpoint(home)?.is_none() {
        let port = if port == 0 {
            session_nodes::saved_endpoint(home)?
                .and_then(|endpoint| {
                    endpoint
                        .addresses
                        .first()
                        .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
                        .map(|address| address.port())
                })
                .unwrap_or(0)
        } else {
            port
        };
        let gateway = uuid::Uuid::new_v4().to_string();
        let key = network::key(home, "hub")?;
        let relay = LocalRelay::start(home, &gateway, key.public()).await?;
        let local_relay = format!("http://{}/", relay.address);
        let endpoint = network::builder(key, port, Some(&local_relay), None)?
            .bind()
            .await?;
        if let Err(error) = session_nodes::register_endpoint(
            home,
            network::address(&endpoint)?,
            NativeHost::capture(std::process::id())?,
            &gateway,
        ) {
            endpoint.close().await;
            return Err(error.into());
        }
        if let Err(error) = session_nodes::register_relay(home, &gateway, relay.address) {
            endpoint.close().await;
            relay.stop().await;
            let _ = session_nodes::clear_endpoint(home, &gateway);
            return Err(error.into());
        }
        let hub = Arc::new(Hub {
            home: home.clone(),
            daemon: DaemonClient::new(home.clone()).without_voice_origin(),
            endpoint,
            gateway,
            relay,
            connections: Mutex::default(),
        });
        hubs.insert(home.root().to_owned(), Arc::clone(&hub));
        tokio::spawn(hub.accept());
    }
    Ok(())
}

pub async fn stop(home: &HomeLayout) {
    if let Some(hub) = hubs().lock().await.remove(home.root()) {
        let _ = session_nodes::clear_endpoint(home, &hub.gateway);
        hub.endpoint.close().await;
        hub.relay.stop().await;
    }
}

pub async fn revoke(home: &HomeLayout, node_id: &str) -> Result<()> {
    session_nodes::revoke(home, node_id)?;
    if let Some(hub) = hubs().lock().await.get(home.root()) {
        if let Some((_, connection)) = hub.connections.lock().await.remove(node_id) {
            connection.close(0u32.into(), b"device revoked");
        }
        if let Ok(id) = node_id.parse() {
            hub.relay.service.clients().disconnect(id, None);
        }
    }
    Ok(())
}
