use crate::{
    AppState,
    api::{ApiResult, call, object},
};
use crate::{
    api::{ApiError, success},
    connect_frames,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Extension, FromRequestParts, Path, Query, State},
    http::{HeaderMap, request::Parts},
    routing::{get, post},
};
use base64::Engine;
use cccc_contracts::connect::ConnectFrameProof;
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/connect", get(status))
        .route("/api/v1/groups/{group_id}/connect/catalog", get(catalog))
        .route("/api/v1/connect/name", post(rename))
        .route(
            "/api/v1/connect/direct",
            get(direct_status).post(direct_action),
        )
        .route("/api/v1/connect/identity", get(identity))
        .route("/api/v1/connect/group-check", post(group_check))
        .route(
            "/api/v1/connect/groups",
            get(group_status).post(group_select),
        )
        .route("/api/v1/connect/open", post(open))
        .route("/api/v1/connect/frame", get(frame).post(renew))
        .layer(DefaultBodyLimit::max(4096))
        .route(
            "/api/v1/connect/peer",
            post(peer).layer(DefaultBodyLimit::max(14 * 1024 * 1024)),
        )
}

#[derive(serde::Deserialize)]
struct GroupQuery {
    group_id: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct CatalogQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
}

// The existing Connect admin gate applies. This exposes communication metadata,
// not target Web access; the daemon checks each current Group-pair binding.
async fn catalog(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<CatalogQuery>,
) -> ApiResult {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Connect discovery is unavailable in exhibit mode",
        ));
    }
    let mut args = object(json!(query));
    args.insert("group_id".into(), json!(group_id));
    args.insert("by".into(), json!("user"));
    call(&state, "connect_catalog", args).await
}

async fn direct_status(
    State(state): State<AppState>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Direct connections require administrator access",
        ));
    }
    call(
        &state,
        "connect_direct_status",
        object(json!({"group_id":query.group_id,"by":"user"})),
    )
    .await
}
async fn direct_action(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Direct connections require administrator access",
        ));
    }
    let action = body["action"].as_str().unwrap_or("").to_owned();
    if !matches!(
        action.as_str(),
        "configure" | "invite" | "join" | "approve" | "revoke" | "remove"
    ) {
        return Err(ApiError::forbidden("Unknown direct action"));
    }
    let mut args = object(body);
    args.insert("by".into(), json!("user"));
    call(&state, &format!("connect_direct_{action}"), args).await
}

async fn group_status(State(state): State<AppState>, Query(query): Query<GroupQuery>) -> ApiResult {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Group connection management requires administrator access",
        ));
    }
    call(
        &state,
        "connect_group_status",
        object(json!({"group_id":query.group_id,"by":"user"})),
    )
    .await
}

async fn group_select(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Group connection management requires administrator access",
        ));
    }
    call(&state,"connect_group_select",object(json!({"group_id":body.get("group_id"),"invitation":body.get("invitation"),"by":"user"}))).await
}

async fn group_check(
    State(state): State<AppState>,
    Json(body): Json<cccc_contracts::connect_groups::ConnectGroupCheck>,
) -> Result<Json<cccc_contracts::connect_groups::ConnectGroupCheckResult>, ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "an exhibit cannot approve Group connections",
        ));
    }
    cccc_core::connect_groups::check(&state.home, &body)
        .map(Json)
        .map_err(|e| match e {
            cccc_core::connect_groups::SelectionError::Invalid => {
                ApiError::forbidden_code("connect_group_denied", e.to_string())
            }
            cccc_core::connect_groups::SelectionError::Unavailable(_) => {
                ApiError::unavailable("connect_group_unavailable", e.to_string())
            }
        })
}

async fn rename(State(state): State<AppState>, Json(body): Json<serde_json::Value>) -> ApiResult {
    call(
        &state,
        "connect_rename",
        object(json!({"by":"user", "display_name":body.get("display_name")})),
    )
    .await
}

async fn peer(
    State(state): State<AppState>,
    PeerAuthorization(proof): PeerAuthorization,
    Json(operation): Json<cccc_contracts::connect::ConnectPeerOperation>,
) -> ApiResult {
    let envelope = cccc_contracts::connect::ConnectPeerRequest { proof, operation };
    call(
        &state,
        "connect_peer_receive",
        object(json!({"group_id":envelope.operation.target_group_id(),"envelope":envelope})),
    )
    .await
}

// Parts extractors run before Json reads the body. Reject anonymous uploads before
// allocating their attachment payload; the daemon rechecks the proof and body hash.
struct PeerAuthorization(cccc_contracts::connect::ConnectPeerAuthorization);

impl FromRequestParts<AppState> for PeerAuthorization {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if state.web_mode.is_read_only() {
            return Err(ApiError::forbidden(
                "an exhibit does not accept peer collaboration",
            ));
        }
        let encoded = parts
            .headers
            .get(cccc_contracts::connect::CONNECT_PROOF_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 4096)
            .ok_or_else(|| ApiError::forbidden("a current Connect device proof is required"))?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| ApiError::forbidden("invalid Connect proof encoding"))?;
        let proof: cccc_contracts::connect::ConnectPeerAuthorization =
            serde_json::from_slice(&bytes)
                .map_err(|_| ApiError::forbidden("invalid Connect proof"))?;
        if proof
            .connection_id
            .as_deref()
            .is_some_and(cccc_contracts::direct::is_direct)
        {
            return Err(ApiError::forbidden_code(
                "connect_peer_denied",
                "Direct authority is not accepted by the account HTTP port",
            ));
        }
        cccc_core::connect_peer::authenticate_authorization(&state.home, &proof)
            .map_err(|message| ApiError::forbidden_code("connect_peer_denied", message))?;
        Ok(Self(proof))
    }
}

async fn status(State(state): State<AppState>) -> ApiResult {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Connect workbench requires a normal administrator view",
        ));
    }
    call(&state, "connect_status", object(json!({"by": "user"}))).await
}

#[derive(serde::Deserialize)]
struct IdentityQuery {
    nonce: String,
}

async fn identity(
    State(state): State<AppState>,
    Query(query): Query<IdentityQuery>,
) -> Result<Json<cccc_contracts::connect::ConnectIdentityProof>, ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::unavailable(
            "connect_unavailable",
            "Connect requires a normal Web listener; this listener is an exhibit",
        ));
    }
    connect_frames::identity_proof(&state.home, &query.nonce)
        .map(Json)
        .map_err(|message| ApiError::unavailable("connect_unavailable", message))
}

#[derive(serde::Deserialize)]
struct OpenRequest {
    instance_id: String,
    frame_id: String,
}

async fn open(
    State(state): State<AppState>,
    Extension(principal): Extension<crate::auth::Principal>,
    headers: HeaderMap,
    Json(body): Json<OpenRequest>,
) -> ApiResult {
    let origin = crate::request_origin::served_origin(&state, &headers)
        .ok_or_else(|| ApiError::bad("entry origin is unavailable"))?;
    let (target_origin, proof) =
        connect_frames::issue(&state.home, &body.instance_id, &origin, &body.frame_id)
            .map_err(|message| ApiError::unavailable("connect_unavailable", message))?;
    let client = state
        .connect_http
        .as_ref()
        .map_err(|message| ApiError::unavailable("connect_http_unavailable", message.clone()))?;
    connect_frames::confirm_target(&state.home, client, &body.instance_id, &target_origin)
        .await
        .map_err(|message| ApiError::unavailable("connect_target_unavailable", message))?;
    if !principal
        .current_admin(&state.home)
        .map_err(|error| ApiError::unavailable("access_token_store_error", error.to_string()))?
    {
        return Err(ApiError::forbidden(
            "entry administrator access was revoked",
        ));
    }
    let (current_origin, current_proof) =
        connect_frames::issue(&state.home, &body.instance_id, &origin, &body.frame_id)
            .map_err(|message| ApiError::unavailable("connect_unavailable", message))?;
    if current_origin != target_origin
        || current_proof.target_device_id != proof.target_device_id
        || current_proof.source_device_id != proof.source_device_id
    {
        return Err(ApiError::forbidden(
            "Connect binding changed during target confirmation",
        ));
    }
    let proof = current_proof;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&proof).map_err(|error| ApiError::bad(error.to_string()))?);
    Ok(success(
        json!({"origin":target_origin, "url":format!("{target_origin}/ui/connect/?proof={encoded}"), "proof":proof}),
    ))
}

#[derive(serde::Deserialize)]
struct FrameQuery {
    frame_id: String,
}

async fn frame(State(state): State<AppState>, Query(query): Query<FrameQuery>) -> ApiResult {
    let proof = state
        .connect_frames
        .current(&state.home, &query.frame_id)
        .map_err(|message| ApiError::forbidden_code("connect_frame_expired", message))?;
    Ok(success(json!({"frame": proof})))
}

async fn renew(State(state): State<AppState>, Json(proof): Json<ConnectFrameProof>) -> ApiResult {
    let frame_id = proof.frame_id.clone();
    state
        .connect_frames
        .accept(&state.home, proof, false)
        .map_err(|message| ApiError::forbidden_code("connect_frame_invalid", message))?;
    Ok(success(json!({"frame_id":frame_id})))
}
