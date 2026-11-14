use super::super::{network, protocol, relay_http};
use cccc_client::DaemonClient;
use cccc_contracts::{
    DaemonRequest,
    local_session::{LocalSessionRuntime, LocalSessionSource},
    session_node::*,
};
use cccc_core::{HomeLayout, settings};
use iroh::{Endpoint, SecretKey, Watcher, endpoint::Connection};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{net::TcpListener, sync::broadcast, task::JoinHandle};

pub struct Fixture {
    temp: Option<tempfile::TempDir>,
    pub home: HomeLayout,
    pub ticket: SessionNodeTicket,
    pub client: DaemonClient,
    shutdown: broadcast::Sender<()>,
    proxy: Option<JoinHandle<()>>,
    daemon: Option<JoinHandle<anyhow::Result<()>>>,
}

pub async fn until(mut predicate: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !predicate().await {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("observable relay state transition");
}

impl Fixture {
    pub async fn new() -> Self {
        let temp = tempfile::tempdir().expect("isolated relay fixture");
        let home = HomeLayout::from_path(temp.path().join("hub")).expect("hub");
        let node = HomeLayout::from_path(temp.path().join("node")).expect("node");
        home.initialize().expect("initialize");
        node.initialize().expect("initialize node");
        let daemon_home = home.clone();
        let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        let client = DaemonClient::new(home.clone()).without_voice_origin();
        until(async || {
            client
                .call(&DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                })
                .await
                .is_ok()
        })
        .await;
        let port = std::env::var("SESSION_NODE_RELAY_TEST_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(0);
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .expect("public facade");
        let url = std::env::var("SESSION_NODE_RELAY_TEST_URL")
            .ok()
            .map(|value| format!("{}/", value.trim_end_matches('/')))
            .unwrap_or_else(|| format!("http://{}/", listener.local_addr().expect("address")));
        settings::update(&home, |settings| {
            settings.remote_access.extend(
                json!({"enabled":true,"provider":"manual","web_public_url":url})
                    .as_object()
                    .expect("config")
                    .clone(),
            );
            Ok(())
        })
        .expect("private route config");
        let (shutdown, _) = broadcast::channel(4);
        let routes = relay_http::routes(home.clone(), false, shutdown.clone());
        let mut stop = shutdown.subscribe();
        let proxy = tokio::spawn(async move {
            axum::serve(listener, routes)
                .with_graceful_shutdown(async move {
                    let _ = stop.recv().await;
                })
                .await
                .expect("relay facade");
        });
        let ticket = super::super::hub::create_invitation(&home, 0)
            .await
            .expect("gateway");
        assert_eq!(ticket.v, SESSION_NODE_RELAY_TICKET);
        assert_eq!(ticket.relay_url.as_deref(), Some(url.as_str()));
        Self {
            temp: Some(temp),
            home,
            ticket,
            client,
            shutdown,
            proxy: Some(proxy),
            daemon: Some(daemon),
        }
    }

    pub async fn endpoint(&self, key: SecretKey, ticket: Option<&SessionNodeTicket>) -> Endpoint {
        let token = ticket.map(|ticket| {
            format!(
                "{}{}:{}",
                super::super::relay_access::AUTH_PREFIX,
                ticket.invite_id,
                ticket.relay_token.as_deref().expect("relay capability")
            )
        });
        let endpoint = network::builder(key, 0, self.ticket.relay_url.as_deref(), token.as_deref())
            .expect("production endpoint configuration")
            .ca_tls_config(test_ca())
            .clear_ip_transports()
            .bind()
            .await
            .expect("relay-only endpoint");
        assert!(
            endpoint.bound_sockets().is_empty(),
            "No UDP transport can satisfy this test"
        );
        endpoint
    }

    pub async fn hello(
        &self,
        endpoint: &Endpoint,
        ticket: Option<&SessionNodeTicket>,
    ) -> (Connection, Value) {
        let destination = network::destination(&SessionNodeEndpoint {
            endpoint_id: self.ticket.endpoint_id.clone(),
            addresses: vec![],
            relay_url: self.ticket.relay_url.clone(),
        })
        .expect("relay-only destination");
        // EndpointAddr contains no direct candidates and the endpoint has no IP sockets.
        let connection = match tokio::time::timeout(
            Duration::from_secs(20),
            endpoint.connect(destination, SESSION_NODE_ALPN),
        )
        .await
        {
            Ok(Ok(connection)) => connection,
            other => {
                let hubs = super::super::hub::hubs().lock().await;
                let hub = hubs.get(self.home.root()).expect("fixture hub");
                panic!(
                    "Relay connection failed: {other:?}; node status {:?}; hub status {:?}",
                    endpoint.home_relay_status().get(),
                    hub.endpoint.home_relay_status().get()
                );
            }
        };
        let hello = protocol::call(
            &connection,
            &protocol::Request::Hello {
                version: SESSION_NODE_PROTOCOL,
                name: "Relay laptop".into(),
                invite_id: ticket.map(|ticket| ticket.invite_id.clone()),
                secret: ticket.map(|ticket| ticket.secret.clone()),
            },
        )
        .await
        .expect("application enrollment");
        (connection, hello)
    }

    pub async fn close(mut self) {
        super::super::hub::stop(&self.home).await;
        let _ = self.shutdown.send(());
        if let Some(proxy) = self.proxy.take() {
            proxy.abort();
            let _ = proxy.await;
        }
        let _ = self
            .client
            .call(&DaemonRequest {
                v: 1,
                op: "shutdown".into(),
                args: Default::default(),
            })
            .await;
        if let Some(daemon) = self.daemon.take() {
            tokio::time::timeout(Duration::from_secs(5), daemon)
                .await
                .expect("fixture daemon stops")
                .expect("daemon task")
                .expect("daemon shutdown");
        }
        self.temp.take();
    }
}

pub fn advertised() -> SessionNodeSource {
    SessionNodeSource {
        source: LocalSessionSource {
            source_id: "codex:relay-source".into(),
            runtime: LocalSessionRuntime::Codex,
            native_session_id: "relay-source".into(),
            title: "Relay transport fixture".into(),
            cwd: "/node-only/project".into(),
            origin: Some("cli".into()),
            connected: true,
        },
        session_revision: "relay-revision".into(),
        alive: true,
    }
}

pub fn relay_client(
    endpoint: &Endpoint,
    url: &str,
    key: SecretKey,
) -> iroh_relay::client::ClientBuilder {
    let tls = test_ca().client_config(std::sync::Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ));
    iroh_relay::client::ClientBuilder::new(
        url.parse::<iroh::RelayUrl>().expect("relay URL"),
        key,
        endpoint.dns_resolver().expect("resolver").clone(),
    )
    .tls_client_config(tls.expect("relay TLS configuration"))
}

// Manual TLS-offload acceptance can trust its own private CA without changing
// system trust or weakening certificate verification. CI uses public roots.
fn test_ca() -> iroh_relay::tls::CaTlsConfig {
    std::env::var_os("SESSION_NODE_RELAY_TEST_CA").map_or_else(Default::default, |path| {
        let der = std::fs::read(path).expect("manual relay CA certificate");
        iroh_relay::tls::CaTlsConfig::custom_roots([rustls::pki_types::CertificateDer::from(der)])
    })
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(proxy) = &self.proxy {
            proxy.abort();
        }
        if let Some(daemon) = &self.daemon {
            daemon.abort();
        }
        if let Some(temp) = self.temp.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let home = self.home.clone();
            runtime.spawn(async move {
                super::super::hub::stop(&home).await;
                drop(temp);
            });
        }
    }
}
