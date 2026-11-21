//! Private session-node identity, grants and source directory. Network peers never become users.
mod client;
mod grants;
mod invitations;
mod join_authority;
mod relay;
mod sources;
mod store;
#[cfg(test)]
mod tests;

pub use client::{
    ClientDescriptor, JoinBinding, clear_client, client, saved_client, set_client,
    update_memberships,
};
pub use grants::{pair, revoke, set_connected};
pub use invitations::{create_bound_invite, create_invite, discard_invite};
pub use join_authority::{capture_join_generation, join_generation_matches};
pub use relay::{
    address as relay_address, authorities as relay_authorities, authorize as authorize_relay,
    normalize_url as normalize_relay_url, public_url as relay_public_url,
    register as register_relay,
};
pub use sources::{binding_valid, list_sources, memberships, source, update_sources};
pub use store::{
    bound_invites_supported, clear_endpoint, endpoint, identity, peers, register_endpoint,
    saved_endpoint,
};

pub fn is_remote_source(source_id: &str) -> bool {
    source_id.starts_with(cccc_contracts::session_node::REMOTE_SOURCE_PREFIX)
}
