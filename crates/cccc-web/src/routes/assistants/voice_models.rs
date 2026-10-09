use super::*;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/voice/asr/models", get(list))
        .route("/api/v1/voice/asr/models/{model_id}/install", post(install))
        .route("/api/v1/voice/asr/models/{model_id}/remove", post(remove))
}
async fn list(State(state): State<AppState>) -> ApiResult {
    let models = voice_asr::list_models(&state.home).map_err(voice_error)?;
    Ok(success(
        json!({"group_id":"","service_models":models,"service_runtime":voice_asr::runtime_status()}),
    ))
}
async fn install(State(state): State<AppState>, Path(model_id): Path<String>) -> ApiResult {
    Ok(success(
        json!({"model":voice_asr::begin_install(state.home,model_id).map_err(voice_error)?}),
    ))
}
async fn remove(State(state): State<AppState>, Path(model_id): Path<String>) -> ApiResult {
    Ok(success(
        json!({"model":voice_asr::remove_model(&state.home,&model_id).map_err(voice_error)?}),
    ))
}
