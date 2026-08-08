mod access_token_support;
mod access_tokens;
mod actor_assets;
mod actor_profiles;
mod actors;
mod assistants;
mod capabilities;
mod context;
mod diagnostics;
mod file_response;
mod filesystem;
mod group_bridge;
mod group_bridge_close;
mod group_bridge_command_sessions;
mod group_bridge_pairing;
mod group_bridge_pairing_endpoint;
mod group_bridge_seen;
mod group_bridge_session;
mod group_bridge_store;
mod group_copy;
mod group_prompt_notify;
mod group_prompts;
mod group_space;
mod group_space_provider;
mod groups;
mod headless;
mod headless_store;
mod im;
mod im_authorization;
mod messaging;
mod messaging_cross_group;
mod nomcp;
mod nomcp_admin;
mod nomcp_pages;
mod nomcp_render;
mod nomcp_resources;
mod nomcp_send;
mod presentation;
mod presentation_browser;
mod remote_access;
mod remote_access_projection;
mod runtime_activity;
mod settings;
mod streams;
mod system;
mod system_branding;
mod system_branding_assets;
mod system_scope;
mod terminal;
mod terminal_ws;
mod terminal_ws_protocol;
mod terminal_ws_replay;
mod web_model_browser;
mod web_model_connector_activity;
mod web_model_connector_provisioning;
mod web_model_connector_store;
mod web_model_connectors;
mod web_model_delivery;
mod web_model_delivery_completion;
mod web_model_delivery_state;
#[cfg(test)]
mod web_model_delivery_test_support;
#[cfg(test)]
mod web_model_delivery_tests;
mod web_model_supervisor;

use crate::AppState;
use axum::Router;
use serde_json::Value;

fn first_non_blank<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(system::routes())
        .merge(system_branding::routes())
        .merge(system_branding_assets::routes())
        .merge(system_scope::routes())
        .merge(filesystem::routes())
        .merge(access_tokens::routes())
        .merge(groups::routes())
        .merge(group_copy::routes())
        .merge(group_bridge::routes())
        .merge(group_bridge_pairing::routes())
        .merge(group_bridge_session::routes())
        .merge(actors::routes())
        .merge(assistants::routes())
        .merge(group_space::routes())
        .merge(group_space_provider::routes())
        .merge(headless::routes())
        .merge(im::routes())
        .merge(messaging::routes())
        .merge(messaging_cross_group::routes())
        .merge(presentation::routes())
        .merge(presentation_browser::routes())
        .merge(web_model_connectors::routes())
        .merge(web_model_browser::routes())
        .merge(nomcp::routes())
        .merge(context::routes())
        .merge(diagnostics::routes())
        .merge(remote_access::routes())
        .merge(runtime_activity::routes())
        .merge(settings::routes())
        .merge(capabilities::routes())
        .merge(streams::routes())
        .merge(terminal::routes())
}

pub(crate) use web_model_supervisor::spawn as spawn_web_model_supervisor;

#[cfg(test)]
mod tests {
    use super::first_non_blank;
    use serde_json::json;

    #[test]
    fn request_aliases_skip_empty_primary_values() {
        let value = json!({"primary":" ","legacy":" value "});
        assert_eq!(
            first_non_blank(&value, &["primary", "legacy"]),
            Some("value")
        );
    }
}
