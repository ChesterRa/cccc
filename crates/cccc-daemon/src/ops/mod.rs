pub(crate) mod actor_activity;
pub(crate) mod actor_delivery;
mod actor_delivery_preamble;
mod actor_delivery_render;
mod actor_delivery_worker;
mod actor_listing;
mod actor_profile_runtime;
pub(crate) mod actor_runtime;
#[cfg(test)]
mod actor_runtime_tests;
mod actor_saga;
mod actor_secrets;
mod actors;
mod assistants;
mod automation_config;
mod automation_manage;
mod automation_rule_access;
pub(crate) mod automation_runtime;
mod capabilities;
mod claude_hooks;
mod codex_mcp;
mod context;
mod context_projection;
mod diagnostics;
mod group_bridge;
mod group_copy;
mod group_create_rollback;
mod group_creation;
mod group_reset;
mod group_runtime;
mod group_scopes;
mod group_space;
mod groups;
mod hermes_runtime;
mod im;
pub(crate) mod local_headless;
mod maintenance;
mod memory;
mod message_idempotency;
mod message_metadata;
mod messaging;
mod messaging_inbox;
mod messaging_query;
mod messaging_query_status;
mod messaging_recipients;
mod messaging_status;
mod presentation;
mod profile_access;
mod profiles;
mod remote_access;
mod runtime_completion;
mod runtime_hook_input;
mod runtime_hook_session;
mod runtime_mcp;
pub(crate) mod runtime_restore;
mod runtime_session;
mod runtime_state;
mod settings;
mod terminal;
mod terminal_history_source;
mod terminal_text;
mod working_state;
#[cfg(test)]
mod working_state_tests;

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;

use crate::dispatch::{OpError, OpResult};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Result<Option<OpResult>, OpError> {
    for handler in [
        group_creation::handle,
        groups::handle,
        hermes_runtime::handle,
        group_copy::handle,
        group_bridge::handle,
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
