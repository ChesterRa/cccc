use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::api::ApiError;
use crate::browser_surface::{
    PromptSubmissionOutcome, conversation_url_for_target, stored_verified_submission_evidence,
};

use super::web_model_browser::key;
use super::web_model_delivery_completion::{args, call as daemon_call, complete_args, reconcile};
use super::web_model_delivery_state::{record_connector, target as load_target, update_target};

static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static WORKERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
pub(super) const IDLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

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

pub(super) enum DeliveryOutcome {
    Submitted,
    Idle,
    Deferred,
    Ambiguous,
}

pub(super) async fn ensure_worker(state: AppState, group_id: String, actor_id: String) {
    let session_key = key(&group_id, &actor_id);
    let Some(worker) = SessionGuard::acquire(&WORKERS, session_key.clone()) else {
        return;
    };
    tokio::spawn(async move {
        let _worker = worker;
        let mut retry_seconds = 1_u64;
        let mut shutdown = state.shutdown.subscribe();
        loop {
            let surface = state.browser_surfaces.info(&session_key).await;
            if !surface["active"].as_bool().unwrap_or(false) {
                break;
            }
            let delay = match deliver_pending(&state, &group_id, &actor_id).await {
                Ok(DeliveryOutcome::Submitted) => {
                    retry_seconds = 1;
                    std::time::Duration::from_millis(10)
                }
                Ok(
                    DeliveryOutcome::Idle | DeliveryOutcome::Deferred | DeliveryOutcome::Ambiguous,
                ) => {
                    retry_seconds = 1;
                    IDLE_POLL_INTERVAL
                }
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
    });
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
    let surface = state.browser_surfaces.info(session_key).await;
    if !surface["active"].as_bool().unwrap_or(false) {
        return Ok(DeliveryOutcome::Idle);
    }
    let target = load_target(state, group_id, actor_id)?;
    let target_url = target["url"].as_str().unwrap_or("");
    if target_url.is_empty() && target["kind"] != "new_chat" {
        return Ok(DeliveryOutcome::Idle);
    }
    if target["last_delivery_status"] == "submission_ambiguous" {
        if recover_verified_ambiguous_submission(state, group_id, actor_id, &target).await? {
            return Ok(DeliveryOutcome::Submitted);
        }
        return Ok(DeliveryOutcome::Ambiguous);
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
            return Ok(DeliveryOutcome::Ambiguous);
        }
        if reconciled["kind"] == "new_chat" {
            return resolve_pending_new_chat(state, group_id, actor_id, session_key, &reconciled)
                .await;
        }
        return Ok(DeliveryOutcome::Submitted);
    }
    if target["kind"] == "new_chat"
        && matches!(
            target["last_delivery_status"].as_str(),
            Some("submitted" | "pending_new_chat_bind")
        )
    {
        return resolve_pending_new_chat(state, group_id, actor_id, session_key, &target).await;
    }
    let wait = daemon_call(
        state,
        "web_model_runtime_wait_next_turn",
        args(group_id, actor_id),
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
            return Ok(DeliveryOutcome::Deferred);
        }
        Ok(PromptSubmissionOutcome::Ambiguous(browser)) => {
            let message = "browser submission was attempted but could not be verified; automatic redelivery is paused";
            let complete = complete_args(
                group_id,
                actor_id,
                turn_id,
                turn["event_ids"].clone(),
                &delivery_id,
            );
            let completion = daemon_call(state, "web_model_runtime_complete_turn", complete).await;
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
                    "last_delivery_turn_id":turn_id,
                    "last_delivery_event_ids":turn["event_ids"],
                    "last_delivery_reconcile_attempts":0,
                    "last_delivery_at":cccc_contracts::utc_now(),
                    "last_submission_evidence":browser,
                    "last_error":if completion_error.is_empty() {message} else {&completion_error}
                }),
            )?;
            if completion.is_ok() {
                record_connector(state, group_id, actor_id, "ambiguous", turn_id, message)?;
            }
            tracing::warn!(
                group_id,
                actor_id,
                turn_id,
                cursor_committed = completion.is_ok(),
                "Web-model browser submission could not be verified; automatic redelivery is paused"
            );
            return Ok(DeliveryOutcome::Ambiguous);
        }
        Err(error) => {
            update_target(
                state,
                group_id,
                actor_id,
                json!({"last_delivery_status":"failed","last_error":error.to_string()}),
            )?;
            record_connector(state, group_id, actor_id, "failed", "", &error.to_string())?;
            return Err(ApiError::unavailable(
                "web_model_delivery_failed",
                error.to_string(),
            ));
        }
    };
    if let Some(seed) = &bootstrap_seed {
        mark_bootstrap_seed_delivered(state, group_id, actor_id, target_url, seed)?;
    }
    let mut pending_new_chat_bind = target["kind"] == "new_chat";
    if pending_new_chat_bind {
        if let Some(conversation_url) = state
            .browser_surfaces
            .wait_for_conversation_url(session_key, target_url, std::time::Duration::from_secs(15))
            .await
            .map_err(|error| {
                ApiError::unavailable("web_model_conversation_bind_failed", error.to_string())
            })?
        {
            bind_new_chat_target(state, group_id, actor_id, &conversation_url)?;
            pending_new_chat_bind = false;
        }
    }
    let complete = complete_args(
        group_id,
        actor_id,
        turn_id,
        turn["event_ids"].clone(),
        &delivery_id,
    );
    if let Err(error) = daemon_call(state, "web_model_runtime_complete_turn", complete).await {
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
    let final_status = if pending_new_chat_bind {
        "pending_new_chat_bind"
    } else {
        "submitted"
    };
    let final_error = if pending_new_chat_bind {
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
    record_connector(state, group_id, actor_id, "submitted", turn_id, "")?;
    Ok(DeliveryOutcome::Submitted)
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
            return Ok(DeliveryOutcome::Deferred);
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        BOOTSTRAP_SEED_VERSION, bootstrap_seed_digest, browser_delivery_id, build_browser_prompt,
        compatibility_image_for_delivery,
    };

    #[test]
    fn browser_delivery_id_is_stable_per_turn_and_distinct_across_turns() {
        assert_eq!(
            browser_delivery_id("web1", "webturn:web1:abc123"),
            "webdelivery:web1:abc123"
        );
        assert_ne!(
            browser_delivery_id("web1", "webturn:web1:abc123"),
            browser_delivery_id("web1", "webturn:web1:def456")
        );
    }

    #[test]
    fn compatibility_images_are_visually_identical_but_unique_per_delivery() {
        let (first_name, first) = compatibility_image_for_delivery("webdelivery:web1:first")
            .expect("first compatibility image");
        let (same_name, same) = compatibility_image_for_delivery("webdelivery:web1:first")
            .expect("same compatibility image");
        let (second_name, second) = compatibility_image_for_delivery("webdelivery:web1:second")
            .expect("second compatibility image");

        assert_eq!(first_name, same_name);
        assert_eq!(first, same);
        assert_ne!(first_name, second_name);
        assert_ne!(first, second);
        assert!(first.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(second.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(&first[first.len() - 8..first.len() - 4], b"IEND");
        assert_eq!(&second[second.len() - 8..second.len() - 4], b"IEND");
        assert!(
            first
                .windows(b"CCCC-Delivery\0".len())
                .any(|value| value == b"CCCC-Delivery\0")
        );
        assert!(
            second
                .windows(b"CCCC-Delivery\0".len())
                .any(|value| value == b"CCCC-Delivery\0")
        );
    }

    #[test]
    fn browser_prompt_bootstraps_once_per_bound_conversation_and_prompt_revision() {
        let turn = json!({
            "coalesced_text":"[cccc] message hello",
            "system_prompt":"[CCCC] You are web1 in group test"
        });
        let url = "https://chatgpt.com/c/test";
        let (first, seed) = build_browser_prompt(
            &turn,
            &json!({}),
            url,
            "web1",
            "webdelivery:web1:one",
            "event-one",
        )
        .expect("first prompt");
        let seed = seed.expect("bootstrap seed");
        assert!(first.contains("[CCCC] Session bootstrap for this browser chat:"));
        assert!(first.contains("[CCCC] You are web1 in group test"));
        assert!(first.contains("[CCCC] Web transport:"));
        assert!(first.contains("[cccc] Browser batch webdelivery:web1:one"));
        assert_eq!(seed.digest, bootstrap_seed_digest(&seed.text));

        let seeded_target = json!({
            "bootstrap_seed_delivered_at":"2026-08-07T00:00:00Z",
            "bootstrap_seed_version":BOOTSTRAP_SEED_VERSION,
            "bootstrap_seed_digest":seed.digest,
            "bootstrap_seed_conversation_url":url
        });
        let (next, next_seed) = build_browser_prompt(
            &turn,
            &seeded_target,
            url,
            "web1",
            "webdelivery:web1:two",
            "event-two",
        )
        .expect("next prompt");
        assert!(next_seed.is_none());
        assert!(!next.contains("Session bootstrap"));
        assert!(next.contains("[cccc] Browser batch webdelivery:web1:two"));

        let (_, rebound_seed) = build_browser_prompt(
            &turn,
            &seeded_target,
            "https://chatgpt.com/c/other",
            "web1",
            "webdelivery:web1:three",
            "event-three",
        )
        .expect("rebound prompt");
        assert!(rebound_seed.is_some());
    }

    #[test]
    fn image_compat_prompt_explains_that_the_blank_image_has_no_task_context() {
        let turn = json!({
            "coalesced_text":"[user -> web1] hello",
            "system_prompt":"[CCCC] You are web1",
            "delivery":{"web_model_mode":"image_compat"}
        });
        let (prompt, _) = build_browser_prompt(
            &turn,
            &json!({}),
            "https://chatgpt.com/",
            "web1",
            "webdelivery:web1:image",
            "event-one",
        )
        .expect("image compatibility prompt");
        assert!(prompt.contains("blank image is transport-only"));
        assert!(prompt.contains("carries no task context"));
    }
}
