//! Bounded connection during Actor startup or explicit retry. Status GETs never send.
use super::{web_model_connector_store as store, web_model_delivery_completion::call};
use crate::{
    AppState,
    api::{ApiError, ApiResult, success},
    browser_surface::{PairingPage, PromptSubmissionOutcome},
};
use axum::{Json, Router, extract::State, routing::post};
use cccc_core::web_model_connectors;
use serde_json::{Map, Value, json};
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
    time::Duration,
};

static CONNECTING: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
const PAIRING_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/web-model/pairing", post(change))
}

pub(super) fn is_connecting(state: &AppState, pairing_id: &str) -> bool {
    CONNECTING
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(&(state.runtime_id.clone(), pairing_id.to_owned()))
}
struct Connecting((String, String));
impl Connecting {
    fn new(state: &AppState, id: &str) -> Self {
        let key = (state.runtime_id.clone(), id.to_owned());
        CONNECTING
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.clone());
        Self(key)
    }
}
impl Drop for Connecting {
    fn drop(&mut self) {
        CONNECTING
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.0);
    }
}

async fn change(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group = super::web_model_connectors::required(&body, "group_id")?;
    let actor = super::web_model_connectors::required(&body, "actor_id")?;
    let action = super::web_model_connectors::required(&body, "action")?;
    if !matches!(action.as_str(), "connect" | "cancel" | "remove") {
        return Err(ApiError::bad("invalid pairing action"));
    }
    let connector = store::load(&state)?
        .into_iter()
        .find(|c| c["revoked"] != true)
        .ok_or_else(|| ApiError::bad("Configure the shared ChatGPT connector first"))?;
    let mut args = json!({"by":"user","group_id":group,"actor_id":actor,"connector_id":connector["connector_id"]})
        .as_object().expect("literal JSON object").clone();
    // Cancellation invalidates this exact candidate immediately, including while
    // its send is in flight. It cannot cancel a newer attempt or an old binding.
    if action == "cancel" {
        args.insert(
            "pairing_id".into(),
            json!(super::web_model_connectors::required(&body, "pairing_id")?),
        );
        return Ok(success(
            call(&state, "web_model_pairing_cancel", args).await?,
        ));
    }
    if action == "remove" {
        let _control = super::web_model_delivery::control_guard(&group, &actor)?;
        return Ok(success(
            call(&state, "web_model_binding_remove", args).await?,
        ));
    }
    start_connection(&state, &group, &actor, false)
        .await
        .map(success)
}

/// The first attempt follows normal Actor startup. An existing attempt, even
/// expired/cancelled/interrupted, requires an explicit retry; never replay sends.
pub(super) async fn ensure_automatic(state: &AppState, group: &str, actor: &str) {
    let _ = start_connection(state, group, actor, true).await;
}

async fn start_connection(
    state: &AppState,
    group: &str,
    actor: &str,
    first_attempt: bool,
) -> Result<Value, ApiError> {
    let control = super::web_model_delivery::control_guard(group, actor)?;
    let connector = store::load(state)?
        .into_iter()
        .find(|c| c["revoked"] != true)
        .ok_or_else(|| ApiError::bad("Configure the shared ChatGPT connector first"))?;
    let doc = cccc_core::GroupStore::new(state.home.clone())
        .and_then(|s| s.load(group))
        .map_err(|e| ApiError::bad(e.to_string()))?;
    let config = doc
        .actors
        .iter()
        .find(|a| a.id == actor)
        .ok_or_else(|| ApiError::bad("Actor not found"))?;
    let automatic = config.enabled;
    if first_attempt
        && (!automatic
            || web_model_connectors::binding_for_actor(
                &connector,
                group,
                actor,
                &cccc_core::actors::generation_identity(config),
            )
            .is_some()
            || web_model_connectors::pairing_for_actor(&connector, group, actor)
                .is_some_and(|p| p["generation"] == cccc_core::actors::generation_identity(config)))
    {
        return Ok(json!({}));
    }
    let mut args = json!({"by":"user","group_id":group,"actor_id":actor,
        "connector_id":connector["connector_id"],"automatic":automatic})
    .as_object()
    .expect("literal JSON object")
    .clone();
    let key = super::web_model_browser::key(group, actor);
    let target = state
        .browser_surfaces
        .pairing_page(&key)
        .await
        .map_err(|e| pairing_error(&e.to_string()))?;
    let pair = call(state, "web_model_pairing_begin", args.clone()).await?;
    let id = pair["pairing_id"]
        .as_str()
        .ok_or_else(|| ApiError::bad("Invalid pairing response"))?
        .to_owned();
    let code = pair["code"]
        .as_str()
        .ok_or_else(|| ApiError::bad("Invalid pairing response"))?;
    let prompt = format!(
        "Connect this conversation to the Actor selected in CCCC. Send this temporary CCCC pairing code to cccc_pair through the same CCCC connector: {{\"code\":\"{code}\"}}. This records a pending link; reply with the returned receipt exactly as plain text so CCCC can verify this window and activate the link. Subsequent CCCC tool calls will use that Actor's configured permissions. This setup does not read workspace files or run tasks; wait for the next task message. If ChatGPT requires my approval, ask me. If the call is blocked or fails, report the error."
    );
    args.insert("pairing_id".into(), json!(id));
    let connecting = Connecting::new(state, &id);
    let mut shutdown = state.shutdown.subscribe();
    // Move the control permit into the task before responding: duplicate clicks,
    // navigation and delivery cannot race the same Actor's handshake. Dropping
    // the browser UI does not interrupt it; no request timeout drives retries.
    let state = state.clone();
    let group = group.to_owned();
    let actor = actor.to_owned();
    tokio::spawn(async move {
        let result = tokio::select! {
            _ = shutdown.recv() => Err("pairing_interrupted"),
            result = tokio::time::timeout(PAIRING_TIMEOUT, connect(&state, &args, &key, &target, &prompt)) => {
                result.unwrap_or(Err("pairing_timeout"))
            }
        };
        if let Err(reason) = result {
            let mut failed = args.clone();
            failed.insert("error_code".into(), json!(reason));
            // A cancelled/replaced/removed candidate deliberately rejects this
            // late update. Never change another attempt or an established route.
            let _ = call(&state, "web_model_pairing_fail", failed).await;
        }
        drop(control);
        drop(connecting);
        if result.is_ok() && automatic {
            super::web_model_delivery::ensure_worker(state, group, actor).await;
        }
    });
    Ok(json!({"state":"waiting","pairing_id":id,"expires_at_ms":pair["expires_at_ms"]}))
}

async fn connect(
    state: &AppState,
    args: &Map<String, Value>,
    key: &str,
    target: &PairingPage,
    prompt: &str,
) -> Result<(), &'static str> {
    candidate(state, args)?;
    match state
        .browser_surfaces
        .submit_pairing_prompt(key, target, prompt)
        .await
    {
        Ok(PromptSubmissionOutcome::Verified(_)) => {}
        Ok(PromptSubmissionOutcome::Ambiguous(v) | PromptSubmissionOutcome::Deferred(v)) => {
            return Err(if v["submission_evidence"] == "composer_occupied" {
                "pairing_composer_occupied"
            } else {
                "pairing_submission_uncertain"
            });
        }
        Err(error) => {
            return Err(match error.to_string().as_str() {
                "pairing_target_changed" => "pairing_target_changed",
                "pairing_composer_occupied" => "pairing_composer_occupied",
                _ => "pairing_submission_uncertain",
            });
        }
    }
    loop {
        let pair = candidate(state, args)?;
        let url = target
            .receipt_url(
                &state.browser_surfaces,
                key,
                prompt,
                pair["receipt"].as_str(),
            )
            .await
            .map_err(|error| {
                let code = if error.to_string() == "pairing_target_changed" {
                    "pairing_target_changed"
                } else {
                    "pairing_interrupted"
                };
                // Browser errors may embed page text or URLs. Log only the
                // stage and safe category, not receipt/prompt/transport payloads.
                tracing::warn!(
                    code,
                    stage = "receipt_inspection",
                    "Web Model pairing check failed"
                );
                code
            })?;
        if pair["state"] == "awaiting_confirmation"
            && let Some(url) = url
        {
            let mut confirm = args.clone();
            confirm.insert("url".into(), json!(url));
            call(state, "web_model_pairing_confirm", confirm)
                .await
                .map_err(|_| "pairing_failed")?;
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn candidate(state: &AppState, args: &Map<String, Value>) -> Result<Value, &'static str> {
    let connector = store::load(state)
        .map_err(|_| "pairing_failed")?
        .into_iter()
        .find(|c| c["connector_id"] == args["connector_id"] && c["revoked"] != true)
        .ok_or("pairing_interrupted")?;
    let pair = web_model_connectors::pairing_for_actor(
        &connector,
        args["group_id"].as_str().unwrap_or_default(),
        args["actor_id"].as_str().unwrap_or_default(),
    )
    .filter(|p| {
        p["pairing_id"] == args["pairing_id"]
            && matches!(
                p["state"].as_str(),
                Some("waiting" | "awaiting_confirmation")
            )
    })
    .ok_or("pairing_interrupted")?;
    if pair["expires_at_ms"]
        .as_i64()
        .is_none_or(|t| t <= chrono::Utc::now().timestamp_millis())
    {
        return Err("pairing_timeout");
    }
    // Lifecycle can change through another port while the browser is waiting.
    let group = cccc_core::GroupStore::new(state.home.clone())
        .and_then(|s| s.load(args["group_id"].as_str().unwrap_or_default()))
        .map_err(|_| "pairing_interrupted")?;
    if !group.actors.iter().any(|a| {
        a.id == args["actor_id"]
            && (if pair["automatic"] == true {
                a.enabled
                    && group.running
                    && !matches!(
                        group.state,
                        cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
                    )
            } else {
                !a.enabled
            })
            && a.runtime == cccc_contracts::ActorRuntime::WebModel
            && cccc_core::actors::generation_identity(a) == pair["generation"]
    }) {
        return Err("pairing_interrupted");
    }
    Ok(pair)
}

fn pairing_error(code: &str) -> ApiError {
    let code = match code {
        "pairing_browser_required" => "pairing_browser_required",
        "pairing_browser_not_ready" => "pairing_browser_not_ready",
        "pairing_composer_occupied" => "pairing_composer_occupied",
        "pairing_target_changed" => "pairing_target_changed",
        _ => "pairing_failed",
    };
    ApiError::bad_code(code, code, json!({}))
}

#[cfg(test)]
mod tests;
