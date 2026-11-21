use super::store::{self, Invitation};
use crate::HomeLayout;
use cccc_contracts::session_node::{
    SESSION_NODE_PROTOCOL, SESSION_NODE_RELAY_TICKET, SessionNodeTicket,
};
use chrono::{Duration, Utc};
use std::io;

pub fn create_invite(home: &HomeLayout) -> io::Result<SessionNodeTicket> {
    create(home, None, None, None)
}

pub fn create_bound_invite(
    home: &HomeLayout,
    node_id: &str,
    expires_at: &str,
    generation: &str,
) -> io::Result<SessionNodeTicket> {
    let deadline = expires_at
        .parse::<chrono::DateTime<Utc>>()
        .map_err(|_| store::denied())?;
    if generation.is_empty()
        || node_id.is_empty()
        || node_id.len() > 128
        || node_id.chars().any(char::is_control)
        || deadline <= Utc::now()
    {
        return Err(store::denied());
    }
    create(home, Some(node_id), Some(generation), Some(deadline))
}

/// Retract an unpublished ticket after a failed approval commit.
pub fn discard_invite(home: &HomeLayout, id: &str, secret: &str) -> io::Result<()> {
    store::update(home, |state| {
        if state.invites.get(id).is_some_and(|invite| {
            invite.secret_hash
                == store::invitation_hash(invite.expected_node_id.as_deref(), "application", secret)
        }) {
            state.invites.remove(id);
        }
        Ok(())
    })
}

fn create(
    home: &HomeLayout,
    expected: Option<&str>,
    generation: Option<&str>,
    deadline: Option<chrono::DateTime<Utc>>,
) -> io::Result<SessionNodeTicket> {
    let public_relay = super::relay::public_url(home)?;
    store::update(home, |state| {
        let hub = state
            .hub
            .as_ref()
            .filter(|hub| hub.owner.alive())
            .ok_or_else(store::denied)?;
        if expected.is_some() && !hub.bound_invites {
            return Err(io::Error::other(
                "The running device gateway requires an explicit update/restart before device approval",
            ));
        }
        let endpoint = hub.endpoint.clone();
        let relay_url = hub.relay_address.and(public_relay);
        let now = Utc::now();
        if deadline.is_some_and(|deadline| deadline <= now)
            || expected.zip(generation).is_some_and(|(node, generation)| {
                super::join_authority::current(state, node, now) != generation
            })
        {
            return Err(store::denied());
        }

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
        let expires_at = deadline
            .unwrap_or(now + Duration::minutes(10))
            .min(now + Duration::minutes(10))
            .to_rfc3339();
        state.invites.insert(
            invite_id.clone(),
            Invitation {
                secret_hash: store::invitation_hash(expected, "application", &secret),
                expires_at: expires_at.clone(),
                relay_node_id: None,
                expected_node_id: expected.map(str::to_owned),
                relay_secret_hash: relay_token
                    .as_deref()
                    .map(|value| store::invitation_hash(expected, "relay", value)),
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
