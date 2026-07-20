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
mod group_bridge;
mod group_bridge_command_sessions;
mod group_bridge_pairing;
mod group_bridge_session;
mod group_bridge_store;
mod group_copy;
mod group_prompts;
mod group_space;
mod group_space_provider;
mod groups;
mod headless;
mod im;
mod messaging;
mod nomcp;
mod nomcp_admin;
mod nomcp_pages;
mod nomcp_render;
mod nomcp_resources;
mod nomcp_send;
mod presentation;
mod presentation_browser;
mod remote_access;
mod settings;
mod streams;
mod system;
mod terminal;
mod terminal_ws;
mod web_model_browser;
mod web_model_connectors;

use crate::AppState;
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(system::routes())
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
        .merge(presentation::routes())
        .merge(presentation_browser::routes())
        .merge(web_model_connectors::routes())
        .merge(web_model_browser::routes())
        .merge(nomcp::routes())
        .merge(context::routes())
        .merge(diagnostics::routes())
        .merge(remote_access::routes())
        .merge(settings::routes())
        .merge(capabilities::routes())
        .merge(streams::routes())
        .merge(terminal::routes())
}
