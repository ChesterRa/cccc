//! Private session-node identity, grants and source directory. Network peers never become users.
mod client;
mod grants;
mod invitations;
mod relay;
mod sources;
mod store;
#[cfg(test)]
mod tests;

pub use client::{
    ClientDescriptor, clear_client, client, saved_client, set_client, update_memberships,
};
pub use grants::{pair, revoke, set_connected};
pub use invitations::create_invite;
pub use relay::{
    address as relay_address, authorities as relay_authorities, authorize as authorize_relay,
    normalize_url as normalize_relay_url, public_url as relay_public_url,
    register as register_relay, status as relay_status, unregister as unregister_relay,
};
pub use sources::{
    binding_valid, list_device_sources, list_sources, memberships, source, update_sources,
};
pub use store::{
    StateReadError, clear_endpoint, endpoint, gateway_stopped, identity, peers, register_endpoint,
    relay_enabled, saved_endpoint, set_relay_enabled, stop_gateway,
};

use cccc_contracts::local_session::SessionSourceKind;

/// Decode the reserved namespace at the boundary. The resulting kind is not authority.
pub fn source_kind(source_id: &str) -> SessionSourceKind {
    if source_id.starts_with(cccc_contracts::session_node::REMOTE_SOURCE_PREFIX) {
        SessionSourceKind::Device
    } else {
        SessionSourceKind::Local
    }
}

pub fn is_remote_source(source_id: &str) -> bool {
    source_kind(source_id) == SessionSourceKind::Device
}
