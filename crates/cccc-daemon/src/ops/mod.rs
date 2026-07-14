pub(crate) mod actor_runtime;
mod actor_secrets;
mod actors;
mod assistants;
mod capabilities;
mod context;
mod diagnostics;
mod group_copy;
mod group_scopes;
mod group_space;
mod groups;
mod im;
mod maintenance;
mod memory;
mod messaging;
mod messaging_inbox;
mod presentation;
mod profiles;
mod remote_access;
mod runtime_state;
mod settings;
mod terminal;

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;

use crate::dispatch::{OpError, OpResult};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Result<Option<OpResult>, OpError> {
    for handler in [
        groups::handle,
        group_copy::handle,
        group_scopes::handle,
        group_space::handle,
        actors::handle,
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
