//! The shared browser desktop is a global login/maintenance surface. Actor
//! viewers only expose their fixed Page target, never this shared desktop.
use crate::{
    AppState,
    api::{ApiError, ApiResult, success},
};
use axum::{
    Json, Router,
    extract::{Query, State, ws::WebSocketUpgrade},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};
const KEY: &str = "web-model-login";
#[derive(Default, serde::Deserialize)]
struct QueryArgs {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    viewer_mode: String,
    #[serde(default)]
    inspect: bool,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/web-model/shared-browser", get(info))
        .route("/api/v1/web-model/shared-browser/open", post(open))
        .route("/api/v1/web-model/shared-browser/close", post(close))
        .route("/api/v1/web-model/shared-browser/ws", get(upgrade))
}
pub(super) fn profile(state: &AppState) -> std::path::PathBuf {
    state
        .home
        .root()
        .join("state/web_model_browser/_shared/chatgpt_web/chrome_profile")
}
async fn payload(state: &AppState, inspect: bool) -> ApiResult {
    let mut surface = state.browser_surfaces.info(KEY).await;
    let readiness = if inspect && surface["active"] == true {
        state
            .browser_surfaces
            .prompt_readiness(KEY)
            .await
            .map_err(|e| ApiError::bad(e.to_string()))?
    } else {
        surface["metadata"]["prompt_readiness"].clone()
    };
    if inspect {
        surface = state.browser_surfaces.info(KEY).await;
    }
    Ok(success(
        json!({"browser_surface":surface,"browser_session":{"active":surface["active"],"ready":readiness["ready"],"login_required":readiness["login_required"],"verification_required":readiness["verification_required"],"tab_url":surface["url"]}}),
    ))
}
async fn info(State(state): State<AppState>, Query(query): Query<QueryArgs>) -> ApiResult {
    payload(&state, query.inspect).await
}
async fn open(State(state): State<AppState>, Json(_body): Json<Value>) -> ApiResult {
    state
        .browser_surfaces
        .ensure_open_shared_system(KEY, &profile(&state), "https://chatgpt.com/", 1366, 900)
        .await
        .map_err(|e| ApiError::bad(format!("{e:#}")))?;
    payload(&state, false).await
}
async fn close(State(state): State<AppState>) -> ApiResult {
    let connectors = super::web_model_connector_store::load(&state)?;
    let store = cccc_core::GroupStore::new(state.home.clone())
        .map_err(super::web_model_connector_store::io_error)?;
    for meta in store
        .list()
        .map_err(super::web_model_connector_store::io_error)?
    {
        let group = store
            .load(&meta.group_id)
            .map_err(super::web_model_connector_store::io_error)?;
        if group.actors.iter().any(|a| {
            a.runtime == cccc_contracts::ActorRuntime::WebModel
                && a.enabled
                && connectors.iter().any(|c| {
                    cccc_core::web_model_connectors::binding_for_actor(
                        c,
                        &group.group_id,
                        &a.id,
                        &cccc_core::actors::generation_identity(a),
                    )
                    .is_some()
                })
        }) {
            return Err(ApiError::bad(
                "Stop all paired Web Model Actors before closing their shared browser",
            ));
        }
    }
    state
        .browser_surfaces
        .close_profile(&profile(&state))
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    payload(&state, false).await
}
async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<QueryArgs>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Shared login is unavailable in read-only mode",
        ));
    }
    Ok(ws.on_upgrade(move |socket| async move {
        if query.mode == "vnc" {
            crate::browser_surface::serve_vnc_socket(
                socket,
                &state.browser_surfaces,
                KEY,
                state.shutdown.subscribe(),
            )
            .await;
        } else {
            crate::browser_surface::serve_socket(
                socket,
                &state.browser_surfaces,
                KEY,
                &query.viewer_mode,
                state.shutdown.subscribe(),
            )
            .await;
        }
    }))
}
