//! Administrator-only device pairing, separate from Group invitation links. The gateway lives in
//! the daemon; these routes only authorize the administrator and forward the operation.
use crate::{
    AppState,
    api::{ApiError, ApiResult, body_object, object},
    auth::Principal,
};
use axum::{
    Json, Router,
    extract::{Extension, Path, Request, State},
    http::{HeaderValue, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, put},
};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/session_nodes", get(status))
        .route("/api/v1/session_nodes/invitations", post(invite))
        .route("/api/v1/session_nodes/{node_id}/revoke", post(revoke))
        .route("/api/v1/session_nodes/stop", post(stop))
        .route("/api/v1/session_nodes/relay", put(relay))
        .route("/api/v1/session_nodes/sources", get(sources))
        .layer(middleware::from_fn(no_store))
}

pub(super) fn authorize(state: &AppState, principal: &Principal) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() || !principal.current_admin(&state.home).unwrap_or(false) {
        return Err(ApiError::forbidden(
            "Session-node pairing requires this instance's administrator",
        ));
    }
    Ok(())
}

/// Forward one management operation. A daemon from before device-session management answers
/// `unknown_op`, and one that is stopping refuses new work; both are temporary for this page.
async fn call(state: &AppState, op: &str, args: serde_json::Map<String, Value>) -> ApiResult {
    crate::api::call(state, op, args)
        .await
        .map_err(|error| match error.code() {
            "unknown_op" => ApiError::unavailable(
                "session_node_upgrade_required",
                "The running CCCC daemon is older than this page; restart CCCC to manage devices",
            ),
            "shutdown_in_progress" => {
                ApiError::unavailable("shutdown_in_progress", error.message())
            }
            _ => error,
        })
}

/// Pairing tickets and device directories are private; never let a cache keep them.
async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn status(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let Json(mut response) =
        call(&state, "session_node_status", object(json!({"by":"user"}))).await?;
    if let Some(result) = response.get_mut("result").and_then(Value::as_object_mut) {
        result.insert("supported".into(), Value::Bool(true));
    }
    Ok(Json(response))
}

async fn invite(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let response = call(&state, "session_node_invite", object(json!({"by":"user"}))).await?;
    // The ticket is private: release it only if the caller is still the administrator.
    authorize(&state, &principal)?;
    Ok(response)
}

async fn revoke(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(node_id): Path<String>,
) -> ApiResult {
    authorize(&state, &principal)?;
    call(
        &state,
        "session_node_revoke",
        object(json!({"by":"user","node_id":node_id})),
    )
    .await
}

async fn stop(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    call(&state, "session_node_stop", object(json!({"by":"user"}))).await
}

async fn relay(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let body = body_object(body)?;
    let enabled = body
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| ApiError::bad("enabled must be a boolean"))?;
    call(
        &state,
        "session_node_relay_set",
        object(json!({"by":"user","enabled":enabled})),
    )
    .await
}

async fn sources(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let response = call(
        &state,
        "session_node_source_list",
        object(json!({"by":"user","include_offline":true})),
    )
    .await?;
    authorize(&state, &principal)?;
    Ok(response)
}
