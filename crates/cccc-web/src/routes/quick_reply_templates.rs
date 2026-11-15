use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState,
    api::{self, ApiResult, object},
    auth::Principal,
};

#[derive(Deserialize)]
struct TemplatesUpdate {
    if_revision: Value,
    templates: Value,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/quick-reply-templates", get(read).put(save))
}

// Each signed-in identity reaches only its own list: the daemon keys it by this user ID.
async fn read(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    api::call(
        &state,
        "quick_reply_templates_get",
        object(json!({"by":"user","caller_id":principal.user_id})),
    )
    .await
}

async fn save(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(input): Json<TemplatesUpdate>,
) -> ApiResult {
    api::call(
        &state,
        "quick_reply_templates_set",
        object(json!({
            "by":"user",
            "caller_id":principal.user_id,
            "if_revision":input.if_revision,
            "templates":input.templates,
        })),
    )
    .await
}
