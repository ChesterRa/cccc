//! What `cccc sessions node connect` resumes or pairs: the saved pairing, or a fresh ticket.
use super::client;
use anyhow::{Result, ensure};
use cccc_contracts::session_node::SessionNodeTicket;
use cccc_core::{HomeLayout, session_nodes::ClientDescriptor};

pub struct ConnectTarget {
    home: HomeLayout,
    previous: Option<ClientDescriptor>,
}

impl ConnectTarget {
    pub fn load(home: &HomeLayout) -> Result<Self> {
        Ok(Self {
            home: home.clone(),
            previous: client::available_client(home)?,
        })
    }

    pub fn can_resume(&self) -> bool {
        self.previous.is_some()
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

    pub async fn connect(
        self,
        ticket: Option<SessionNodeTicket>,
        name: &str,
        requested_runtime: Option<&str>,
    ) -> Result<()> {
        ensure!(
            ticket.is_some() || self.can_resume(),
            "A device pairing ticket is required"
        );
        client::connect_observed(&self.home, ticket, name, self.previous, requested_runtime).await
    }
}
