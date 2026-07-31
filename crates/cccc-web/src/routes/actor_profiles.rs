use axum::extract::{Extension, Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};
use crate::auth::Principal;

#[derive(Debug, Default, Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    force_detach: bool,
    scope: Option<String>,
    owner_id: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/actor_profiles",
            get(profile_list).post(profile_upsert),
        )
        .route("/api/v1/profiles", get(profile_list).post(profile_upsert))
        .route(
            "/api/v1/actor_profiles/{profile_id}",
            get(profile_get).delete(profile_delete),
        )
        .route(
            "/api/v1/profiles/{profile_id}",
            get(profile_get).put(profile_put).delete(profile_delete),
        )
        .route(
            "/api/v1/actor_profiles/{profile_id}/env_private",
            get(profile_secret_keys).post(profile_secret_update),
        )
        .route(
            "/api/v1/profiles/{profile_id}/env_private",
            get(profile_secret_keys).post(profile_secret_update),
        )
        .route(
            "/api/v1/actor_profiles/{profile_id}/copy_actor_secrets",
            axum::routing::post(copy_actor_secrets),
        )
        .route(
            "/api/v1/profiles/{profile_id}/copy_profile_secrets",
            axum::routing::post(copy_profile_secrets),
        )
}

#[derive(Debug, Default, Deserialize)]
struct ProfileQuery {
    view: Option<String>,
    scope: Option<String>,
    owner_id: Option<String>,
}

async fn profile_list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ProfileQuery>,
) -> ApiResult {
    call(
        &state,
        "actor_profile_list",
        auth_args(
            &principal,
            object(json!({"view":query.view.unwrap_or_else(|| "global".into())})),
        ),
    )
    .await
}

async fn profile_get(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(profile_id): Path<String>,
    Query(query): Query<ProfileQuery>,
) -> ApiResult {
    call(
        &state,
        "actor_profile_get",
        auth_args(
            &principal,
            object(json!({
                "profile_id":profile_id,
                "profile_scope":query.scope,
                "profile_owner":query.owner_id,
            })),
        ),
    )
    .await
}

async fn profile_upsert(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    call(
        &state,
        "actor_profile_upsert",
        auth_args(&principal, body_object(body)?),
    )
    .await
}

async fn profile_put(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(profile_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("profile_id".into(), Value::String(profile_id));
    call(&state, "actor_profile_upsert", auth_args(&principal, args)).await
}

async fn profile_delete(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(profile_id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> ApiResult {
    call(
        &state,
        "actor_profile_delete",
        auth_args(
            &principal,
            object(json!({
                "profile_id":profile_id,
                "force_detach":query.force_detach,
                "profile_scope":query.scope,
                "profile_owner":query.owner_id,
            })),
        ),
    )
    .await
}

fn auth_args(principal: &Principal, mut args: Map<String, Value>) -> Map<String, Value> {
    args.insert("caller_id".into(), Value::String(principal.user_id.clone()));
    args.insert("is_admin".into(), Value::Bool(principal.is_admin));
    args.insert("allowed_groups".into(), json!(principal.allowed_groups));
    args
}

async fn profile_secret_keys(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(profile_id): Path<String>,
    Query(query): Query<ProfileQuery>,
) -> ApiResult {
    call(
        &state,
        "actor_profile_env_private_keys",
        auth_args(
            &principal,
            object(json!({
                "profile_id":profile_id,
                "profile_scope":query.scope,
                "profile_owner":query.owner_id,
            })),
        ),
    )
    .await
}

async fn profile_secret_update(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(profile_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("profile_id".into(), Value::String(profile_id));
    call(
        &state,
        "actor_profile_env_private_update",
        auth_args(&principal, args),
    )
    .await
}

async fn copy_actor_secrets(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(profile_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("profile_id".into(), Value::String(profile_id));
    call(
        &state,
        "actor_profile_copy_actor_secrets",
        auth_args(&principal, args),
    )
    .await
}

async fn copy_profile_secrets(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(profile_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("profile_id".into(), Value::String(profile_id));
    call(
        &state,
        "actor_profile_copy_profile_secrets",
        auth_args(&principal, args),
    )
    .await
}
