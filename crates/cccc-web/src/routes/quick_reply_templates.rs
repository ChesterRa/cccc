use axum::{Extension, Json, Router, extract::State, routing::get};
use serde_json::{Value, json};

use crate::{
    AppState,
    api::{self, ApiResult, body_object, object},
    auth::Principal,
};

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
    Json(body): Json<Value>,
) -> ApiResult {
    let mut body = body_object(body)?;
    let mut args = object(json!({"by":"user","caller_id":principal.user_id}));
    // Identity always comes from the Principal; the complete ordered list is passed as sent.
    for key in ["if_revision", "replies"] {
        if let Some(value) = body.remove(key) {
            args.insert(key.into(), value);
        }
    }
    api::call(&state, "quick_reply_templates_set", args).await
}
