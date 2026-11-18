//! Attended update: an administrator confirms a version and a mode, the
//! supervising host prepares that update, and this Web closes so the host can
//! finish it. The Web never replaces files itself.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult};
use crate::codex_voice::CodexVoiceSessions;
use crate::codex_voice::restart::VoiceGateError;
use crate::update_hook::{
    UpdateCheckHook, UpdateHook, UpdateMode, UpdateRejection, UpdateRequest, UpdateSource,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/update/status", get(status))
        .route("/api/v1/update/check", post(check))
        .route("/api/v1/update/start", post(start))
}

/// Local facts only. Reading them never uses the network, so a page can read
/// them whenever it opens, including when it reconnects after an update.
async fn status(State(state): State<AppState>) -> ApiResult {
    let (checker, in_place) = update_checker(&state)?;
    Ok(Json(json!({"ok":true,"result":{
        "status":checker.status(),
        "in_place":in_place
    }})))
}

/// Asks release discovery once, because an administrator asked.
async fn check(State(state): State<AppState>) -> ApiResult {
    let (checker, in_place) = update_checker(&state)?;
    let hook = state
        .restart
        .as_ref()
        .and_then(|restart| restart.update_hook());
    let local_restart = hook.as_ref().is_some_and(|hook| hook.local_restart());
    let release_keep = hook.as_ref().is_some_and(|hook| hook.release_keep());
    let check = checker.check().await;
    Ok(Json(json!({"ok":true,"result":{
        "check":check,
        "in_place":in_place,
        "local_restart":local_restart,
        "release_keep":release_keep
    }})))
}

/// The host's check, and whether this process can install a release itself.
fn update_checker(state: &AppState) -> Result<(UpdateCheckHook, bool), ApiError> {
    let restart = state.restart.as_ref();
    let checker = restart
        .and_then(|restart| restart.update_checker())
        .ok_or_else(|| {
            ApiError::conflict(
                "update_unavailable",
                "this CCCC process cannot check for updates; run `cccc update --check`",
                json!({}),
            )
        })?;
    let in_place = restart
        .and_then(|restart| restart.update_hook())
        .is_some_and(|hook| hook.release_in_place());
    Ok((checker, in_place))
}

async fn start(
    State(state): State<AppState>,
    Extension(principal): Extension<crate::auth::Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let source = update_source(&body)?;
    let mode = body
        .get("mode")
        .and_then(Value::as_str)
        .and_then(UpdateMode::parse)
        .ok_or_else(|| ApiError::bad("mode must be keep or restart"))?;
    let restart = state.restart.as_ref().ok_or_else(unavailable)?;
    let transaction = hand_over(
        restart,
        &state.codex_voice,
        UpdateRequest { source, mode },
        || require_current_admin(&state, &principal),
    )
    .await?;
    Ok(Json(json!({"ok":true,"result":{
        "accepted":true,
        "transaction":transaction,
        "mode":mode.as_str()
    }})))
}

/// A published release by its version, or the rebuilt program a check offered,
/// by what that check saw. The path is never taken from the request.
fn update_source(body: &Value) -> Result<UpdateSource, ApiError> {
    let text = |name: &str| {
        body.get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| ApiError::bad(format!("{name} is required")))
    };
    // A start without a source names a release, as before there were two.
    match body.get("source").map(Value::as_str) {
        None | Some(Some("release")) => Ok(UpdateSource::Release {
            version: text("version")?,
        }),
        Some(Some("local")) => Ok(UpdateSource::Local {
            token: text("token")?,
            source_id: text("source_id")?,
        }),
        Some(_) => Err(ApiError::bad("source must be release or local")),
    }
}

fn unavailable() -> ApiError {
    ApiError::conflict(
        "update_unavailable",
        "this CCCC process cannot update itself; update it with `cccc update`",
        json!({}),
    )
}

/// The one handoff of an attended update, for the Web's route and for the
/// host's own interface: the host prepares the update while this Web keeps
/// serving, then this Web closes so the host can finish it. `allowed` is
/// asked again after the preparation and after Voice closes, because a
/// preparation can take minutes. Every way out other than the handoff gives
/// the update up, including the caller going away while the host prepares.
pub(crate) async fn hand_over(
    restart: &crate::RestartHandle,
    voice: &std::sync::Arc<CodexVoiceSessions>,
    request: UpdateRequest,
    allowed: impl Fn() -> Result<(), ApiError>,
) -> Result<String, ApiError> {
    let hook = restart.update_hook().ok_or_else(unavailable)?;
    let preparation = hook.begin(request).map_err(rejected)?;
    let reserved = Reserved(Some((&hook, &preparation.transaction)));
    preparation.ready.await.map_err(rejected)?;
    allowed()?;
    // The Web closes for the update, so Voice stops, as it does for a binding
    // change. It does so only now, so a rejected update leaves Voice running.
    voice
        .prepare_web_restart()
        .await
        .map_err(VoiceGateError::into_api_error)?;
    let reopen = ReopenVoice(Some(voice.as_ref()));
    allowed()?;
    restart.request_update().map_err(|error| {
        ApiError::unavailable(
            "update_handoff_failed",
            format!("failed to close the Web for the update: {error}"),
        )
    })?;
    reopen.keep_closed();
    Ok(reserved.hand_over())
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
