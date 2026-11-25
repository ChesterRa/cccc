//! Authenticated direct/relay iroh transport for existing native sessions. Group authority stays
//! on the hub and provider context/approvals stay on the original machine. The hub gateway runs
//! inside the daemon process; the Web only forwards the public relay route.
#![cfg_attr(not(test), deny(clippy::disallowed_methods))]

mod broker;
mod capabilities;
mod client;
mod client_dial;
mod client_receive;
mod client_target;
mod host;
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
mod relay_proxy;
mod relay_proxy_system;
mod relay_runtime;

pub use capabilities::verify as verify_daemon;
pub use client::{connect, stop_signal};
pub use client_target::ConnectTarget;
pub use host::Host;
pub use hub::{create_invitation, restore, revoke, set_relay, stop, stop_explicitly};
pub use network::{decode as decode_ticket, encode as encode_ticket};
pub use relay_http::routes as relay_routes;

#[cfg(test)]
mod relay_tests;

/// Tests that run a daemon in this process share its runtime manager and the hub
/// registry, so one daemon's startup or shutdown disturbs another's. They hold
/// this lock for as long as their daemon runs.
#[cfg(test)]
pub(crate) static DAEMON_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
