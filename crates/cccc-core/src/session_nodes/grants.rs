use super::store::{self, Invitation};
use crate::HomeLayout;
use cccc_contracts::session_node::{
    SESSION_NODE_PROTOCOL, SESSION_NODE_RELAY_TICKET, SessionNodePeer, SessionNodeTicket,
};
use chrono::{Duration, Utc};
use std::io;

// Disconnection retains authority; only explicit revocation frees an active slot.
const MAX_ACTIVE_DEVICES: usize = 32;
const MAX_REVOKED_HISTORY: usize = 32;

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

pub fn pair(
    home: &HomeLayout,
    gateway: &str,
    node_id: &str,
    name: &str,
    invite: Option<(&str, &str)>,
) -> io::Result<SessionNodePeer> {
    if node_id.is_empty()
        || node_id.len() > 128
        || name.trim().is_empty()
        || name.len() > 160
        || name.chars().any(char::is_control)
    {
        return Err(store::denied());
    }
    store::update(home, |state| {
        store::check_gateway(state, gateway)?;
        let now = Utc::now();
        if state.peers.get(node_id).is_some_and(|peer| peer.enabled) {
            // A resumed enrollment may carry a consumed ticket after a lost reply.
            // Consume a valid fresh ticket without replacing the existing grant.
            if let Some((id, secret)) = invite
                && state
                    .invites
                    .get(id)
                    .is_some_and(|pending| valid_invite(pending, secret))
            {
                state.invites.remove(id);
            }
            let peer = state.peers.get_mut(node_id).ok_or_else(store::denied)?;
            peer.connected = true;
            peer.last_seen_at = now.to_rfc3339();
            let peer = peer.clone();
            prune_revoked_history(state);
            return Ok(peer);
        }
        let (id, secret) = invite.ok_or_else(store::denied)?;
        let pending = state.invites.get(id).ok_or_else(store::denied)?;
        if !valid_invite(pending, secret) {
            return Err(store::denied());
        }
        if state.peers.values().filter(|peer| peer.enabled).count() >= MAX_ACTIVE_DEVICES {
            return Err(io::Error::other("Session-node device limit reached"));
        }
        state.invites.remove(id);
        let peer = SessionNodePeer {
            node_id: node_id.into(),
            name: name.trim().into(),
            grant_id: uuid::Uuid::new_v4().to_string(),
            enabled: true,
            connected: true,
            granted_at: now.to_rfc3339(),
            last_seen_at: now.to_rfc3339(),
        };
        state.peers.insert(node_id.into(), peer.clone());
        prune_revoked_history(state);
        Ok(peer)
    })
}

pub fn set_connected(
    home: &HomeLayout,
    gateway: &str,
    node_id: &str,
    connected: bool,
) -> io::Result<()> {
    store::update(home, |state| {
        store::check_gateway(state, gateway)?;
        let peer = state.peers.get_mut(node_id).ok_or_else(store::denied)?;
        if connected && !peer.enabled {
            return Err(store::denied());
        }
        peer.connected = connected;
        if connected {
            peer.last_seen_at = Utc::now().to_rfc3339();
        }
        Ok(())
    })
}

pub fn revoke(home: &HomeLayout, node_id: &str) -> io::Result<()> {
    store::update(home, |state| {
        let peer = state.peers.get_mut(node_id).ok_or_else(store::denied)?;
        peer.enabled = false;
        peer.connected = false;
        prune_revoked_history(state);
        Ok(())
    })
}

/// History is informational, not a revocation denylist: absent devices also
/// require a fresh, unconsumed invitation. Re-enrollment always rotates grant_id,
/// so neither retained nor evicted sources can restore old Group bindings.
/// Keep the most recently granted revoked identities, with a stable tie-break.
fn prune_revoked_history(state: &mut store::State) {
    let mut revoked = state
        .peers
        .values()
        .filter(|peer| !peer.enabled)
        .map(|peer| {
            (
                peer.granted_at.parse::<chrono::DateTime<Utc>>().ok(),
                peer.node_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    revoked.sort();
    let excess = revoked.len().saturating_sub(MAX_REVOKED_HISTORY);
    for (_, node_id) in revoked.into_iter().take(excess) {
        state.peers.remove(&node_id);
    }
    state
        .sources
        .retain(|_, source| state.peers.contains_key(&source.node_id));
}

fn valid_invite(invite: &Invitation, secret: &str) -> bool {
    invite.secret_hash == store::hash(secret)
        && invite
            .expires_at
            .parse::<chrono::DateTime<Utc>>()
            .is_ok_and(|deadline| deadline > Utc::now())
}
