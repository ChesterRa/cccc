//! Attended update: an administrator confirms a version and a mode, the
//! supervising host prepares that update, and this Web closes so the host can
//! finish it. The Web never replaces files itself.

use axum::extract::State;
use axum::routing::post;
use axum::{Extension, Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult};
use crate::codex_voice::CodexVoiceSessions;
use crate::codex_voice::restart::VoiceGateError;
use crate::update_hook::{UpdateHook, UpdateMode, UpdateRejection, UpdateRequest};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/update/start", post(start))
}

async fn start(
    State(state): State<AppState>,
    Extension(principal): Extension<crate::auth::Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let version = body
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("version is required"))?
        .to_owned();
    let mode = body
        .get("mode")
        .and_then(Value::as_str)
        .and_then(UpdateMode::parse)
        .ok_or_else(|| ApiError::bad("mode must be keep or restart"))?;
    let unavailable = || {
        ApiError::conflict(
            "update_unavailable",
            "this CCCC process cannot update itself; update it with `cccc update`",
            json!({}),
        )
    };
    let restart = state.restart.as_ref().ok_or_else(unavailable)?;
    let hook = restart.update_hook().ok_or_else(unavailable)?;
    let preparation = hook
        .begin(UpdateRequest { version, mode })
        .map_err(rejected)?;
    // From here on, every way out other than the handoff gives the update up,
    // including the client going away while the host is still preparing.
    let reserved = Reserved(Some((&hook, &preparation.transaction)));
    preparation.ready.await.map_err(rejected)?;
    require_current_admin(&state, &principal)?;
    // The Web closes for the update, so Voice stops, as it does for a binding
    // change. It does so only now, so a rejected update leaves Voice running.
    state
        .codex_voice
        .prepare_web_restart()
        .await
        .map_err(VoiceGateError::into_api_error)?;
    let voice = ReopenVoice(Some(&state.codex_voice));
    require_current_admin(&state, &principal)?;
    restart.request_update().map_err(|error| {
        ApiError::unavailable(
            "update_handoff_failed",
            format!("failed to close the Web for the update: {error}"),
        )
    })?;
    voice.keep_closed();
    let transaction = reserved.hand_over();
    Ok(Json(json!({"ok":true,"result":{
        "accepted":true,
        "transaction":transaction,
        "mode":mode.as_str()
    }})))
}

fn rejected(rejection: UpdateRejection) -> ApiError {
    ApiError::conflict(rejection.code, rejection.message, json!({}))
}

/// A preparation can take minutes. An administrator whose access was revoked
/// meanwhile does not get the update carried out.
fn require_current_admin(
    state: &AppState,
    principal: &crate::auth::Principal,
) -> Result<(), ApiError> {
    let current = principal
        .current_admin(&state.home)
        .map_err(|error| ApiError::unavailable("access_token_store_error", error.to_string()))?;
    if current {
        Ok(())
    } else {
        Err(ApiError::forbidden("administrator access was revoked"))
    }
}

/// Gives the reserved update up unless it was handed over.
struct Reserved<'a>(Option<(&'a UpdateHook, &'a str)>);

impl Reserved<'_> {
    fn hand_over(mut self) -> String {
        self.0
            .take()
            .map(|(_, transaction)| transaction.to_owned())
            .unwrap_or_default()
    }
}

impl Drop for Reserved<'_> {
    fn drop(&mut self) {
        if let Some((hook, transaction)) = self.0 {
            hook.abandon(transaction);
        }
    }
}

/// Reopens Voice unless the update was handed over.
struct ReopenVoice<'a>(Option<&'a CodexVoiceSessions>);

impl ReopenVoice<'_> {
    fn keep_closed(mut self) {
        self.0 = None;
    }
}

impl Drop for ReopenVoice<'_> {
    fn drop(&mut self) {
        if let Some(voice) = self.0 {
            voice.cancel_prepared_restart();
        }
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
