//! Owns the loopback relay independently of the public Web listener.
use super::relay_access::RelayAccess;
use anyhow::{Context, Result};
use cccc_core::HomeLayout;
use iroh::EndpointId;
use iroh_relay::{
    KeyCache,
    server::{ClientRateLimit, Metrics, RelayService},
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    num::NonZeroU32,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
};

pub(super) struct LocalRelay {
    pub address: SocketAddr,
    pub service: RelayService,
    access: Arc<RelayAccess>,
    listener: JoinHandle<()>,
    reaper: JoinHandle<()>,
}

impl LocalRelay {
    pub async fn start(home: &HomeLayout, gateway: &str, hub_id: EndpointId) -> Result<Self> {
        let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = socket.local_addr()?;
        let access = Arc::new(RelayAccess::new(home.clone(), gateway.into(), hub_id));
        let rate = ClientRateLimit::new(
            NonZeroU32::new(4 * 1024 * 1024).context("Invalid relay byte limit")?,
        );
        let service = RelayService::new(
            super::relay_probe::handlers(),
            Default::default(),
            Some(rate),
            KeyCache::new(256),
            access.clone(),
            Arc::new(Metrics::default()),
        );
        let accept_service = service.clone();
        let listener = tokio::spawn(async move {
            let permits = Arc::new(Semaphore::new(64));
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
                    accepted = socket.accept() => {
                        let Ok((stream, _)) = accepted else { break; };
                        let Ok(permit) = permits.clone().try_acquire_owned() else { continue; };
                        let service = accept_service.clone();
                        tasks.spawn(async move {
                            let _permit = permit;
                            service.handle_connection(stream, None, Duration::from_secs(10)).await;
                        });
                    }
                }
            }
        });
        let reap_service = service.clone();
        let reap_access = access.clone();
        let reaper = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                reap_access.reap(&reap_service).await;
            }
        });
        Ok(Self {
            address,
            service,
            access,
            listener,
            reaper,
        })
    }

    pub async fn stop(&self) {
        self.access.stopped.store(true, Ordering::Release);
        self.listener.abort();
        self.reaper.abort();
        self.service.shutdown().await;
    }
}

impl Drop for LocalRelay {
    fn drop(&mut self) {
        self.listener.abort();
        self.reaper.abort();
        if !self.access.stopped.swap(true, Ordering::AcqRel)
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let service = self.service.clone();
            runtime.spawn(async move {
                service.shutdown().await;
            });
        }
    }
}
