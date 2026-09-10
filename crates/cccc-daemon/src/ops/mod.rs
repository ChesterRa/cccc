pub(crate) mod actor_activity;
pub(crate) mod actor_delivery;
mod actor_delivery_preamble;
mod actor_delivery_render;
mod actor_delivery_worker;
mod actor_listing;
mod actor_profile_runtime;
pub(crate) mod actor_runtime;
mod actor_runtime_status;
#[cfg(test)]
mod actor_runtime_status_tests;
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
pub(crate) mod cli_management;
mod codex_mcp;
pub(crate) mod codex_voice_analyst;
pub(crate) mod codex_voice_controller;
pub(crate) mod codex_voice_lifecycle;
mod context;
mod context_projection;
mod deepseek_runtime;
mod diagnostics;
mod group_bridge;
pub(crate) use group_bridge::{schedule_due_retries, schedule_pending_route_retry};
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
mod membership;
pub(crate) use membership::ReachRestore;
pub(crate) use membership::validated_live_web_binding;
mod membership_account;
mod membership_cloudflared;
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
pub(crate) mod runtime_delivery;
mod runtime_mcp;
pub(crate) mod runtime_restore;
mod runtime_session;
mod runtime_state;
mod settings;
mod task_list;
mod terminal;
mod terminal_history_source;
mod terminal_text;
mod voice_notifications;
mod working_state;
#[cfg(test)]
mod working_state_tests;

use cccc_contracts::DaemonRequest;

pub(crate) mod operation;
use operation::Operation;

pub(crate) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    for resolver in [
        group_creation::resolve_operation,
        groups::resolve_operation,
        hermes_runtime::resolve_operation,
        cli_management::resolve_operation,
        group_copy::resolve_operation,
        group_bridge::resolve_operation,
        group_scopes::resolve_operation,
        group_space::resolve_operation,
        actors::resolve_operation,
        automation_config::resolve_operation,
        assistants::resolve_operation,
        capabilities::resolve_operation,
        messaging::resolve_operation,
        presentation::resolve_operation,
        profiles::resolve_operation,
        diagnostics::resolve_operation,
        remote_access::resolve_operation,
        membership::resolve_operation,
        runtime_state::resolve_operation,
        maintenance::resolve_operation,
        im::resolve_operation,
        memory::resolve_operation,
        context::resolve_operation,
        settings::resolve_operation,
        voice_notifications::resolve_operation,
        terminal::resolve_operation,
    ] {
        if let Some(operation) = resolver(request) {
            return Some(operation);
        }
    }
    None
}
