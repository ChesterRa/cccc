//! Authenticated direct/relay iroh transport for existing native sessions. Group authority stays
//! on the hub and provider context/approvals stay on the original machine.
mod broker;
mod capabilities;
mod client;
mod client_receive;
mod hub;
mod hub_connection;
mod hub_delivery;
mod hub_rpc;
mod network;
mod protocol;
mod relay_access;
mod relay_copy;
mod relay_http;
mod relay_probe;
mod relay_runtime;

pub use client::{connect, stop_signal};
pub use hub::{create_invitation, restore, revoke, stop};
pub use network::{decode as decode_ticket, encode as encode_ticket};

pub(crate) use relay_http::routes as relay_routes;

#[cfg(test)]
mod relay_tests;

pub use capabilities::verify as verify_daemon;
