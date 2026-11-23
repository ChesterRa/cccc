//! Only a previously authenticated binding can resume a connection selected by a legacy hub URL.
use super::{client, network};
use anyhow::{Result, ensure};
use cccc_contracts::session_node::SessionNodeTicket;
use cccc_core::{
    HomeLayout,
    session_nodes::{self, ClientDescriptor, JoinBinding},
};
use iroh::SecretKey;

enum Resume {
    Saved,
    VerifiedKey(Box<SecretKey>),
    TicketRequired,
}

pub struct JoinTarget {
    home: HomeLayout,
    previous: Option<ClientDescriptor>,
    resume: Resume,
}

impl JoinTarget {
    pub fn local(home: &HomeLayout) -> Result<Self> {
        let previous = client::available_client(home)?;
        let resume = if previous.is_some() {
            Resume::Saved
        } else {
            Resume::TicketRequired
        };
        Ok(Self {
            home: home.clone(),
            previous,
            resume,
        })
    }

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
        let origin = session_nodes::normalize_relay_url(hub).map_err(|_| invalid())?;
        let mut target = Self::local(home)?;
        target.resume = Resume::TicketRequired;
        if let Some(old) = &target.previous
            && !old.grant_id.is_empty()
            && old.join_binding.as_ref().is_some_and(|bound| {
                bound.origin == origin && bound.hub_id == old.endpoint.endpoint_id
            })
        {
            let key = network::key(home, "client")?;
            if old.node_id == key.public().to_string() {
                target.resume = Resume::VerifiedKey(Box::new(key));
            }
        }
        Ok(target)
    }

    pub fn can_resume(&self) -> bool {
        !matches!(self.resume, Resume::TicketRequired)
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

    pub async fn connect(self, ticket: Option<SessionNodeTicket>, name: &str) -> Result<()> {
        ensure!(
            ticket.is_some() || self.can_resume(),
            "A device pairing ticket is required"
        );
        let saved_key = match (ticket.is_none(), self.resume) {
            (true, Resume::VerifiedKey(key)) => Some(*key),
            _ => None,
        };
        client::connect_observed(&self.home, ticket, name, self.previous, saved_key).await
    }
}

pub(super) fn confirmed_binding(
    authenticated_hub: &str,
    node_id: &str,
    previous: Option<&ClientDescriptor>,
) -> Option<JoinBinding> {
    let old = previous
        .filter(|old| old.node_id == node_id && old.endpoint.endpoint_id == authenticated_hub)?;
    old.join_binding
        .as_ref()
        .filter(|bound| bound.hub_id == authenticated_hub)
        .cloned()
}
