use super::*;
use axum::Json;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use cccc_daemon::experimental_codex_voice::{DEFAULT_REALTIME_VOICE, REALTIME_VOICES};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::codex_voice::StartOutcome;

mod attach_deadline;
mod settings;
pub(super) use settings::{analyst_settings, update_analyst_settings};

pub(super) async fn active(State(state): State<AppState>) -> ApiResult {
    require_interactive_web(&state)?;
    let current = state.codex_voice.current().await;
    let readiness = settings::codex_voice_readiness(&state.home).await;
    Ok(success(json!({
        "call":current.call.map(payload::info_value),
        "analyst":current.analyst.map(payload::analyst_info_value),
        "voices":REALTIME_VOICES,
        "default_voice":DEFAULT_REALTIME_VOICE,
        "readiness":readiness,
    })))
}

pub(super) async fn start(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    require_interactive_web(&state)?;
    let offer_sdp = body["offer_sdp"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad("offer_sdp is required"))?;
    let client_session_id = body["client_session_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad("client_session_id is required"))?;
    let voice = body["voice"].as_str().unwrap_or(DEFAULT_REALTIME_VOICE);
    let outcome = state
        .codex_voice
        .start(&state.home, client_session_id, offer_sdp, voice)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "Codex Voice start failed");
            voice_start_error(&error)
        })?;
    match outcome {
        StartOutcome::Busy(info) => Err(ApiError::conflict(
            "codex_voice_busy",
            "Another Codex Voice call is already active.",
            json!({"call":payload::info_value(info)}),
        )),
        StartOutcome::Started(started) => {
            let info = started.session.info();
            if started.newly_created {
                attach_deadline::spawn(state.clone(), info.clone());
            }
            Ok(success(json!({
                "call":payload::info_value(info),
                "analyst":payload::analyst_info_value(started.session.analyst().info()),
                "answer_sdp":started.answer_sdp,
                "experimental":true,
            })))
        }
    }
}

fn voice_start_error(error: &anyhow::Error) -> ApiError {
    if format!("{error:#}").contains("upgrade to 1.18.14 or newer") {
        return ApiError::unavailable(
            "opencode_upgrade_required",
            "OpenCode 1.18.14 or newer is required. Upgrade OpenCode, then restart the Analyst.",
        );
    }
    ApiError::unavailable(
        "codex_voice_unavailable",
        "Codex Voice could not start. Check the Analyst Runtime Profile, Realtime Voice login, and current Voice status.",
    )
}

pub(super) async fn reset_analyst(
    State(state): State<AppState>,
    Path(generation): Path<String>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let analyst = state
        .codex_voice
        .reset_analyst(&state.home, &generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %generation, "Voice Analyst reset failed");
            ApiError::conflict(
                "codex_voice_analyst_reset_failed",
                "The Voice Analyst could not start a new session. Stop or cancel current work first.",
                json!({"generation":generation}),
            )
        })?;
    Ok(success(
        json!({"analyst":payload::analyst_info_value(analyst)}),
    ))
}

pub(super) async fn cancel_analyst(
    State(state): State<AppState>,
    Path(generation): Path<String>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let cancelled = state
        .codex_voice
        .cancel_analyst(&generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %generation, "Voice Analyst cancellation failed");
            ApiError::conflict(
                "codex_voice_analyst_cancel_failed",
                "The Voice Analyst could not cancel the current investigation.",
                json!({"generation":generation}),
            )
        })?;
    Ok(success(json!({"cancelled":cancelled})))
}

pub(super) async fn stop(
    State(state): State<AppState>,
    Path(generation): Path<String>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let stopped = state.codex_voice.stop(&generation).await.map_err(|error| {
        tracing::warn!(%error, %generation, "Codex Voice stop failed");
        ApiError::unavailable(
            "codex_voice_stop_failed",
            "Codex Voice could not release the live call cleanly.",
        )
    })?;
    Ok(success(json!({"stopped":stopped})))
}

pub(super) async fn upgrade(
    State(state): State<AppState>,
    Path(generation): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    require_interactive_web(&state)?;
    let attachment = state
        .codex_voice
        .attach(&generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %generation, "Codex Voice call attach failed");
            ApiError::conflict(
                "codex_voice_not_attachable",
                "This Codex Voice call can no longer accept a browser connection.",
                json!({"generation":generation}),
            )
        })?;
    Ok(ws.on_upgrade(move |socket| voice_socket::serve(socket, state, attachment)))
}

pub(super) async fn upgrade_terminal(
    State(state): State<AppState>,
    Path(generation): Path<String>,
    Query(query): Query<TerminalQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    require_interactive_web(&state)?;
    let session = state
        .codex_voice
        .terminal_session(&generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %generation, "Voice Analyst terminal lookup failed");
            ApiError::conflict(
                "codex_voice_terminal_unavailable",
                "The Voice Analyst terminal is not available yet.",
                json!({"generation":generation}),
            )
        })?;
    Ok(ws.on_upgrade(move |socket| terminal::serve(socket, state, session, query)))
}

fn require_interactive_web(state: &AppState) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden_code(
            "read_only",
            "Codex Voice is unavailable in read-only Web mode.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_version_failure_is_actionable_through_the_voice_api() {
        let error = anyhow::anyhow!(
            "OpenCode 1.18.13 cannot host a reliable managed ACP session; upgrade to 1.18.14 or newer"
        )
        .context("launch persistent Voice Analyst");
        let mapped = voice_start_error(&error);

        assert!(mapped.to_string().starts_with("opencode_upgrade_required:"));
    }
}
