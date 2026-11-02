use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use cccc_core::chet_history::TextRequest;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    AppState,
    api::{self, ApiError, ApiResult, object, success},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/chet/history", get(history))
        .route("/api/v1/chet/status", get(status))
        .route("/api/v1/chet/inputs", post(submit))
        .route("/api/v1/chet/inputs/{id}", get(find))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryQuery {
    before: Option<String>,
    after: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitInput {
    id: Uuid,
    request: TextRequest,
}

fn require_interactive_web(state: &AppState) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden_code(
            "read_only",
            "The global assistant is not available in read-only Web mode.",
        ));
    }
    Ok(())
}

async fn history(State(state): State<AppState>, Query(query): Query<HistoryQuery>) -> ApiResult {
    require_interactive_web(&state)?;
    api::call(
        &state,
        "chet_history_list",
        object(json!({"by":"user","before":query.before,"after":query.after,"limit":query.limit,"resolve_sources":true})),
    )
    .await
}

async fn find(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
    require_interactive_web(&state)?;
    api::call(
        &state,
        "chet_history_find",
        object(json!({"by":"user","id":id,"resolve_sources":true})),
    )
    .await
}

async fn status(State(state): State<AppState>) -> ApiResult {
    require_interactive_web(&state)?;
    Ok(success(state.codex_voice.chet_status().await))
}

async fn submit(State(state): State<AppState>, Json(input): Json<SubmitInput>) -> ApiResult {
    require_interactive_web(&state)?;
    if input.id.is_nil()
        || input.request.text.trim().is_empty()
        || input.request.text.len() > 64 * 1024
    {
        return Err(ApiError::bad("A nonempty input and ID are required"));
    }
    // Both owners must be reconciled before a new Analyst can be launched.
    super::process_recovery::ensure_voice_start_allowed(&state).await?;
    let (send, receive) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let registered = api::call(
            &state,
            "chet_history_register",
            object(json!({"by":"user","id":input.id,"request":input.request})),
        )
        .await;
        let dispatch = match &registered {
            Ok(response) => match response.0["result"]["registration"].as_str() {
                Some("inserted") => true,
                Some("existing") => false,
                _ => {
                    let _ = send.send(Err(ApiError::unavailable(
                        "chet_submission_unknown",
                        "The input registration could not be confirmed. Review history before trying again.",
                    )));
                    return;
                }
            },
            Err(_) => false,
        };
        let preparation = dispatch.then(|| state.codex_voice.track_chet_preparation(input.id));
        let _ = send.send(registered);
        if dispatch {
            match state
                .codex_voice
                .submit_chet_input(&state.home, input.id, &input.request)
                .await
            {
                Ok(()) => {}
                Err(crate::codex_voice::ChetSubmitError::NotStarted) => {
                    let saved = api::call(
                        &state,
                        "chet_history_not_started",
                        object(json!({"by":"user","id":Uuid::new_v4(),
                                      "input_id":input.id,"reason":"assistant_unavailable"})),
                    )
                    .await;
                    let confirmed = saved
                        .as_ref()
                        .ok()
                        .and_then(|response| response.0["result"]["registration"].as_str())
                        .is_some_and(|value| matches!(value, "inserted" | "existing"));
                    if !confirmed {
                        state.codex_voice.mark_chet_history_write_failed();
                    }
                }
                Err(crate::codex_voice::ChetSubmitError::Unconfirmed) => {
                    // Do not log the prompt or replay an uncertain delivery.
                    tracing::warn!("Chet input delivery was not confirmed");
                }
            }
        }
        drop(preparation);
    });
    receive.await.unwrap_or_else(|_| {
        Err(ApiError::unavailable(
            "chet_submission_unknown",
            "The input registration could not be confirmed. Review history before trying again.",
        ))
    })
}
