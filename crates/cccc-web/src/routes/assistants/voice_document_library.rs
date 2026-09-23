use super::*;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/groups/{group_id}/assistants/voice_secretary/documents/library",
        get(read).post(update),
    )
}
async fn read(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(
        &state,
        "assistant_voice_document_library",
        object(json!({"group_id":group_id})),
    )
    .await
}
async fn update(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.insert("by".into(), json!("user"));
    call(&state, "assistant_voice_document_library_update", args).await
}
