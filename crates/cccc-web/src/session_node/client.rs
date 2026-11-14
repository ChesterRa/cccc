#[path = "client_pairing_state.rs"]
mod pairing_state;

use super::{
    broker,
    client_receive::run_session,
    network,
    protocol::{self, Request},
};
use anyhow::{Context, Result, ensure};
use cccc_client::DaemonClient;
use cccc_contracts::session_node::*;
use cccc_core::{
    HomeLayout,
    local_sessions::NativeHost,
    session_nodes::{self, ClientDescriptor},
};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::RwLock};

pub async fn connect(
    home: &HomeLayout,
    ticket: Option<SessionNodeTicket>,
    name: &str,
) -> Result<()> {
    let saved = session_nodes::saved_client(home)?;
    ensure!(
        !saved.as_ref().is_some_and(|client| client.owner.alive()),
        "A native session-node connector already runs for this Home"
    );
    let endpoint_address = match &ticket {
        Some(ticket) => SessionNodeEndpoint {
            endpoint_id: ticket.endpoint_id.clone(),
            addresses: ticket.addresses.clone(),
            relay_url: ticket.relay_url.clone(),
        },
        None => saved
            .as_ref()
            .context("Pairing ticket required for the first connection")?
            .endpoint
            .clone(),
    };
    super::capabilities::verify(home, endpoint_address.relay_url.is_some()).await?;
    let destination = network::destination(&endpoint_address)?;
    let relay_token = ticket.as_ref().and_then(|ticket| {
        ticket.relay_token.as_ref().map(|token| {
            format!(
                "{}{}:{}",
                super::relay_access::AUTH_PREFIX,
                ticket.invite_id,
                token
            )
        })
    });
    let endpoint = network::builder(
        network::key(home, "client")?,
        0,
        endpoint_address.relay_url.as_deref(),
        relay_token.as_deref(),
    )?
    .bind()
    .await?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let descriptor = ClientDescriptor {
        endpoint: endpoint_address,
        node_id: endpoint.id().to_string(),
        grant_id: String::new(),
        owner: NativeHost::capture(std::process::id())?,
        address: listener.local_addr()?,
        secret: uuid::Uuid::new_v4().to_string(),
        connected: false,
        memberships: Vec::new(),
    };
    // Keep the prior route and grant intact until Hello has been accepted.
    let mut expected = saved;
    let link = Arc::new(RwLock::new(None));
    let broker = tokio::spawn(broker::serve(
        home.clone(),
        descriptor.secret.clone(),
        listener,
        Arc::clone(&link),
    ));
    let _cleanup = ConnectorCleanup {
        home: home.clone(),
        secret: descriptor.secret.clone(),
        broker,
    };
    let daemon = DaemonClient::new(home.clone()).without_voice_origin();
    let mut invitation = ticket;
    let mut backoff = 1;
    // Keep one signal subscription alive across connect, Hello, active work
    // and backoff; dropping it between phases can lose a user's single Ctrl-C.
    let mut stop = std::pin::pin!(stop_signal());
    let result = loop {
        let connected = tokio::select! {
            _ = &mut stop => break Ok(()),
            result = tokio::time::timeout(Duration::from_secs(15), endpoint.connect(destination.clone(), SESSION_NODE_ALPN)) => result,
        };
        if let Ok(Ok(connection)) = connected {
            let hello_request = Request::Hello {
                version: SESSION_NODE_PROTOCOL,
                name: name.into(),
                invite_id: invitation.as_ref().map(|ticket| ticket.invite_id.clone()),
                secret: invitation.as_ref().map(|ticket| ticket.secret.clone()),
            };
            let hello = tokio::select! {
                _ = &mut stop => { connection.close(0u32.into(), b"connector stopped"); break Ok(()); },
                result = protocol::call(&connection, &hello_request) => result,
            };
            match hello {
                Ok(hello) => {
                    let peer: SessionNodePeer = serde_json::from_value(hello["peer"].clone())?;
                    ensure!(
                        peer.node_id == endpoint.id().to_string(),
                        "Paired node identity mismatch"
                    );
                    let mut current = descriptor.clone();
                    current.grant_id = peer.grant_id;
                    current.connected = true;
                    current.memberships = serde_json::from_value(hello["memberships"].clone())?;
                    pairing_state::commit(home, expected.as_ref(), &current)?;
                    invitation = None;
                    *link.write().await = Some(connection.clone());
                    backoff = 1;
                    eprintln!(
                        "Session node connected. Select its sessions in the hub's Connect sessions dialog. Press Ctrl-C to disconnect; native applications keep running."
                    );
                    let session = run_session(home, &daemon, &descriptor.secret, &connection);
                    tokio::select! { _ = &mut stop => { connection.close(0u32.into(), b"connector stopped"); break Ok(()); }, result = session => { if result.is_err() { eprintln!("Session node disconnected; pending writes will not be replayed."); } } }
                    connection.close(0u32.into(), b"connector reconnecting");
                    *link.write().await = None;
                    if let Some(current) = session_nodes::saved_client(home)? {
                        session_nodes::update_memberships(
                            home,
                            &descriptor.secret,
                            false,
                            current.memberships,
                        )?;
                    }
                    expected = pairing_state::reconnect_state(home, &descriptor.secret)?;
                }
                Err(error) if error.downcast_ref::<protocol::RemoteRejection>().is_some() => {
                    connection.close(0u32.into(), b"pairing rejected");
                    break Err(anyhow::anyhow!(
                        "Session-node pairing was rejected or its outcome is unknown. If the device is not listed on the hub, create a new pairing ticket; revoked devices require a new invitation."
                    ));
                }
                Err(_) => {
                    // A transport loss during enrollment/resume is not proof
                    // that the hub rejected the key. Retry only the handshake.
                    connection.close(0u32.into(), b"handshake interrupted");
                }
            }
        }
        tokio::select! { _ = &mut stop => break Ok(()), _ = tokio::time::sleep(Duration::from_secs(backoff)) => {} }
        backoff = (backoff * 2).min(15);
    };
    *link.write().await = None;
    endpoint.close().await;
    // Retain only the route and membership hints for explicit reconnect and
    // useful offline errors. A dead owner cannot authorize native tool calls.
    if let Ok(Some(current)) = session_nodes::saved_client(home) {
        let _ =
            session_nodes::update_memberships(home, &descriptor.secret, false, current.memberships);
    }
    result
}

struct ConnectorCleanup {
    home: HomeLayout,
    secret: String,
    broker: tokio::task::JoinHandle<()>,
}

impl Drop for ConnectorCleanup {
    fn drop(&mut self) {
        self.broker.abort();
        if let Ok(Some(current)) = session_nodes::saved_client(&self.home) {
            let _ = session_nodes::update_memberships(
                &self.home,
                &self.secret,
                false,
                current.memberships,
            );
        }
    }
}

pub async fn stop_signal() {
    #[cfg(unix)]
    if let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
#[path = "client_pairing_tests.rs"]
mod pairing_tests;
