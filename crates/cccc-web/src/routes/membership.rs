use axum::Router;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use serde_json::json;
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiResult, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/membership", get(state))
        .route("/api/v1/membership/login", post(login))
        .route("/api/v1/membership/login/poll", post(login_poll))
        .route("/api/v1/membership/logout", post(logout))
        .route("/api/v1/membership/reach/on", post(reach_on))
        .route("/api/v1/membership/reach/off", post(reach_off))
}

async fn state(State(state): State<AppState>) -> ApiResult {
    call(&state, "membership_status", object(json!({"by": "user"}))).await
}

async fn login(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_login",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn login_poll(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_login_poll",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn logout(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_logout",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn reach_on(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_reach_on",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn reach_off(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_reach_off",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}
