use super::*;

pub(super) async fn archive(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_document_archive", args).await
}

pub(super) async fn delete(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.insert("by".into(), json!("user"));
    call(&state, "assistant_voice_document_delete", args).await
}
