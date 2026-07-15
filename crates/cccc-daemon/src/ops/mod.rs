pub(crate) mod actor_delivery;
mod actor_delivery_render;
mod actor_delivery_worker;
pub(crate) mod actor_runtime;
#[cfg(test)]
mod actor_runtime_tests;
mod actor_secrets;
mod actors;
mod assistants;
mod automation_config;
pub(crate) mod automation_runtime;
mod capabilities;
mod codex_mcp;
mod context;
mod diagnostics;
mod group_copy;
mod group_runtime;
mod group_scopes;
mod group_space;
mod groups;
mod im;
mod maintenance;
mod memory;
mod message_idempotency;
mod message_metadata;
mod messaging;
mod messaging_inbox;
mod messaging_query;
mod messaging_status;
mod presentation;
mod profiles;
mod remote_access;
pub(crate) mod runtime_restore;
mod runtime_session;
mod runtime_state;
mod settings;
mod terminal;

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;

use crate::dispatch::{OpError, OpResult};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Result<Option<OpResult>, OpError> {
    actor_delivery::drain(home);
    for handler in [
        groups::handle,
        group_copy::handle,
        group_scopes::handle,
        group_space::handle,
        actors::handle,
        automation_config::handle,
        assistants::handle,
        capabilities::handle,
        messaging::handle,
        presentation::handle,
        profiles::handle,
        diagnostics::handle,
        remote_access::handle,
        runtime_state::handle,
        maintenance::handle,
        im::handle,
        memory::handle,
        context::handle,
        settings::handle,
        terminal::handle,
    ] {
        if let Some(result) = handler(home, request) {
            return Ok(Some(result));
        }
    }
    Ok(None)
}
