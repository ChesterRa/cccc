use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

#[derive(Debug, Default, Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    force_detach: bool,
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

async fn profile_list(State(state): State<AppState>) -> ApiResult {
    call(&state, "actor_profile_list", Map::new()).await
}

async fn profile_get(State(state): State<AppState>, Path(profile_id): Path<String>) -> ApiResult {
    call(
        &state,
        "actor_profile_get",
        object(json!({"profile_id":profile_id})),
    )
    .await
}

async fn profile_upsert(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    call(&state, "actor_profile_upsert", body_object(body)?).await
}

async fn profile_put(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("profile_id".into(), Value::String(profile_id));
    call(&state, "actor_profile_upsert", args).await
}

async fn profile_delete(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> ApiResult {
    call(
        &state,
        "actor_profile_delete",
        object(json!({"profile_id":profile_id,"force_detach":query.force_detach})),
    )
    .await
}

async fn profile_secret_keys(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
) -> ApiResult {
    call(
        &state,
        "actor_profile_env_private_keys",
        object(json!({"profile_id":profile_id})),
    )
    .await
}

async fn profile_secret_update(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("profile_id".into(), Value::String(profile_id));
    call(&state, "actor_profile_env_private_update", args).await
}

async fn copy_actor_secrets(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("profile_id".into(), Value::String(profile_id));
    call(&state, "actor_profile_copy_actor_secrets", args).await
}

async fn copy_profile_secrets(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("profile_id".into(), Value::String(profile_id));
    call(&state, "actor_profile_copy_profile_secrets", args).await
}
