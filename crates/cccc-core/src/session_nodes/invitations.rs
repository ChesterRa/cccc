use super::store::{self, Invitation};
use crate::HomeLayout;
use cccc_contracts::session_node::{
    SESSION_NODE_PROTOCOL, SESSION_NODE_RELAY_TICKET, SessionNodeTicket,
};
use chrono::{Duration, Utc};
use std::io;

pub fn create_invite(home: &HomeLayout) -> io::Result<SessionNodeTicket> {
    let public_relay = super::relay::public_url(home)?;
    store::update(home, |state| {
        let hub = state
            .hub
            .as_ref()
            .filter(|hub| hub.owner.alive())
            .ok_or_else(store::denied)?;
        let endpoint = hub.endpoint.clone();
        let relay_url = hub.relay_address.and(public_relay);
        let now = Utc::now();
        state.invites.retain(|_, invite| {
            invite
                .expires_at
                .parse::<chrono::DateTime<Utc>>()
                .is_ok_and(|deadline| deadline > now)
        });
        if state.invites.len() >= 32 {
            return Err(io::Error::other("Too many pending device invitations"));
        }
        let invite_id = uuid::Uuid::new_v4().to_string();
        let secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        // The HTTP relay proxy must never receive the end-to-end pairing secret.
        let relay_token = relay_url.as_ref().map(|_| {
            format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )
        });
        let expires_at = (now + Duration::minutes(10)).to_rfc3339();
        state.invites.insert(
            invite_id.clone(),
            Invitation {
                secret_hash: store::hash(&secret),
                expires_at: expires_at.clone(),
                relay_node_id: None,
                expected_node_id: None,
                relay_secret_hash: relay_token.as_deref().map(store::hash),
            },
        );
        Ok(SessionNodeTicket {
            v: if relay_url.is_some() {
                SESSION_NODE_RELAY_TICKET
            } else {
                SESSION_NODE_PROTOCOL
            },
            endpoint_id: endpoint.endpoint_id,
            addresses: endpoint.addresses,
            relay_url,
            relay_token,
            invite_id,
            secret,
            expires_at,
        })
    })
}
