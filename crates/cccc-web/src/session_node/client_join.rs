//! A browser approval is bound to one validated hub origin, one local key and one saved descriptor.
use super::{client, network, relay_proxy};
use anyhow::{Result, bail, ensure};
use cccc_contracts::session_node::SessionNodeTicket;
use cccc_core::{
    HomeLayout,
    session_nodes::{self, ClientDescriptor, JoinBinding},
};
use iroh::SecretKey;
use std::time::Duration;
use url::Url;

pub struct JoinTarget {
    home: HomeLayout,
    origin: Url,
    key: SecretKey,
    previous: Option<ClientDescriptor>,
}

impl JoinTarget {
    pub fn prepare(home: &HomeLayout, hub: &str) -> Result<Self> {
        let invalid = || {
            anyhow::anyhow!(
                "Use an HTTPS hub URL or an HTTP loopback URL without credentials, a query, or a fragment"
            )
        };
        let authority = hub
            .split_once("://")
            .ok_or_else(invalid)?
            .1
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        ensure!(
            !authority.contains('@') && !hub.contains('\\') && !hub.chars().any(char::is_control),
            "{}",
            invalid()
        );
        let normalized = session_nodes::normalize_relay_url(hub).map_err(|_| invalid())?;
        let origin = Url::parse(&normalized).map_err(|_| invalid())?;
        let previous = client::available_client(home)?;
        let key = network::key(home, "client")?;
        Ok(Self {
            home: home.clone(),
            origin,
            key,
            previous,
        })
    }

    pub fn origin(&self) -> &Url {
        &self.origin
    }

    pub fn node_id(&self) -> String {
        self.key.public().to_string()
    }

    pub fn can_resume(&self) -> bool {
        self.previous.as_ref().is_some_and(|old| {
            !old.grant_id.is_empty()
                && old.node_id == self.node_id()
                && old.join_binding.as_ref().is_some_and(|bound| {
                    bound.origin == self.origin.as_str() && bound.hub_id == old.endpoint.endpoint_id
                })
        })
    }

    pub fn check_current(&self) -> Result<()> {
        client::pairing_state::ensure_current(&self.home, self.previous.as_ref())?;
        client::pairing_state::ensure_idle(&self.home)
    }

    pub fn needs_relay(&self, ticket: Option<&SessionNodeTicket>) -> bool {
        match ticket {
            Some(ticket) => ticket.relay_url.is_some(),
            None => self
                .previous
                .as_ref()
                .is_some_and(|client| client.endpoint.relay_url.is_some()),
        }
    }

    /// Reuse the native connector's destination-specific proxy policy for the approval API.
    pub fn approval_http_client(&self) -> Result<reqwest::Client> {
        let mut http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10));
        match relay_proxy::select(&self.origin) {
            relay_proxy::Selection::Direct => {}
            relay_proxy::Selection::Proxy(url) => {
                let proxy = reqwest::Proxy::all(url)
                    .map_err(|_| anyhow::anyhow!("Invalid device join proxy configuration"))?;
                http = http.proxy(proxy);
            }
            relay_proxy::Selection::Unavailable => bail!(
                "Device join cannot reach this hub with the configured proxy. Configure a supported static HTTP(S) proxy without embedded credentials or an appropriate NO_PROXY rule"
            ),
        }
        http.build()
            .map_err(|_| anyhow::anyhow!("Cannot initialize the device join HTTP client"))
    }

    pub async fn connect(self, ticket: Option<SessionNodeTicket>, name: &str) -> Result<()> {
        ensure!(
            ticket.is_some() || self.can_resume(),
            "This hub requires device approval"
        );
        client::connect_observed(
            &self.home,
            ticket,
            name,
            self.previous,
            Some((self.origin.to_string(), self.key)),
        )
        .await
    }
}

pub(super) fn confirmed_binding(
    origin: Option<&str>,
    authenticated_hub: &str,
    node_id: &str,
    previous: Option<&ClientDescriptor>,
) -> Option<JoinBinding> {
    if let Some(origin) = origin {
        return Some(JoinBinding {
            origin: origin.into(),
            hub_id: authenticated_hub.into(),
        });
    }
    let previous = previous
        .filter(|old| old.node_id == node_id && old.endpoint.endpoint_id == authenticated_hub)?;
    previous
        .join_binding
        .clone()
        .filter(|bound| bound.hub_id == authenticated_hub)
}
