//! Web Push notifications for messages and reply requests addressed to the
//! user. The Web process owns subscriptions and sending, like the IM bridges,
//! and announces what happens while it runs: a message once, best effort,
//! and an open request again after a push service could not take it.
mod crypto;
mod endpoint;
mod notifier;
mod routes;
mod sender;
mod store;

pub(crate) use notifier::{Runtime, spawn};
pub(crate) use routes::routes;
