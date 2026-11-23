//! Establish QUIC before sending a Hello or any application data.
use anyhow::Result;
use cccc_contracts::session_node::SESSION_NODE_ALPN;
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Builder, Connection},
};
use std::time::Duration;

pub(crate) struct Dialer {
    endpoint: Endpoint,
    destination: EndpointAddr,
}

impl Dialer {
    pub(crate) async fn bind(config: (Builder, EndpointAddr)) -> Result<Self> {
        let (builder, destination) = config;
        Ok(Self {
            endpoint: builder.bind().await?,
            destination,
        })
    }

    pub(crate) async fn connect(
        &mut self,
        configure: impl Fn() -> Result<(Builder, EndpointAddr)>,
        announce_retry: bool,
    ) -> Option<Connection> {
        // A replacement bind may have failed on the previous attempt.
        if self.endpoint.is_closed() {
            *self = Self::bind(configure().ok()?).await.ok()?;
        }
        if let Some(connection) = self.dial(self.destination.clone()).await {
            return Some(connection);
        }
        // Only the route that survived proxy policy may offer a relay fallback.
        self.destination.ip_addrs().next()?;
        self.destination.relay_urls().next()?;
        if announce_retry {
            eprintln!(
                "The first connection attempt did not finish. Retrying through the hub relay."
            );
        }
        // iroh remembers earlier IP candidates on an Endpoint. With one-way UDP they can
        // keep interrupting the handshake even when a relay-only destination is supplied.
        self.close().await;
        *self = Self::bind(configure().ok()?).await.ok()?;
        let relay = self.destination.relay_urls().next()?.clone();
        self.dial(EndpointAddr::new(self.destination.id).with_relay_url(relay))
            .await
    }

    async fn dial(&self, destination: EndpointAddr) -> Option<Connection> {
        tokio::time::timeout(
            Duration::from_secs(15),
            self.endpoint.connect(destination, SESSION_NODE_ALPN),
        )
        .await
        .ok()?
        .ok()
    }

    pub(crate) async fn close(&self) {
        self.endpoint.close().await;
    }
}
