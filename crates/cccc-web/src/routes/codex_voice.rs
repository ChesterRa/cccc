use axum::Router;
use axum::routing::{delete, get, post};
use serde::Deserialize;

use crate::AppState;

mod handlers;
mod payload;
mod terminal;
#[cfg(test)]
mod tests;
mod voice_socket;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/codex_voice/calls/active", get(handlers::active))
        .route(
            "/api/v1/groups/{group_id}/codex_voice/calls",
            post(handlers::start),
        )
        .route(
            "/api/v1/groups/{group_id}/codex_voice/calls/{generation}",
            delete(handlers::stop),
        )
        .route(
            "/api/v1/groups/{group_id}/codex_voice/calls/{generation}/events",
            get(handlers::upgrade),
        )
        .route(
            "/api/v1/groups/{group_id}/codex_voice/analysts/{generation}/terminal",
            get(handlers::upgrade_terminal),
        )
        .route(
            "/api/v1/groups/{group_id}/codex_voice/analysts/{generation}/reset",
            post(handlers::reset_analyst),
        )
        .route(
            "/api/v1/groups/{group_id}/codex_voice/analysts/{generation}/cancel",
            post(handlers::cancel_analyst),
        )
}

#[derive(Debug, Deserialize)]
struct TerminalQuery {
    #[serde(default = "terminal_control")]
    mode: String,
    since: Option<u64>,
    #[serde(default)]
    takeover: bool,
    output_flow: Option<String>,
    bootstrap: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
}

fn terminal_control() -> String {
    "control".into()
}
