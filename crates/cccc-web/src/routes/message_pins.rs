use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use cccc_contracts::message_pins::MessagePinUpdate;
use serde::Deserialize;
use serde_json::json;

use crate::{
    AppState,
    api::{self, ApiResult, object},
};

#[derive(Default, Deserialize)]
struct PinsQuery {
    detail: Option<String>,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/groups/{group_id}/pins", get(list).post(set))
}

async fn list(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<PinsQuery>,
) -> ApiResult {
    api::call(
        &state,
        "message_pins_list",
        object(json!({"group_id":group_id,"by":"user","detail":query.detail})),
    )
    .await
}

async fn set(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<PinsQuery>,
    Json(input): Json<MessagePinUpdate>,
) -> ApiResult {
    api::call(
        &state,
        "message_pins_set",
        object(json!({
            "group_id":group_id,
            "by":"user",
            "detail":query.detail,
            "event_id":input.event_id,
            "pinned":input.pinned,
        })),
    )
    .await
}
