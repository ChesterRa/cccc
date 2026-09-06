use super::require_interactive_web;
use crate::{
    AppState,
    api::{self, ApiError, ApiResult, success},
};
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

pub(crate) async fn preferences(State(state): State<AppState>) -> ApiResult {
    require_interactive_web(&state)?;
    api::call(&state, "voice_preferences_get", Default::default()).await
}

pub(crate) async fn save_preferences(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ApiResult {
    require_interactive_web(&state)?;
    api::call(
        &state,
        "voice_preferences_set",
        body.as_object()
            .cloned()
            .ok_or_else(|| ApiError::bad("expected preferences object"))?,
    )
    .await
}

pub(crate) async fn viewed(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    require_interactive_web(&state)?;
    api::call(
        &state,
        "voice_messages_viewed",
        body.as_object()
            .cloned()
            .ok_or_else(|| ApiError::bad("expected viewed messages object"))?,
    )
    .await
}

pub(crate) async fn notifications(State(state): State<AppState>) -> ApiResult {
    require_interactive_web(&state)?;
    api::call(&state, "voice_notifications_get", Default::default()).await
}

pub(crate) async fn prepare_output(
    State(state): State<AppState>,
    Path(generation): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let call = state.codex_voice.current().await.call;
    if !call.is_some_and(|call| call.generation == generation && call.connected) {
        return Err(ApiError::bad("Voice call is no longer connected"));
    }
    let id = body["result_id"]
        .as_str()
        .filter(|id| id.len() <= 256)
        .ok_or_else(|| ApiError::bad("result_id is required"))?;
    prepare_reserved_output(&state.home, &generation, id)
}

fn prepare_reserved_output(home: &cccc_core::HomeLayout, generation: &str, id: &str) -> ApiResult {
    let text = cccc_core::voice_notifications::prepare_output(home, id, generation)
        .map_err(|error| ApiError::bad(error.to_string()))?;
    let Some(text) = text else {
        return Ok(success(json!({"message":null})));
    };
    Ok(success(
        json!({"message":{"type":"session.context.append","channel":"speakable","content":[{"type":"input_text","text":text}]}}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::voice_notifications::NotificationScope;
    use cccc_core::{GroupStore, HomeLayout, voice_notifications as store};

    #[test]
    fn lost_suppression_response_is_retryable_without_reviving_or_reassigning_output() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let mut group = groups.create("Notification preflight", "").expect("group");
        group.actors.push(cccc_contracts::Actor::new("worker"));
        groups.save(&group).expect("actor");
        let mut preferences = store::preferences(&home).expect("preferences");
        preferences
            .groups
            .insert(group.group_id.clone(), NotificationScope::AllChat);
        store::save_preferences(&home, preferences).expect("subscribe");
        let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
        event.by = "worker".into();
        event.data = json!({"to":["user"],"text":"Synthetic result"})
            .as_object()
            .expect("data")
            .clone();
        cccc_core::ledger::append(
            &groups.ledger_path(&group.group_id).expect("ledger"),
            &event,
        )
        .expect("message");
        store::scan(&home).expect("scan");
        let source = store::snapshot(&home).expect("source").messages[0]
            .source
            .clone();
        store::reserve(&home, &source, "analyst").expect("reserve input");
        store::processed(
            &home,
            &[source.correlation_id()],
            "analyst",
            "turn",
            "Synthetic summary",
        )
        .expect("result");
        let id = store::snapshot(&home).expect("result").results[0]
            .id
            .clone();
        store::reserve_output(&home, &id, "call").expect("reserve output");
        assert!(prepare_reserved_output(&home, "wrong-call", &id).is_err());
        assert!(
            prepare_reserved_output(&home, "call", &id)
                .expect("allowed")
                .0["result"]["message"]
                .is_object()
        );
        store::mark_viewed(&home, &[source]).expect("viewed before retry");
        assert!(
            prepare_reserved_output(&home, "call", &id)
                .expect("suppress")
                .0["result"]["message"]
                .is_null()
        );
        let mut preferences = store::preferences(&home).expect("preferences");
        preferences.suppress_viewed = false;
        store::save_preferences(&home, preferences).expect("policy changed");
        assert!(
            prepare_reserved_output(&home, "call", &id)
                .expect("retry lost response")
                .0["result"]["message"]
                .is_null()
        );
        assert!(prepare_reserved_output(&home, "wrong-call", &id).is_err());
        let result = &store::snapshot(&home).expect("retained").results[0];
        assert!(result.output_suppressed);
        assert!(!result.output_submitted);
    }
}
