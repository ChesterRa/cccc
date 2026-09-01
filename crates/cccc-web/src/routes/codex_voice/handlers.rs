use super::*;
use axum::Json;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use cccc_daemon::experimental_codex_voice::{
    DEFAULT_REALTIME_VOICE, REALTIME_VOICES, RealtimeCallConfig,
};
use serde_json::{Value, json};
use std::time::Duration;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::codex_voice::{SessionInfo, StartOutcome};

pub(super) async fn active(State(state): State<AppState>) -> ApiResult {
    require_interactive_web(&state)?;
    let current = state.codex_voice.current().await;
    let readiness = codex_voice_readiness().await;
    Ok(success(json!({
        "call":current.call.map(payload::info_value),
        "analyst":current.analyst.map(payload::analyst_info_value),
        "voices":REALTIME_VOICES,
        "default_voice":DEFAULT_REALTIME_VOICE,
        "readiness":readiness,
    })))
}

async fn codex_voice_readiness() -> Value {
    let codex_cli_available = cccc_runtime::resolve_executable_in_path("codex", None).is_some();
    let codex_credentials_available = match RealtimeCallConfig::from_environment() {
        Ok(config) => read_codex_credentials_available(&config.auth_path).await,
        Err(_) => false,
    };
    json!({
        "codex_cli_available":codex_cli_available,
        "codex_credentials_available":codex_credentials_available,
    })
}

async fn read_codex_credentials_available(path: &std::path::Path) -> bool {
    const MAX_AUTH_BYTES: u64 = 1024 * 1024;
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return false;
    };
    if !metadata.is_file() || metadata.len() > MAX_AUTH_BYTES {
        return false;
    }
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    codex_credentials_available(&bytes)
}

pub(super) fn codex_credentials_available(bytes: &[u8]) -> bool {
    let Ok(auth) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    ["access_token", "account_id"].into_iter().all(|key| {
        auth.get("tokens")
            .and_then(|tokens| tokens.get(key))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

pub(super) async fn start(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
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
        .start(&state.home, &group_id, client_session_id, offer_sdp, voice)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %group_id, "Codex Voice start failed");
            ApiError::unavailable(
                "codex_voice_unavailable",
                "Codex Voice could not start. Check the initial repository attachment, Codex login, and current Voice status.",
            )
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
                spawn_attach_deadline(state.clone(), info.clone());
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

pub(super) async fn reset_analyst(
    State(state): State<AppState>,
    Path((group_id, generation)): Path<(String, String)>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let analyst = state
        .codex_voice
        .reset_analyst(&state.home, &group_id, &generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %group_id, %generation, "Voice Analyst reset failed");
            ApiError::conflict(
                "codex_voice_analyst_reset_failed",
                "The Voice Analyst could not start a new session. Stop or cancel current work first.",
                json!({"group_id":group_id,"generation":generation}),
            )
        })?;
    Ok(success(
        json!({"analyst":payload::analyst_info_value(analyst)}),
    ))
}

pub(super) async fn cancel_analyst(
    State(state): State<AppState>,
    Path((group_id, generation)): Path<(String, String)>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let cancelled = state
        .codex_voice
        .cancel_analyst(&group_id, &generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %group_id, %generation, "Voice Analyst cancellation failed");
            ApiError::conflict(
                "codex_voice_analyst_cancel_failed",
                "The Voice Analyst could not cancel the current investigation.",
                json!({"group_id":group_id,"generation":generation}),
            )
        })?;
    Ok(success(json!({"cancelled":cancelled})))
}

pub(super) async fn stop(
    State(state): State<AppState>,
    Path((group_id, generation)): Path<(String, String)>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let stopped = state
        .codex_voice
        .stop(&group_id, &generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %group_id, %generation, "Codex Voice stop failed");
            ApiError::unavailable(
                "codex_voice_stop_failed",
                "Codex Voice could not release the live call cleanly.",
            )
        })?;
    Ok(success(json!({"stopped":stopped})))
}

pub(super) async fn upgrade(
    State(state): State<AppState>,
    Path((group_id, generation)): Path<(String, String)>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    require_interactive_web(&state)?;
    let attachment = state
        .codex_voice
        .attach(&group_id, &generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %group_id, %generation, "Codex Voice call attach failed");
            ApiError::conflict(
                "codex_voice_not_attachable",
                "This Codex Voice call can no longer accept a browser connection.",
                json!({"group_id":group_id,"generation":generation}),
            )
        })?;
    Ok(ws.on_upgrade(move |socket| voice_socket::serve(socket, state, attachment)))
}

pub(super) async fn upgrade_terminal(
    State(state): State<AppState>,
    Path((group_id, generation)): Path<(String, String)>,
    Query(query): Query<TerminalQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    require_interactive_web(&state)?;
    let session = state
        .codex_voice
        .terminal_session(&group_id, &generation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %group_id, %generation, "Voice Analyst terminal lookup failed");
            ApiError::conflict(
                "codex_voice_terminal_unavailable",
                "The Voice Analyst terminal is not available yet.",
                json!({"group_id":group_id,"generation":generation}),
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

fn spawn_attach_deadline(state: AppState, info: SessionInfo) {
    tokio::spawn(async move {
        let deadline = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(deadline);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                _ = heartbeat.tick() => match state
                    .codex_voice
                    .heartbeat_if_unattached(&info.group_id, &info.generation)
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => return,
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            group_id = %info.group_id,
                            generation = %info.generation,
                            "failed to renew unattached Codex Voice recording lease"
                        );
                        break;
                    }
                },
            }
        }
        match state
            .codex_voice
            .stop_if_unattached(&info.group_id, &info.generation)
            .await
        {
            Ok(true) => tracing::info!(
                group_id = %info.group_id,
                generation = %info.generation,
                "stopped unattached Codex Voice call"
            ),
            Ok(false) => {}
            Err(error) => tracing::warn!(
                %error,
                group_id = %info.group_id,
                generation = %info.generation,
                "failed to stop unattached Codex Voice call"
            ),
        }
    });
}
