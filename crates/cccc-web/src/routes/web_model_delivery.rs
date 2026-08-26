use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::api::ApiError;
use crate::browser_surface::{
    BOUND_CONVERSATION_ERROR_MARKER, PromptSubmissionOutcome, conversation_url_for_target,
    is_chatgpt_url, stored_verified_submission_evidence,
};

use super::web_model_browser::key;
use super::web_model_delivery_completion::{
    args, call as daemon_call, complete_args, reconcile, record_delivery,
};
use super::web_model_delivery_state::{record_connector, target as load_target, update_target};

static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static WORKERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
pub(super) const IDLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
const DEFERRED_RETRY_BASE: std::time::Duration = std::time::Duration::from_secs(3);
const DEFERRED_MAX_AUTOMATIC_RETRIES: u32 = 3;

fn deferred_retry_delay(retries: u32) -> Option<std::time::Duration> {
    (retries < DEFERRED_MAX_AUTOMATIC_RETRIES).then(|| DEFERRED_RETRY_BASE * (1_u32 << retries))
}

const BOOTSTRAP_SEED_VERSION: &str = "web-model-bootstrap-normal-system-prompt-v2";
const COMPATIBILITY_IMAGE_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAAKUlEQVR42u3OIQEAAAACIP+f1hkWWEB6FgEBAQEBAQEBAQEBAQEBgXdgl/rw4tnPBf0AAAAASUVORK5CYII=";
const COMPATIBILITY_IMAGE_NOTE: &str = "[CCCC] Compatibility attachment: the blank image is transport-only and carries no task context.";
const WEB_TRANSPORT_NOTE: &str = "[CCCC] Web transport:\n\
- This browser conversation is the web surface for the actor above.\n\
- Browser-injected messages are already delivered in chat; do not call cccc_runtime_wait_next_turn for them.\n\
- Use CCCC MCP tools for visible replies, handoffs, local workspace work, validation, and evidence.\n\
- For non-trivial local development work, default to cccc_code_exec so repo reads, patches, tests, diffs, and reports stay in one focused Codex-style loop; use direct tools only for simple one-step actions.\n\
- If CCCC MCP tools are not visible in the selected web model, you do not have CCCC local access in this chat; tell the user to switch to a supported session that can see the CCCC connector.\n\
- Text typed only in this web chat is not delivered to CCCC users or peers.";

struct BootstrapSeed {
    text: String,
    digest: String,
}

struct DeliveryAttempt<'a> {
    turn_id: &'a str,
    event_ids: Value,
    delivery_id: &'a str,
}

pub(super) enum DeliveryOutcome {
    Submitted,
    Idle,
    Deferred(String),
    Ambiguous,
    Stopped,
}

pub(super) async fn ensure_worker(state: AppState, group_id: String, actor_id: String) {
    spawn_worker(state, group_id, actor_id);
}

fn spawn_worker(state: AppState, group_id: String, actor_id: String) {
    let session_key = key(&group_id, &actor_id);
    let Some(worker) = SessionGuard::acquire(&WORKERS, session_key.clone()) else {
        return;
    };
    tokio::spawn(async move {
        // Keep the worker guard in this scope so it is always released before the fresh-turn
        // check below. An event arriving during the final deferred attempt cannot acquire the
        // guard, so that check is responsible for recovering its wake-up.
        let exhausted_turn_id = {
            let _worker = worker;
            let mut exhausted_turn_id = None;
            let mut retry_seconds = 1_u64;
            let mut deferred_turn_id = String::new();
            let mut deferred_retries = 0_u32;
            let mut shutdown = state.shutdown.subscribe();
            loop {
                let surface = state.browser_surfaces.info(&session_key).await;
                if !surface["active"].as_bool().unwrap_or(false) {
                    break;
                }
                let delay = match deliver_pending(&state, &group_id, &actor_id).await {
                    Ok(DeliveryOutcome::Submitted) => {
                        retry_seconds = 1;
                        deferred_turn_id.clear();
                        deferred_retries = 0;
                        std::time::Duration::from_millis(10)
                    }
                    Ok(DeliveryOutcome::Deferred(turn_id)) => {
                        retry_seconds = 1;
                        if deferred_turn_id != turn_id {
                            deferred_turn_id = turn_id;
                            deferred_retries = 0;
                        }
                        let Some(delay) = deferred_retry_delay(deferred_retries) else {
                            tracing::info!(
                                group_id,
                                actor_id,
                                turn_id = deferred_turn_id,
                                "Web-model browser deferred retry budget exhausted"
                            );
                            exhausted_turn_id = Some(deferred_turn_id.clone());
                            break;
                        };
                        deferred_retries += 1;
                        delay
                    }
                    Ok(DeliveryOutcome::Idle | DeliveryOutcome::Ambiguous) => {
                        retry_seconds = 1;
                        deferred_turn_id.clear();
                        deferred_retries = 0;
                        IDLE_POLL_INTERVAL
                    }
                    Ok(DeliveryOutcome::Stopped) => break,
                    Err(error) => {
                        tracing::warn!(
                            group_id,
                            actor_id,
                            %error,
                            "Web-model browser delivery failed; retrying"
                        );
                        retry_seconds = (retry_seconds * 2).min(30);
                        std::time::Duration::from_secs(retry_seconds)
                    }
                };
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {},
                    _ = shutdown.recv() => break,
                }
            }
            exhausted_turn_id
        };

        let Some(exhausted_turn_id) = exhausted_turn_id else {
            return;
        };
        if let Ok(target) = load_target(&state, &group_id, &actor_id) {
            let message =
                "browser model remained unavailable after the bounded automatic retry budget";
            let _ = update_target(
                &state,
                &group_id,
                &actor_id,
                json!({"last_delivery_status":"failed","last_error":message}),
            );
            if let (Some(delivery_id), Some(event_ids)) = (
                target["last_delivery_id"].as_str(),
                target["last_delivery_event_ids"].as_array(),
            ) {
                record_delivery(
                    &state,
                    &group_id,
                    &actor_id,
                    &exhausted_turn_id,
                    Value::Array(event_ids.clone()),
                    delivery_id,
                    "failed",
                    message,
                    json!({"target_url":target["url"]}),
                )
                .await;
            }
        }
        match fresh_turn_after_exhaustion(&state, &group_id, &actor_id, &exhausted_turn_id).await {
            Ok(Some(fresh_turn_id)) => {
                tracing::debug!(
                    group_id,
                    actor_id,
                    exhausted_turn_id,
                    fresh_turn_id,
                    "Rescheduling Web-model browser delivery for fresh direct work"
                );
                spawn_worker(state, group_id, actor_id);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    group_id,
                    actor_id,
                    %error,
                    "Web-model browser fresh unread check failed"
                );
            }
        }
    });
}

async fn fresh_turn_after_exhaustion(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    exhausted_turn_id: &str,
) -> Result<Option<String>, ApiError> {
    if !super::web_model_supervisor::actor_delivery_enabled(state, group_id, actor_id) {
        return Ok(None);
    }
    let wait = daemon_call(
        state,
        "runtime_wait_next_turn",
        browser_wait_args(group_id, actor_id),
    )
    .await?;
    Ok(replacement_turn_id(exhausted_turn_id, &wait))
}

fn replacement_turn_id(exhausted_turn_id: &str, wait: &Value) -> Option<String> {
    if wait["status"] != "work_available" {
        return None;
    }
    wait["turn"]["turn_id"]
        .as_str()
        .map(str::trim)
        .filter(|turn_id| !turn_id.is_empty() && *turn_id != exhausted_turn_id)
        .map(str::to_owned)
}

pub(super) async fn deliver_pending(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
) -> Result<DeliveryOutcome, ApiError> {
    let session_key = key(group_id, actor_id);
    let Some(_delivery) = SessionGuard::acquire(&IN_FLIGHT, session_key.clone()) else {
        return Ok(DeliveryOutcome::Idle);
    };
    deliver_once(state, group_id, actor_id, &session_key).await
}

struct SessionGuard {
    sessions: &'static Mutex<HashSet<String>>,
    key: String,
}

impl SessionGuard {
    fn acquire(storage: &'static OnceLock<Mutex<HashSet<String>>>, key: String) -> Option<Self> {
        let sessions = storage.get_or_init(|| Mutex::new(HashSet::new()));
        let inserted = sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.clone());
        inserted.then_some(Self { sessions, key })
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
    }
}

async fn deliver_once(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    session_key: &str,
) -> Result<DeliveryOutcome, ApiError> {
    if !super::web_model_supervisor::actor_delivery_enabled(state, group_id, actor_id) {
        return Ok(DeliveryOutcome::Stopped);
    }
    let surface = state.browser_surfaces.info(session_key).await;
    if !surface["active"].as_bool().unwrap_or(false) {
        return Ok(DeliveryOutcome::Idle);
    }
    let target = load_target(state, group_id, actor_id)?;
    let target_url = target["url"].as_str().unwrap_or("");
    if target["last_delivery_status"] == "submitting" {
        let message = "browser delivery was interrupted after its at-most-once dispatch fence; the message will not be redelivered automatically";
        let evidence = json!({
            "submitted":false,
            "submission_evidence":"interrupted_dispatch",
            "error":message
        });
        return complete_ambiguous_attempt(
            state,
            group_id,
            actor_id,
            DeliveryAttempt {
                turn_id: required(&target, "last_delivery_turn_id")?,
                event_ids: target["last_delivery_event_ids"].clone(),
                delivery_id: required(&target, "last_delivery_id")?,
            },
            evidence,
            message,
        )
        .await;
    }
    if target["last_delivery_status"] == "legacy_recovery_submitting" {
        let message = "legacy browser recovery was interrupted after dispatch began; the committed message will not be submitted again";
        update_target(
            state,
            group_id,
            actor_id,
            json!({
                "last_delivery_status":"submission_ambiguous",
                "last_delivery_at":cccc_contracts::utc_now(),
                "last_submission_evidence":{
                    "submitted":false,
                    "submission_evidence":"interrupted_legacy_dispatch",
                    "error":message
                },
                "last_error":message
            }),
        )?;
        record_connector(
            state,
            group_id,
            actor_id,
            "ambiguous",
            target["last_delivery_turn_id"].as_str().unwrap_or(""),
            message,
        )?;
        if target["kind"] == "new_chat" {
            return resolve_pending_new_chat(
                state,
                group_id,
                actor_id,
                session_key,
                &load_target(state, group_id, actor_id)?,
            )
            .await;
        }
        return Ok(DeliveryOutcome::Ambiguous);
    }
    if target_url.is_empty() && target["kind"] != "new_chat" {
        return Ok(DeliveryOutcome::Idle);
    }
    if target["last_delivery_status"] == "submission_ambiguous" {
        if recover_verified_ambiguous_submission(state, group_id, actor_id, &target).await? {
            return Ok(DeliveryOutcome::Submitted);
        }
        // The attempted turn was already committed to preserve at-most-once delivery. A known
        // conversation target can therefore continue with later turns without retrying it. A new
        // chat must remain fenced until its conversation URL can be recovered.
        if target["kind"] == "new_chat" {
            return resolve_pending_new_chat(state, group_id, actor_id, session_key, &target).await;
        }
    }
    if is_legacy_pending_delivery(&target) {
        if state
            .browser_surfaces
            .wait_for_conversation_url(session_key, target_url, std::time::Duration::ZERO)
            .await
            .map_err(|error| {
                ApiError::unavailable("web_model_conversation_bind_failed", error.to_string())
            })?
            .is_some()
        {
            return resolve_pending_new_chat(state, group_id, actor_id, session_key, &target).await;
        }
        return recover_legacy_pending_delivery(state, group_id, actor_id, session_key, &target)
            .await;
    }
    if matches!(
        target["last_delivery_status"].as_str(),
        Some(
            "ambiguous"
                | "completion_ambiguous"
                | "submission_ambiguous_completion_pending"
                | "completion_conflict"
        )
    ) {
        if !reconcile(state, group_id, actor_id, &target).await? {
            return Ok(DeliveryOutcome::Ambiguous);
        }
        let reconciled = load_target(state, group_id, actor_id)?;
        if reconciled["last_delivery_status"] == "submission_ambiguous" {
            if reconciled["kind"] == "new_chat" {
                return resolve_pending_new_chat(
                    state,
                    group_id,
                    actor_id,
                    session_key,
                    &reconciled,
                )
                .await;
            }
        } else {
            if reconciled["kind"] == "new_chat" {
                return resolve_pending_new_chat(
                    state,
                    group_id,
                    actor_id,
                    session_key,
                    &reconciled,
                )
                .await;
            }
            return Ok(DeliveryOutcome::Submitted);
        }
    }
    if target["kind"] == "new_chat"
        && matches!(
            target["last_delivery_status"].as_str(),
            Some("submitted" | "pending_new_chat_bind")
        )
    {
        return resolve_pending_new_chat(state, group_id, actor_id, session_key, &target).await;
    }
    if target["kind"] == "existing_chat" && is_chatgpt_url(target_url) {
        if let Err(error) = state
            .browser_surfaces
            .align_chatgpt_conversation_target(
                session_key,
                target_url,
                std::time::Duration::from_secs(5),
            )
            .await
        {
            let message = error.to_string();
            update_target(
                state,
                group_id,
                actor_id,
                json!({
                    "last_delivery_status":"failed",
                    "last_submission_evidence":{
                        "submitted":false,
                        "submission_evidence":"bound_conversation_unavailable",
                        "error":message.as_str()
                    },
                    "last_error":message.as_str()
                }),
            )?;
            record_connector(state, group_id, actor_id, "failed", "", &message)?;
            return Ok(DeliveryOutcome::Stopped);
        }
    }
    let wait = daemon_call(
        state,
        "runtime_wait_next_turn",
        browser_wait_args(group_id, actor_id),
    )
    .await?;
    if wait["status"] != "work_available" {
        return Ok(DeliveryOutcome::Idle);
    }
    let turn = &wait["turn"];
    let turn_id = required(turn, "turn_id")?;
    let delivery_id = browser_delivery_id(actor_id, turn_id);
    let event_label = turn["event_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let (browser_prompt, bootstrap_seed) = build_browser_prompt(
        turn,
        &target,
        target_url,
        actor_id,
        &delivery_id,
        &event_label,
    )?;
    let attachment = compatibility_attachment(state, turn, &delivery_id)?;
    update_target(
        state,
        group_id,
        actor_id,
        json!({"last_delivery_id":delivery_id,"last_delivery_turn_id":turn_id,"last_delivery_event_ids":turn["event_ids"],"last_delivery_status":"submitting","last_delivery_started_at":cccc_contracts::utc_now(),"last_error":""}),
    )?;
    record_delivery(
        state,
        group_id,
        actor_id,
        turn_id,
        turn["event_ids"].clone(),
        &delivery_id,
        "submitting",
        "",
        json!({"target_url":target_url,"auto_bind_new_chat":target["kind"] == "new_chat"}),
    )
    .await;
    let submitted = state
        .browser_surfaces
        .submit_prompt_with_attachment(
            session_key,
            target_url,
            &browser_prompt,
            attachment.as_deref(),
            &delivery_id,
        )
        .await;
    let browser = match submitted {
        Ok(PromptSubmissionOutcome::Verified(browser)) => browser,
        Ok(PromptSubmissionOutcome::Deferred(browser)) => {
            let message = "browser model is not ready for a safe prompt submission";
            update_target(
                state,
                group_id,
                actor_id,
                json!({"last_delivery_status":"deferred","last_submission_evidence":browser,"last_error":message}),
            )?;
            record_connector(state, group_id, actor_id, "deferred", turn_id, message)?;
            return Ok(DeliveryOutcome::Deferred(turn_id.to_owned()));
        }
        Ok(PromptSubmissionOutcome::Ambiguous(browser)) => {
            let message = "browser submission was attempted but could not be verified; this message will not be redelivered automatically";
            return complete_ambiguous_attempt(
                state,
                group_id,
                actor_id,
                DeliveryAttempt {
                    turn_id,
                    event_ids: turn["event_ids"].clone(),
                    delivery_id: &delivery_id,
                },
                browser,
                message,
            )
            .await;
        }
        Err(error) if error.to_string().contains(BOUND_CONVERSATION_ERROR_MARKER) => {
            let message = error.to_string();
            update_target(
                state,
                group_id,
                actor_id,
                json!({
                    "last_delivery_status":"failed",
                    "last_submission_evidence":{
                        "submitted":false,
                        "submission_evidence":"bound_conversation_unavailable",
                        "error":message.as_str()
                    },
                    "last_error":message.as_str()
                }),
            )?;
            record_connector(state, group_id, actor_id, "failed", turn_id, &message)?;
            record_delivery(
                state,
                group_id,
                actor_id,
                turn_id,
                turn["event_ids"].clone(),
                &delivery_id,
                "failed",
                &message,
                json!({"target_url":target_url}),
            )
            .await;
            return Ok(DeliveryOutcome::Stopped);
        }
        Err(error) => {
            let message = format!(
                "browser delivery failed after its at-most-once dispatch fence: {error}; this message will not be redelivered automatically"
            );
            let evidence = json!({
                "submitted":false,
                "submission_evidence":"browser_error_after_dispatch_fence",
                "error":error.to_string()
            });
            return complete_ambiguous_attempt(
                state,
                group_id,
                actor_id,
                DeliveryAttempt {
                    turn_id,
                    event_ids: turn["event_ids"].clone(),
                    delivery_id: &delivery_id,
                },
                evidence,
                &message,
            )
            .await;
        }
    };
    update_target(
        state,
        group_id,
        actor_id,
        completion_pending_patch(
            turn_id,
            turn["event_ids"].clone(),
            browser.clone(),
            bootstrap_seed.as_ref(),
            target_url,
        ),
    )?;
    let submission_evidence = browser["submission_evidence"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    // A verified browser handoff is the terminal delivery fact. Persist it
    // before completing the structured turn so the daemon can validate that
    // every source event actually crossed the runtime boundary.
    record_delivery(
        state,
        group_id,
        actor_id,
        turn_id,
        turn["event_ids"].clone(),
        &delivery_id,
        "submitted",
        &submission_evidence,
        json!({
            "target_url":target_url,
            "auto_bind_new_chat":target["kind"] == "new_chat"
        }),
    )
    .await;
    let complete = complete_args(
        group_id,
        actor_id,
        turn_id,
        turn["event_ids"].clone(),
        &delivery_id,
    );
    if let Err(error) = daemon_call(state, "runtime_complete_turn", complete).await {
        update_target(
            state,
            group_id,
            actor_id,
            json!({"last_delivery_status":"completion_ambiguous","last_delivery_turn_id":turn_id,"last_delivery_event_ids":turn["event_ids"],"last_delivery_reconcile_attempts":0,"last_submission_evidence":browser,"last_error":error.to_string()}),
        )?;
        tracing::warn!(
            group_id,
            actor_id,
            turn_id,
            %error,
            "Web-model browser submission is ambiguous; automatic redelivery is paused"
        );
        return Ok(DeliveryOutcome::Ambiguous);
    }
    let mut pending_new_chat_bind = target["kind"] == "new_chat";
    let mut bind_error = String::new();
    let mut bound_conversation_url = String::new();
    if pending_new_chat_bind {
        match state
            .browser_surfaces
            .wait_for_conversation_url(session_key, target_url, std::time::Duration::from_secs(15))
            .await
        {
            Ok(Some(conversation_url)) => {
                if let Err(error) =
                    bind_new_chat_target(state, group_id, actor_id, &conversation_url)
                {
                    bind_error = error.to_string();
                } else {
                    bound_conversation_url = conversation_url;
                    pending_new_chat_bind = false;
                }
            }
            Ok(None) => {}
            Err(error) => bind_error = error.to_string(),
        }
    }
    let final_status = if pending_new_chat_bind {
        "pending_new_chat_bind"
    } else {
        "submitted"
    };
    let final_error = if !bind_error.is_empty() {
        bind_error.as_str()
    } else if pending_new_chat_bind {
        "conversation_url_pending"
    } else {
        ""
    };
    let now = cccc_contracts::utc_now();
    let mut final_patch = json!({
        "last_delivery_status":final_status,
        "last_delivery_at":now.clone(),
        "last_error":final_error,
        "last_submission_evidence":browser
    });
    if pending_new_chat_bind {
        final_patch.as_object_mut().expect("delivery patch").extend(
            json!({
                "state":"new_chat_submitted",
                "submitted_at":now,
                "delivery_id":delivery_id,
                "next_delivery":"wait_for_new_chat_bind"
            })
            .as_object()
            .cloned()
            .expect("pending new chat patch"),
        );
    }
    update_target(state, group_id, actor_id, final_patch)?;
    // Keep the existing connector-facing status coherent with the target
    // before the best-effort ledger receipt performs an async daemon call.
    // Otherwise observers can see a submitted target while the connector
    // still exposes the preceding MCP probe status.
    record_connector(state, group_id, actor_id, "submitted", turn_id, "")?;
    if !bound_conversation_url.is_empty() {
        record_delivery(
            state,
            group_id,
            actor_id,
            turn_id,
            turn["event_ids"].clone(),
            &delivery_id,
            "bound",
            &submission_evidence,
            json!({
                "target_url":target_url,
                "bound_conversation_url":bound_conversation_url,
                "pending_conversation_url":false,
                "auto_bind_new_chat":true
            }),
        )
        .await;
    }
    if pending_new_chat_bind {
        record_delivery(
            state,
            group_id,
            actor_id,
            turn_id,
            turn["event_ids"].clone(),
            &delivery_id,
            "pending",
            final_error,
            json!({
                "target_url":target_url,
                "pending_conversation_url":true,
                "auto_bind_new_chat":true
            }),
        )
        .await;
    }
    Ok(DeliveryOutcome::Submitted)
}

async fn complete_ambiguous_attempt(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    attempt: DeliveryAttempt<'_>,
    browser: Value,
    message: &str,
) -> Result<DeliveryOutcome, ApiError> {
    update_target(
        state,
        group_id,
        actor_id,
        json!({
            "last_delivery_status":"submission_ambiguous_completion_pending",
            "last_delivery_turn_id":attempt.turn_id,
            "last_delivery_event_ids":attempt.event_ids.clone(),
            "last_delivery_reconcile_attempts":0,
            "last_delivery_at":cccc_contracts::utc_now(),
            "last_submission_evidence":browser,
            "last_error":message
        }),
    )?;
    let complete = complete_args(
        group_id,
        actor_id,
        attempt.turn_id,
        attempt.event_ids.clone(),
        attempt.delivery_id,
    );
    record_delivery(
        state,
        group_id,
        actor_id,
        attempt.turn_id,
        attempt.event_ids.clone(),
        attempt.delivery_id,
        "ambiguous",
        message,
        json!({}),
    )
    .await;
    let completion = daemon_call(state, "runtime_complete_turn", complete).await;
    let completion_status = if completion.is_ok() {
        "submission_ambiguous"
    } else {
        "submission_ambiguous_completion_pending"
    };
    let completion_error = completion
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    update_target(
        state,
        group_id,
        actor_id,
        json!({
            "last_delivery_status":completion_status,
            "last_error":if completion_error.is_empty() {message} else {&completion_error}
        }),
    )?;
    if completion.is_ok() {
        record_connector(
            state,
            group_id,
            actor_id,
            "ambiguous",
            attempt.turn_id,
            message,
        )?;
    }
    tracing::warn!(
        group_id,
        actor_id,
        turn_id = attempt.turn_id,
        completion_recorded = completion.is_ok(),
        "Web-model browser submission could not be verified; the attempted message will not be redelivered automatically"
    );
    Ok(DeliveryOutcome::Ambiguous)
}

async fn recover_verified_ambiguous_submission(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    target: &Value,
) -> Result<bool, ApiError> {
    let submission = &target["last_submission_evidence"];
    let Some(submission_evidence) = stored_verified_submission_evidence(submission) else {
        return Ok(false);
    };
    let turn_id = required(target, "last_delivery_turn_id")?;
    let delivery_id = required(target, "last_delivery_id")?;
    let mut recover_args = args(group_id, actor_id);
    recover_args.insert(
        "event_ids".into(),
        target["last_delivery_event_ids"].clone(),
    );
    let recovered = daemon_call(state, "web_model_runtime_recover_turn", recover_args).await?;
    let turn = &recovered["turn"];
    let target_url = target["url"].as_str().unwrap_or("");
    let observed_url = submission["observed"]["url"].as_str().unwrap_or("");
    let conversation_url = conversation_url_for_target(target_url, observed_url);
    let event_label = target["last_delivery_event_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let (_, bootstrap_seed) = build_browser_prompt(
        turn,
        target,
        target_url,
        actor_id,
        delivery_id,
        &event_label,
    )?;
    if let Some(seed) = &bootstrap_seed {
        mark_bootstrap_seed_delivered(
            state,
            group_id,
            actor_id,
            conversation_url.as_deref().unwrap_or(target_url),
            seed,
        )?;
    }
    if target["kind"] == "new_chat"
        && let Some(conversation_url) = &conversation_url
    {
        bind_new_chat_target(state, group_id, actor_id, conversation_url)?;
    }
    let pending_new_chat_bind = target["kind"] == "new_chat" && conversation_url.is_none();
    let mut recovered_submission = submission.clone();
    if let Some(object) = recovered_submission.as_object_mut() {
        object.insert("submitted".into(), json!(true));
        object.insert("submission_evidence".into(), json!(submission_evidence));
        object.insert("recovered_from".into(), json!("submission_ambiguous"));
    }
    update_target(
        state,
        group_id,
        actor_id,
        json!({
            "last_delivery_status":if pending_new_chat_bind {"pending_new_chat_bind"} else {"submitted"},
            "last_delivery_at":cccc_contracts::utc_now(),
            "last_submission_evidence":recovered_submission,
            "last_error":if pending_new_chat_bind {"conversation_url_pending"} else {""}
        }),
    )?;
    record_connector(state, group_id, actor_id, "submitted", turn_id, "")?;
    tracing::info!(
        group_id,
        actor_id,
        turn_id,
        submission_evidence,
        conversation_bound = conversation_url.is_some(),
        "Recovered a browser submission from persisted direct evidence"
    );
    Ok(true)
}

fn is_legacy_pending_delivery(target: &Value) -> bool {
    target["kind"] == "new_chat"
        && matches!(
            target["last_delivery_status"].as_str(),
            Some("submitted" | "pending_new_chat_bind")
        )
        && target["last_delivery_id"]
            .as_str()
            .is_some_and(|delivery_id| delivery_id.starts_with("wmd_"))
        && target["last_submission_evidence"]["submission_evidence"].as_str()
            != Some("message_echo")
}

async fn recover_legacy_pending_delivery(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    session_key: &str,
    target: &Value,
) -> Result<DeliveryOutcome, ApiError> {
    let event_ids = target["last_delivery_event_ids"].clone();
    let mut recover_args = args(group_id, actor_id);
    recover_args.insert("event_ids".into(), event_ids.clone());
    let recovered = daemon_call(state, "web_model_runtime_recover_turn", recover_args).await?;
    let turn = &recovered["turn"];
    let old_prompt = legacy_wmd_staged_prompt(turn)?;
    let target_url = required(target, "url")?;
    let inspection = state
        .browser_surfaces
        .inspect_staged_prompt(session_key, target_url, &old_prompt)
        .await
        .map_err(|error| {
            ApiError::unavailable("web_model_legacy_inspection_failed", error.to_string())
        })?;
    if !inspection["recoverable"].as_bool().unwrap_or(false) {
        let message = "legacy browser submission cannot be verified automatically; the draft or page state no longer matches the committed turn";
        update_target(
            state,
            group_id,
            actor_id,
            json!({
                "last_delivery_status":"legacy_submission_unverified",
                "last_submission_evidence":inspection,
                "last_error":message
            }),
        )?;
        record_connector(
            state,
            group_id,
            actor_id,
            "ambiguous",
            target["last_delivery_turn_id"].as_str().unwrap_or(""),
            message,
        )?;
        return Ok(DeliveryOutcome::Ambiguous);
    }

    let turn_id = required(turn, "turn_id")?;
    let delivery_id = browser_delivery_id(actor_id, turn_id);
    let event_label = turn["event_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let (browser_prompt, bootstrap_seed) = build_browser_prompt(
        turn,
        target,
        target_url,
        actor_id,
        &delivery_id,
        &event_label,
    )?;
    let attachment = compatibility_attachment(state, turn, &delivery_id)?;
    update_target(
        state,
        group_id,
        actor_id,
        json!({
            "last_delivery_id":delivery_id,
            "last_delivery_turn_id":turn_id,
            "last_delivery_event_ids":event_ids,
            "last_delivery_status":"legacy_recovery_submitting",
            "last_delivery_started_at":cccc_contracts::utc_now(),
            "last_error":""
        }),
    )?;
    let browser = match state
        .browser_surfaces
        .submit_prompt_with_attachment(
            session_key,
            target_url,
            &browser_prompt,
            attachment.as_deref(),
            &delivery_id,
        )
        .await
    {
        Ok(PromptSubmissionOutcome::Verified(browser)) => browser,
        Ok(PromptSubmissionOutcome::Deferred(browser)) => {
            let message = "legacy delivery was safely restaged, but ChatGPT did not expose an enabled Send control";
            update_target(
                state,
                group_id,
                actor_id,
                json!({
                    "last_delivery_status":"legacy_submission_unverified",
                    "last_submission_evidence":browser,
                    "last_error":message
                }),
            )?;
            return Ok(DeliveryOutcome::Deferred(turn_id.to_owned()));
        }
        Ok(PromptSubmissionOutcome::Ambiguous(browser)) => {
            let message = "legacy recovery attempted submission but could not verify whether ChatGPT accepted it; automatic redelivery is paused";
            update_target(
                state,
                group_id,
                actor_id,
                json!({
                    "last_delivery_status":"submission_ambiguous",
                    "last_submission_evidence":browser,
                    "last_error":message,
                    "last_delivery_at":cccc_contracts::utc_now()
                }),
            )?;
            record_connector(state, group_id, actor_id, "ambiguous", turn_id, message)?;
            return Ok(DeliveryOutcome::Ambiguous);
        }
        Err(error) => {
            update_target(
                state,
                group_id,
                actor_id,
                json!({"last_delivery_status":"failed","last_error":error.to_string()}),
            )?;
            return Err(ApiError::unavailable(
                "web_model_legacy_recovery_failed",
                error.to_string(),
            ));
        }
    };
    if let Some(seed) = &bootstrap_seed {
        mark_bootstrap_seed_delivered(state, group_id, actor_id, target_url, seed)?;
    }
    let conversation_url = state
        .browser_surfaces
        .wait_for_conversation_url(session_key, target_url, std::time::Duration::from_secs(15))
        .await
        .map_err(|error| {
            ApiError::unavailable("web_model_conversation_bind_failed", error.to_string())
        })?;
    let pending = conversation_url.is_none();
    if let Some(conversation_url) = conversation_url {
        bind_new_chat_target(state, group_id, actor_id, &conversation_url)?;
    }
    update_target(
        state,
        group_id,
        actor_id,
        json!({
            "last_delivery_status":if pending {"pending_new_chat_bind"} else {"submitted"},
            "last_delivery_at":cccc_contracts::utc_now(),
            "last_submission_evidence":browser,
            "last_error":if pending {"conversation_url_pending"} else {""}
        }),
    )?;
    record_connector(state, group_id, actor_id, "submitted", turn_id, "")?;
    Ok(DeliveryOutcome::Submitted)
}

fn legacy_wmd_staged_prompt(turn: &Value) -> Result<String, ApiError> {
    let actor_id = required(turn, "actor_id")?;
    let messages = turn["messages"]
        .as_array()
        .ok_or_else(|| ApiError::bad("recovered runtime turn missing messages"))?;
    let mut output = messages
        .iter()
        .map(|event| {
            let by = event["by"].as_str().unwrap_or_default();
            let text = event["data"]["text"].as_str().unwrap_or_default();
            format!("[{by} -> {actor_id}] {text}")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if output.chars().count() > 24_000 {
        output = output.chars().take(23_920).collect();
        output.push_str("\n\n[cccc] coalesced turn text truncated");
    }
    Ok(output)
}

fn browser_delivery_id(actor_id: &str, turn_id: &str) -> String {
    let turn_key = turn_id.rsplit(':').next().unwrap_or(turn_id);
    format!("webdelivery:{actor_id}:{turn_key}")
}

fn browser_wait_args(group_id: &str, actor_id: &str) -> serde_json::Map<String, Value> {
    let mut request = args(group_id, actor_id);
    request.insert("transport".into(), json!("web_model_browser"));
    request
}

fn build_browser_prompt(
    turn: &Value,
    target: &Value,
    target_url: &str,
    actor_id: &str,
    delivery_id: &str,
    event_label: &str,
) -> Result<(String, Option<BootstrapSeed>), ApiError> {
    let prompt = required(turn, "coalesced_text")?;
    let system_prompt = required(turn, "system_prompt")?;
    let seed_text = format!(
        "[CCCC] Session bootstrap for this browser chat:\n\n{system_prompt}\n\n{WEB_TRANSPORT_NOTE}"
    );
    let digest = bootstrap_seed_digest(&seed_text);
    let seed_required = target["bootstrap_seed_delivered_at"]
        .as_str()
        .is_none_or(str::is_empty)
        || target["bootstrap_seed_version"].as_str() != Some(BOOTSTRAP_SEED_VERSION)
        || target["bootstrap_seed_digest"].as_str() != Some(digest.as_str())
        || target["bootstrap_seed_conversation_url"].as_str() != Some(target_url);
    let seed = seed_required.then_some(BootstrapSeed {
        text: seed_text,
        digest,
    });
    let setup = seed
        .as_ref()
        .map(|seed| format!("{}\n\n", seed.text))
        .unwrap_or_default();
    let compatibility_note = if turn["delivery"]["web_model_mode"] == "image_compat" {
        format!("{COMPATIBILITY_IMAGE_NOTE}\n")
    } else {
        String::new()
    };
    Ok((
        format!(
            "{setup}[cccc] Browser batch {delivery_id} events={event_label} actor={actor_id}\n{compatibility_note}{prompt}"
        ),
        seed,
    ))
}

fn compatibility_attachment(
    state: &AppState,
    turn: &Value,
    delivery_id: &str,
) -> Result<Option<PathBuf>, ApiError> {
    if turn["delivery"]["web_model_mode"] != "image_compat" {
        return Ok(None);
    }
    let (filename, bytes) = compatibility_image_for_delivery(delivery_id)?;
    let directory = state.home.root().join("cache/web-model");
    std::fs::create_dir_all(&directory).map_err(|error| {
        ApiError::unavailable("web_model_attachment_cache_failed", error.to_string())
    })?;
    let path = directory.join(filename);
    let current = std::fs::read(&path).ok();
    if current.as_deref() != Some(bytes.as_slice()) {
        cccc_core::fs::atomic_write(&path, &bytes).map_err(|error| {
            ApiError::unavailable("web_model_attachment_cache_failed", error.to_string())
        })?;
    }
    Ok(Some(path))
}

fn compatibility_image_for_delivery(delivery_id: &str) -> Result<(String, Vec<u8>), ApiError> {
    let delivery_id = delivery_id.trim();
    if delivery_id.is_empty() {
        return Err(ApiError::bad("compatibility image delivery_id is required"));
    }
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode(COMPATIBILITY_IMAGE_B64)
        .map_err(|error| ApiError::bad(format!("decode compatibility image: {error}")))?;
    let digest = format!("{:x}", Sha256::digest(delivery_id.as_bytes()));
    let iend_offset = bytes
        .len()
        .checked_sub(12)
        .filter(|offset| bytes.get(*offset + 4..*offset + 8) == Some(b"IEND"))
        .ok_or_else(|| ApiError::bad("compatibility image is missing its terminal PNG chunk"))?;
    let mut marker = b"CCCC-Delivery\0".to_vec();
    marker.extend_from_slice(digest.as_bytes());
    let marker_len = u32::try_from(marker.len())
        .map_err(|_| ApiError::bad("compatibility image marker is too large"))?;
    let mut chunk = Vec::with_capacity(marker.len() + 12);
    chunk.extend_from_slice(&marker_len.to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&marker);
    let mut checksum = crc32fast::Hasher::new();
    checksum.update(b"tEXt");
    checksum.update(&marker);
    chunk.extend_from_slice(&checksum.finalize().to_be_bytes());
    bytes.splice(iend_offset..iend_offset, chunk);
    Ok((format!("cccc-mcp-compat-{}.png", &digest[..16]), bytes))
}

fn bootstrap_seed_digest(seed: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(seed.as_bytes()));
    digest[..20].to_owned()
}

fn completion_pending_patch(
    turn_id: &str,
    event_ids: Value,
    browser: Value,
    bootstrap_seed: Option<&BootstrapSeed>,
    target_url: &str,
) -> Value {
    let mut patch = json!({
        "last_delivery_status":"completion_ambiguous",
        "last_delivery_turn_id":turn_id,
        "last_delivery_event_ids":event_ids,
        "last_delivery_reconcile_attempts":0,
        "last_delivery_at":cccc_contracts::utc_now(),
        "last_submission_evidence":browser,
        "last_error":"delivery_completion_pending"
    });
    if let Some(seed) = bootstrap_seed {
        patch["bootstrap_seed_delivered_at"] = json!(cccc_contracts::utc_now());
        patch["bootstrap_seed_version"] = json!(BOOTSTRAP_SEED_VERSION);
        patch["bootstrap_seed_digest"] = json!(seed.digest);
        patch["bootstrap_seed_conversation_url"] = json!(target_url);
    }
    patch
}

fn mark_bootstrap_seed_delivered(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    target_url: &str,
    seed: &BootstrapSeed,
) -> Result<(), ApiError> {
    update_target(
        state,
        group_id,
        actor_id,
        json!({
            "bootstrap_seed_delivered_at":cccc_contracts::utc_now(),
            "bootstrap_seed_version":BOOTSTRAP_SEED_VERSION,
            "bootstrap_seed_digest":seed.digest,
            "bootstrap_seed_conversation_url":target_url
        }),
    )
}

async fn resolve_pending_new_chat(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    session_key: &str,
    target: &Value,
) -> Result<DeliveryOutcome, ApiError> {
    let target_url = target["url"].as_str().unwrap_or("");
    let conversation_url = state
        .browser_surfaces
        .wait_for_conversation_url(session_key, target_url, std::time::Duration::ZERO)
        .await
        .map_err(|error| {
            ApiError::unavailable("web_model_conversation_bind_failed", error.to_string())
        })?;
    let Some(conversation_url) = conversation_url else {
        update_target(
            state,
            group_id,
            actor_id,
            json!({"last_delivery_status":"pending_new_chat_bind","last_error":"conversation_url_pending"}),
        )?;
        return Ok(DeliveryOutcome::Ambiguous);
    };
    bind_new_chat_target(state, group_id, actor_id, &conversation_url)?;
    update_target(
        state,
        group_id,
        actor_id,
        json!({"last_delivery_status":"submitted","last_error":""}),
    )?;
    if let (Some(turn_id), Some(delivery_id), Some(event_ids)) = (
        target["last_delivery_turn_id"].as_str(),
        target["last_delivery_id"].as_str(),
        target["last_delivery_event_ids"].as_array(),
    ) {
        record_delivery(
            state,
            group_id,
            actor_id,
            turn_id,
            Value::Array(event_ids.clone()),
            delivery_id,
            "bound",
            "conversation_url_bound",
            json!({
                "target_url":target_url,
                "bound_conversation_url":conversation_url,
                "resolved_pending_new_chat":true
            }),
        )
        .await;
    }
    Ok(DeliveryOutcome::Submitted)
}

fn bind_new_chat_target(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    conversation_url: &str,
) -> Result<(), ApiError> {
    let now = cccc_contracts::utc_now();
    update_target(
        state,
        group_id,
        actor_id,
        json!({
            "state":"bound_existing_chat",
            "kind":"existing_chat",
            "url":conversation_url,
            "saved_at":now,
            "bound_at":now,
            "next_delivery":"existing_chat",
            "bootstrap_seed_conversation_url":conversation_url
        }),
    )
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    value[key]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad(format!("runtime turn missing {key}")))
}
