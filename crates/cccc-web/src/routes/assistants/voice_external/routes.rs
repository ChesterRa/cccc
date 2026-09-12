use super::{Provider, config, connection};
use crate::{
    AppState,
    api::{ApiResult, success},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde_json::{Value, json};

pub(in crate::routes::assistants) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/voice/asr/providers", get(list))
        .route(
            "/api/v1/voice/asr/providers/{provider}",
            axum::routing::put(save),
        )
        .route(
            "/api/v1/voice/asr/providers/{provider}/probe",
            axum::routing::post(probe),
        )
}

async fn list(State(state): State<AppState>) -> ApiResult {
    let mut providers = Vec::new();
    for provider in [Provider::Bailian, Provider::Volcengine] {
        providers.push(
            config::load(&state.home, provider)
                .map_err(super::super::voice_error)?
                .public(provider),
        );
    }
    Ok(success(json!({"providers":providers})))
}

async fn save(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(patch): Json<config::Update>,
) -> ApiResult {
    let provider = Provider::parse(&provider).map_err(super::super::voice_error)?;
    let value = config::save(&state.home, provider, patch).map_err(|error| {
        if error.code == "external_asr_invalid_config" {
            crate::api::ApiError::bad_code(error.code, error.message, json!({}))
        } else {
            super::super::voice_error(error)
        }
    })?;
    Ok(success(value))
}

async fn probe(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(_): Json<Value>,
) -> ApiResult {
    let provider = Provider::parse(&provider).map_err(super::super::voice_error)?;
    let config = config::load(&state.home, provider).map_err(super::super::voice_error)?;
    let mut opened = connection::connect(provider, config, "auto")
        .await
        .map_err(super::super::voice_error)?;
    // Probe authentication/task admission only: no microphone or audio is sent.
    let _ =
        tokio::time::timeout(std::time::Duration::from_secs(2), opened.socket.close(None)).await;
    Ok(success(
        json!({"provider":provider.id(),"model_id":opened.model,"connected":true}),
    ))
}
