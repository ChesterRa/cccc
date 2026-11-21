//! Administrator-only device pairing, separate from Group invitation links.
use crate::{
    AppState,
    api::{ApiError, ApiResult},
    auth::Principal,
    session_node,
};
use axum::{
    Json, Router,
    extract::{Extension, Path, State},
    routing::{get, post},
};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/session_nodes", get(status))
        .route("/api/v1/session_nodes/invitations", post(invite))
        .route("/api/v1/session_nodes/{node_id}/revoke", post(revoke))
        .merge(super::session_node_join::routes())
}

pub(super) fn authorize(state: &AppState, principal: &Principal) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() || !principal.current_admin(&state.home).unwrap_or(false) {
        return Err(ApiError::forbidden(
            "Session-node pairing requires this instance's administrator",
        ));
    }
    Ok(())
}

async fn status(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let mut endpoint = cccc_core::session_nodes::endpoint(&state.home).map_err(unavailable)?;
    if let Some(endpoint) = &mut endpoint
        && cccc_core::session_nodes::relay_address(&state.home)
            .map_err(unavailable)?
            .is_some()
    {
        endpoint.relay_url =
            cccc_core::session_nodes::relay_public_url(&state.home).map_err(unavailable)?;
    }
    let devices = cccc_core::session_nodes::peers(&state.home).map_err(unavailable)?;
    let join_supported = session_node::verify_join_daemon(&state.home).await.is_ok()
        && (endpoint.is_none()
            || cccc_core::session_nodes::bound_invites_supported(&state.home)
                .map_err(unavailable)?);
    authorize(&state, &principal)?;

    let join_url = cccc_core::session_nodes::relay_public_url(&state.home)
        .map_err(unavailable)?
        .filter(|url| {
            url::Url::parse(url).is_ok_and(|url| {
                url.scheme() == "https"
                    && !cccc_core::local_network::is_loopback_url(&url)
                    && !url.host_str().is_some_and(|host| {
                        let host = host.trim_end_matches('.').to_ascii_lowercase();
                        host == "localhost" || host.ends_with(".localhost")
                    })
                    && match url.host() {
                        Some(url::Host::Ipv4(ip)) => !ip.is_unspecified(),
                        Some(url::Host::Ipv6(ip)) => {
                            !ip.is_unspecified()
                                && !ip
                                    .to_ipv4_mapped()
                                    .is_some_and(|v4| v4.is_loopback() || v4.is_unspecified())
                        }
                        _ => true,
                    }
            })
        });
    Ok(Json(
        json!({"ok":true,"result":{"supported":true,"join_supported":join_supported,"join_url":join_url,"transport":"iroh","endpoint":endpoint,"devices":devices}}),
    ))
}

async fn invite(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let ticket = session_node::create_invitation(&state.home, 0)
        .await
        .map_err(unavailable)?;
    authorize(&state, &principal)?;
    let encoded = session_node::encode_ticket(&ticket).map_err(unavailable)?;
    Ok(Json(
        json!({"ok":true,"result":{"ticket":encoded,"expires_at":ticket.expires_at}}),
    ))
}

async fn revoke(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(node_id): Path<String>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let mut join = state.session_node_join.lock().await;
    authorize(&state, &principal)?;
    session_node::revoke(&state.home, &node_id)
        .await
        .map_err(unavailable)?;
    join.revoke(&node_id);
    Ok(Json(json!({"ok":true,"result":{"revoked":true}})))
}

pub(super) fn unavailable(error: impl std::fmt::Display) -> ApiError {
    ApiError::unavailable("session_node_unavailable", error.to_string())
}
