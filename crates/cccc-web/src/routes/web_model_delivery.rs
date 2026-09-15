use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use cccc_core::web_model_connectors::BrowserTargetOwner;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::api::ApiError;
use crate::browser_surface::{
    BOUND_CONVERSATION_ERROR_MARKER, PromptSubmissionOutcome, conversation_url_for_target,
    stored_verified_submission_evidence,
};

use super::web_model_browser::{key, surface_key};
use super::web_model_delivery_completion::{
    args, call as daemon_call, complete_args, reconcile, record_delivery,
};
use super::web_model_delivery_state::{record_connector, snapshot, update_target};

static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static WORKERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
const BOOTSTRAP_SEED_VERSION: &str = "web-model-bootstrap-normal-system-prompt-v2";
const COMPATIBILITY_IMAGE_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAAKUlEQVR42u3OIQEAAAACIP+f1hkWWEB6FgEBAQEBAQEBAQEBAQEBgXdgl/rw4tnPBf0AAAAASUVORK5CYII=";
const COMPATIBILITY_IMAGE_NOTE: &str = "[CCCC] Compatibility attachment: the blank image is transport-only and carries no task context.";
const WEB_TRANSPORT_NOTE: &str = "[CCCC] Web transport:\n\
- This browser conversation is the web surface for the actor above.\n\
- Browser-injected messages are already delivered in chat; do not call cccc_runtime_wait_next_turn for them.\n\
- Use CCCC MCP tools for visible replies, handoffs, local workspace work, validation, and evidence.\n\
- A completed member report remains the human-facing output. After reviewing it, call cccc_coordination(action=\"decide\", event_ids=[...], decision=\"continue\"|\"wait_user\"|\"complete\"|\"blocked\") only to record machine responsibility. Keep your normal assistant reply for people and do not repeat the report in summary. Reading or replying alone does not resolve the handoff.\n\
- continue must include next_actor_id, next_title, and next_text so real work is created and delivered. End this turn only after the decision result says caller_may_idle=true; safe_to_idle tells whether the whole group may be quiet.\n\
- For non-trivial local development work, default to cccc_code_exec so repo reads, patches, tests, diffs, and reports stay in one focused Codex-style loop; use direct tools only for simple one-step actions.\n\
- If cccc_coordination is visible but rejects decide as an unavailable action, this conversation cached an older connector schema. Create and bind a new chat; do not substitute add_decision or add_handoff because they do not resolve responsibility.\n\
- If CCCC MCP tools are not visible in the selected web model, you do not have CCCC local access in this chat; tell the user to switch to a supported session that can see the CCCC connector.\n\
- Text typed only in this web chat is not delivered to CCCC users or peers.";

struct BootstrapSeed {
    text: String,
    digest: String,
}

struct DeliveryAttempt<'a> {
    owner: &'a BrowserTargetOwner,
    turn_id: &'a str,
    event_ids: Value,
    delivery_id: &'a str,
}

pub(super) enum DeliveryOutcome {
    Submitted,
    Idle,
    Deferred,
    Ambiguous,
    Stopped,
}

pub(super) fn retryable_pre_send_deferral(evidence: &Value) -> bool {
    matches!(
        evidence["submission_evidence"].as_str(),
        Some("not_sent_chat_busy" | "not_sent_composer_occupied" | "not_sent_composer_unavailable")
    )
}

fn requires_browser_action(evidence: &Value) -> bool {
    matches!(
        evidence["submission_evidence"].as_str(),
        Some(
            "not_sent_conversation_archived"
                | "not_sent_rate_limited"
                | "not_sent_access_denied"
                | "not_sent_verification_required"
                | "not_sent_login_required"
        )
    )
}

pub(super) async fn ensure_worker(state: AppState, group_id: String, actor_id: String) {
    let session_key = key(&group_id, &actor_id);
    let Some(worker) = SessionGuard::acquire(&WORKERS, session_key) else {
        return;
    };
    tokio::spawn(async move {
        let _worker = worker;
        if let Err(error) = visit_pending(&state, &group_id, &actor_id, true).await {
            tracing::warn!(group_id, actor_id, %error, "Browser delivery visit ended; original work retained");
        }
    });
}

// Component tests own their browser lifetime; production always uses an automatic visit.
#[cfg(test)]
pub(super) async fn deliver_pending(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
) -> Result<DeliveryOutcome, ApiError> {
    visit_pending(state, group_id, actor_id, false).await
}

async fn visit_pending(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    manage_browser: bool,
) -> Result<DeliveryOutcome, ApiError> {
    let session_key = key(group_id, actor_id);
    let Some(_delivery) = SessionGuard::acquire(&IN_FLIGHT, session_key.clone()) else {
        return Ok(DeliveryOutcome::Idle);
    };
    let _operation = state.browser_surfaces.web_model_operation.lock().await;
    if super::web_model_browser::another_chat_is_pending(state, group_id, actor_id)? {
        return Ok(DeliveryOutcome::Idle);
    }
    let surface = state.browser_surfaces.info(surface_key()).await;
    let automatic = surface["active"] != true
        || state
            .browser_surfaces
            .web_model_auto_close
            .load(std::sync::atomic::Ordering::Acquire);
    if automatic
        && super::web_model_browser::next_delivery_check(&state.home)?
            .is_some_and(|at| chrono::Utc::now() < at)
    {
        return Ok(DeliveryOutcome::Idle);
    }
    let result = deliver_once(state, group_id, actor_id, surface_key(), manage_browser).await;
    if state
        .browser_surfaces
        .web_model_auto_close
        .load(std::sync::atomic::Ordering::Acquire)
    {
        // Keep a newly created chat until its durable /c/... target is known.
        // Never close a human draft, a login window explicitly opened by the
        // user, or a different group's in-flight navigation.
        let target = super::web_model_delivery_state::target(state, group_id, actor_id)?;
        let pending_url = target["kind"] == "new_chat"
            && matches!(
                target["last_delivery_status"].as_str(),
                Some(
                    "pending_new_chat_bind"
                        | "submitted"
                        | "completion_ambiguous"
                        | "submission_ambiguous"
                )
            );
        let visited = state.browser_surfaces.info(surface_key()).await["active"] == true;
        let mut pending_receipt = false;
        if !pending_url && visited {
            let blocked = state
                .browser_surfaces
                .relay_surface_deferral(surface_key())
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?;
            let has_draft = blocked
                .as_ref()
                .is_some_and(|b| b["composer_chars"].as_u64().unwrap_or(0) > 0);
            // Close only after the receipt transaction above has settled, not
            // merely because a second page inspection looks different.
            let submission = &target["last_submission_evidence"];
            pending_receipt = submission["submission_evidence"] == "optimistic_echo_unconfirmed";
            let close_receipt_check = target["last_delivery_status"] == "submitted"
                && match submission["submission_evidence"].as_str() {
                    Some("message_echo") => submission["receipt_needles"]
                        .as_array()
                        .is_some_and(|needles| !needles.is_empty()),
                    Some("user_message_count_increased") => {
                        submission["observed"]["user_message_count"]
                            .as_u64()
                            .zip(submission["baseline"]["user_message_count"].as_u64())
                            .is_some_and(|(observed, baseline)| observed > baseline)
                    }
                    _ => false,
                };
            if !has_draft && !pending_receipt && close_receipt_check {
                pending_receipt = match state
                    .browser_surfaces
                    .relay_receipt_stable_before_close(surface_key(), submission)
                    .await
                {
                    Ok(stable) => !stable,
                    Err(error) => {
                        tracing::debug!(%error, "could not confirm a stable browser receipt before close");
                        true
                    }
                };
            }
            // An optimistic or unstable receipt is still an in-flight Send,
            // not idle time. Only the final browser close pays this guard.
            if !has_draft && !pending_receipt {
                state
                    .browser_surfaces
                    .close(surface_key())
                    .await
                    .map_err(|error| ApiError::bad(error.to_string()))?;
            }
        }
        if visited {
            let retry = target["last_delivery_status"] == "deferred"
                || pending_url
                || pending_receipt
                || result.is_err()
                || matches!(result, Ok(DeliveryOutcome::Deferred));
            super::web_model_browser::schedule_delivery_check(&state.home, retry)?;
        }
    }
    result
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
        // Construct an owning guard only after acquisition succeeds. Eager
        // then_some drops the rejected guard and releases the current owner.
        inserted.then(|| Self { sessions, key })
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
    manage_browser: bool,
) -> Result<DeliveryOutcome, ApiError> {
    if super::web_model_browser::automation_hold(&state.home).is_some()
        || !super::web_model_supervisor::actor_delivery_enabled(state, group_id, actor_id)
    {
        return Ok(DeliveryOutcome::Stopped);
    }
    let surface = state.browser_surfaces.info(session_key).await;
    let (target, target_owner) = snapshot(state, group_id, actor_id)?;
    let owner = &target_owner;
    let target_url = target["url"].as_str().unwrap_or("");
    if surface["active"] != true
        && target["last_submission_evidence"]["submission_evidence"]
            == "bound_conversation_unavailable"
    {
        return Ok(DeliveryOutcome::Stopped);
    }

    if target["last_delivery_status"] == "preparing" {
        return retry_unsubmitted_turn(
            state,
            group_id,
            actor_id,
            DeliveryAttempt {
                owner,
                turn_id: required(&target, "last_delivery_turn_id")?,
                event_ids: target["last_delivery_event_ids"].clone(),
                delivery_id: required(&target, "last_delivery_id")?,
            },
            "browser preparation was interrupted before any Send action",
        )
        .await;
    }
    let mut retained_busy_deferral = target["last_delivery_status"] == "deferred"
        && (retryable_pre_send_deferral(&target["last_submission_evidence"])
            || requires_browser_action(&target["last_submission_evidence"]));
    if retained_busy_deferral {
        let statuses = daemon_call(
            state,
            "ledger_statuses",
            json!({
                "group_id":group_id, "event_ids":target["last_delivery_event_ids"]
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        )
        .await?;
        let ids = target["last_delivery_event_ids"].as_array();
        let handled = ids.is_some_and(|ids| {
            !ids.is_empty()
                && ids.iter().all(|id| {
                    matches!(
                        statuses["statuses"][id.as_str().unwrap_or("")]["obligation_status"]
                            [actor_id]["delivery_state"]
                            .as_str(),
                        Some("accepted" | "ambiguous")
                    )
                })
        });
        if handled {
            let unverified = ids.is_some_and(|ids| {
                ids.iter().any(|id| {
                        statuses["statuses"][id.as_str().unwrap_or("")]["obligation_status"]
                            [actor_id]["delivery_state"]
                            == "ambiguous"
                    })
            });
            update_target(
                state,
                group_id,
                actor_id,
                owner,
                json!({
                    "last_delivery_status":if unverified {"submission_ambiguous"} else {"handled"},
                    "last_error":if unverified {"The earlier delivery remains unverified; it will not be automatically resubmitted."} else {""}
                }),
            )?;
            retained_busy_deferral = false;
        }
    }
    if retained_busy_deferral
        && requires_browser_action(&target["last_submission_evidence"])
        && surface["active"] != true
    {
        return Ok(DeliveryOutcome::Stopped);
    }
    if retained_busy_deferral && surface["active"] == true {
        if state
            .browser_surfaces
            .web_model_auto_close
            .load(std::sync::atomic::Ordering::Acquire)
        {
            super::web_model_browser::schedule_delivery_check(&state.home, true)?;
        }
        // A previously archived target needs explicit restoration/replacement.
        // Do not keep navigating back from another group's working conversation.
        if target["last_submission_evidence"]["submission_evidence"]
            == "not_sent_conversation_archived"
            && surface["url"] != target["url"]
        {
            return Ok(DeliveryOutcome::Stopped);
        }
        let blocked = state
            .browser_surfaces
            .relay_target_deferral(session_key, target_url)
            .await
            .map_err(|e| ApiError::unavailable("web_model_browser_probe_failed", e.to_string()))?;
        if let Some(browser) = blocked {
            super::web_model_browser::hold_on_restriction(state, &browser)?;
            let needs_action = requires_browser_action(&browser);
            if manage_browser
                && !needs_action
                && browser["composer_chars"].as_u64().unwrap_or(0) == 0
            {
                state
                    .browser_surfaces
                    .web_model_auto_close
                    .store(true, std::sync::atomic::Ordering::Release);
                super::web_model_browser::schedule_delivery_check(&state.home, true)?;
            }
            if browser["submission_evidence"]
                != target["last_submission_evidence"]["submission_evidence"]
            {
                update_target(
                    state,
                    group_id,
                    actor_id,
                    owner,
                    json!({"last_submission_evidence":browser}),
                )?;
            }
            return Ok(if needs_action {
                DeliveryOutcome::Stopped
            } else {
                DeliveryOutcome::Idle
            });
        }
    }
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
                owner,
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
            owner,
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
            owner,
            "ambiguous",
            target["last_delivery_turn_id"].as_str().unwrap_or(""),
            message,
        )?;
        if target["kind"] == "new_chat" {
            return resolve_pending_new_chat(
                state,
                group_id,
                actor_id,
                owner,
                session_key,
                &target,
            )
            .await;
        }
        return Ok(DeliveryOutcome::Ambiguous);
    }
    if target_url.is_empty() && target["kind"] != "new_chat" {
        return Ok(DeliveryOutcome::Idle);
    }
    if target["last_delivery_status"] == "submission_ambiguous" {
        if recover_verified_ambiguous_submission(
            state,
            group_id,
            actor_id,
            owner,
            &target,
            manage_browser,
        )
        .await?
        {
            return Ok(DeliveryOutcome::Submitted);
        }
        // The attempted turn was already committed to preserve at-most-once delivery. A known
        // conversation target can therefore continue with later turns without retrying it. A new
        // chat must remain fenced until its conversation URL can be recovered.
        if target["kind"] == "new_chat" {
            return resolve_pending_new_chat(
                state,
                group_id,
                actor_id,
                owner,
                session_key,
                &target,
            )
            .await;
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
            return resolve_pending_new_chat(
                state,
                group_id,
                actor_id,
                owner,
                session_key,
                &target,
            )
            .await;
        }
        return recover_legacy_pending_delivery(
            state,
            group_id,
            actor_id,
            owner,
            session_key,
            &target,
        )
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
        if !reconcile(state, group_id, actor_id, owner, &target).await? {
            return Ok(DeliveryOutcome::Ambiguous);
        }
        let (reconciled, reconciled_owner) = snapshot(state, group_id, actor_id)?;
        if &reconciled_owner != owner {
            return Ok(DeliveryOutcome::Idle);
        }
        if reconciled["last_delivery_status"] == "submission_ambiguous" {
            if reconciled["kind"] == "new_chat" {
                return resolve_pending_new_chat(
                    state,
                    group_id,
                    actor_id,
                    owner,
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
                    owner,
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
        return resolve_pending_new_chat(state, group_id, actor_id, owner, session_key, &target)
            .await;
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
    if !update_target(
        state,
        group_id,
        actor_id,
        owner,
        json!({
            "last_delivery_id":delivery_id, "last_delivery_turn_id":turn_id,
            "last_delivery_event_ids":turn["event_ids"], "last_delivery_status":"preparing",
            "last_delivery_started_at":cccc_contracts::utc_now(),
            "last_submission_evidence":null, "last_error":""
        }),
    )? {
        return retry_unsubmitted_turn(
            state,
            group_id,
            actor_id,
            DeliveryAttempt {
                owner,
                turn_id,
                event_ids: turn["event_ids"].clone(),
                delivery_id: &delivery_id,
            },
            "browser binding or target changed before preparation",
        )
        .await;
    }
    if manage_browser && surface["active"] == true {
        let blocker = state
            .browser_surfaces
            .relay_surface_deferral(session_key)
            .await
            .map_err(|error| ApiError::bad(error.to_string()))?;
        // A real admitted batch takes ownership of an already-open delivery page.
        // Login and human drafts remain owned by the user until resolved.
        if blocker.as_ref().is_none_or(|b| {
            b["composer_chars"].as_u64().unwrap_or(0) == 0 && !requires_browser_action(b)
        }) {
            state
                .browser_surfaces
                .web_model_auto_close
                .store(true, std::sync::atomic::Ordering::Release);
            super::web_model_browser::schedule_delivery_check(&state.home, true)?;
        }
    }
    if surface["active"] != true {
        // Reserve the account-wide five-minute delay BEFORE any network work;
        // failed launches and process restarts must not turn into retry storms.
        super::web_model_browser::schedule_delivery_check(&state.home, true)?;
        state
            .browser_surfaces
            .web_model_auto_close
            .store(true, std::sync::atomic::Ordering::Release);
        if let Err(error) = super::web_model_browser::ensure_open_for_actor_locked(
            state, group_id, actor_id, 1366, 900,
        )
        .await
        {
            return retry_unsubmitted_turn(
                state,
                group_id,
                actor_id,
                DeliveryAttempt {
                    owner,
                    turn_id,
                    event_ids: turn["event_ids"].clone(),
                    delivery_id: &delivery_id,
                },
                &error.to_string(),
            )
            .await;
        }
    }
    let target_owner = owner.for_delivery(&delivery_id);
    let owner = &target_owner;
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
    let submitted = state
        .browser_surfaces
        .submit_prompt_with_attachment_before_dispatch(
            session_key,
            target_url,
            &browser_prompt,
            attachment.as_deref(),
            &delivery_id,
            || async {
                if !super::web_model_supervisor::actor_delivery_enabled(state, group_id, actor_id) {
                    return Err(anyhow::anyhow!("actor stopped during browser preparation"));
                }
                record_delivery(
                    state,
                    group_id,
                    actor_id,
                    turn_id,
                    turn["event_ids"].clone(),
                    &delivery_id,
                    "submitting",
                    "",
                    json!({"target_url":target_url,
                    "auto_bind_new_chat":target["kind"] == "new_chat"}),
                )
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                acquire_dispatch_permit(state, group_id, actor_id, owner, "submitting").await
            },
        )
        .await;
    let submitted = if matches!(
        &submitted,
        Err(_) | Ok(PromptSubmissionOutcome::Deferred(_))
    ) {
        state
            .browser_surfaces
            .clear_staged_prompt(session_key, &browser_prompt, &delivery_id)
            .await
            .and(submitted)
    } else {
        submitted
    };
    let browser = match submitted {
        Ok(PromptSubmissionOutcome::Verified(browser)) => browser,
        Ok(PromptSubmissionOutcome::Deferred(browser)) => {
            super::web_model_browser::hold_on_restriction(state, &browser)?;
            let needs_action = requires_browser_action(&browser);
            let busy = retryable_pre_send_deferral(&browser);
            let message = "browser model is not ready for a safe prompt submission";
            update_target(
                state,
                group_id,
                actor_id,
                owner,
                json!({"last_delivery_status":"deferred","last_submission_evidence":browser,"last_error":message}),
            )?;
            record_delivery(
                state,
                group_id,
                actor_id,
                turn_id,
                turn["event_ids"].clone(),
                &delivery_id,
                "failed",
                message,
                json!({"target_url":target_url}),
            )
            .await?;
            record_connector(
                state, group_id, actor_id, owner, "deferred", turn_id, message,
            )?;
            // A running Chat or an unsent human draft is not a failed connection.
            // Reuse the existing idle cadence rather than exhausting technical retries.
            return Ok(if needs_action {
                DeliveryOutcome::Stopped
            } else if busy {
                DeliveryOutcome::Idle
            } else {
                DeliveryOutcome::Deferred
            });
        }
        Ok(PromptSubmissionOutcome::Ambiguous(browser)) => {
            let browser = if manage_browser
                && browser["submission_evidence"] == "optimistic_echo_unconfirmed"
            {
                state
                    .browser_surfaces
                    .refresh_optimistic_submission(
                        session_key,
                        target_url,
                        &browser_prompt,
                        &browser,
                    )
                    .await
            } else {
                browser
            };
            if browser["submitted"] != true {
                let message = "browser submission was attempted but could not be verified; this message will not be redelivered automatically";
                return complete_ambiguous_attempt(
                    state,
                    group_id,
                    actor_id,
                    DeliveryAttempt {
                        owner,
                        turn_id,
                        event_ids: turn["event_ids"].clone(),
                        delivery_id: &delivery_id,
                    },
                    browser,
                    message,
                )
                .await;
            }
            browser
        }
        Err(error) if error.to_string().contains(BOUND_CONVERSATION_ERROR_MARKER) => {
            let message = error.to_string();
            update_target(
                state,
                group_id,
                actor_id,
                owner,
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
            record_connector(
                state, group_id, actor_id, owner, "failed", turn_id, &message,
            )?;
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
            .await?;
            return Ok(DeliveryOutcome::Stopped);
        }
        Err(error) => {
            return retry_unsubmitted_turn(
                state,
                group_id,
                actor_id,
                DeliveryAttempt {
                    owner,
                    turn_id,
                    event_ids: turn["event_ids"].clone(),
                    delivery_id: &delivery_id,
                },
                &error.to_string(),
            )
            .await;
        }
    };
    update_target(
        state,
        group_id,
        actor_id,
        owner,
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
    .await?;
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
            owner,
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
                    bind_new_chat_target(state, group_id, actor_id, owner, &conversation_url)
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
    update_target(state, group_id, actor_id, owner, final_patch)?;
    // Keep the existing connector-facing status coherent with the target
    // before the best-effort ledger receipt performs an async daemon call.
    // Otherwise observers can see a submitted target while the connector
    // still exposes the preceding MCP probe status.
    record_connector(state, group_id, actor_id, owner, "submitted", turn_id, "")?;
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
        .await?;
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
        .await?;
    }
    Ok(DeliveryOutcome::Submitted)
}

async fn acquire_dispatch_permit(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    owner: &BrowserTargetOwner,
    status: &str,
) -> anyhow::Result<std::fs::File> {
    let home = state.home.clone();
    let group_id = group_id.to_owned();
    let actor_id = actor_id.to_owned();
    let owner = owner.clone();
    let status = status.to_owned();
    tokio::task::spawn_blocking(move || {
        let permit = cccc_core::web_model_connectors::browser_dispatch_permit(
            &home, &group_id, &actor_id, &owner,
        )?
        .ok_or_else(|| anyhow::anyhow!("browser binding or target changed before Send"))?;
        let patch = json!({"last_delivery_status":status});
        if !cccc_core::web_model_connectors::update_browser_target(
            &home,
            &group_id,
            &actor_id,
            &owner,
            patch.as_object().expect("dispatch patch"),
        )? {
            anyhow::bail!("browser attempt changed before Send");
        }
        Ok(permit)
    })
    .await?
}

// Reuse the native failed-delivery transition: it releases this exact reservation,
// leaves the original message intact, and never acknowledges it as received.
async fn retry_unsubmitted_turn(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    attempt: DeliveryAttempt<'_>,
    error: &str,
) -> Result<DeliveryOutcome, ApiError> {
    let owner = attempt.owner;
    record_delivery(
        state,
        group_id,
        actor_id,
        attempt.turn_id,
        attempt.event_ids,
        attempt.delivery_id,
        "failed",
        error,
        json!({}),
    )
    .await?;
    update_target(
        state,
        group_id,
        actor_id,
        owner,
        json!({
            "last_delivery_status":"deferred", "last_error":error,
            "last_submission_evidence":{"submitted":false,
                "submission_evidence":"not_sent_before_dispatch", "error":error}
        }),
    )?;
    record_connector(
        state,
        group_id,
        actor_id,
        owner,
        "deferred",
        attempt.turn_id,
        error,
    )?;
    Ok(DeliveryOutcome::Deferred)
}

async fn complete_ambiguous_attempt(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    attempt: DeliveryAttempt<'_>,
    browser: Value,
    message: &str,
) -> Result<DeliveryOutcome, ApiError> {
    let owner = attempt.owner;
    let reconcile_attempts =
        u64::from(browser["reconciled_by"].as_str() == Some("single_page_refresh"));
    update_target(
        state,
        group_id,
        actor_id,
        owner,
        json!({
            "last_delivery_status":"submission_ambiguous_completion_pending",
            "last_delivery_turn_id":attempt.turn_id,
            "last_delivery_event_ids":attempt.event_ids.clone(),
            "last_delivery_reconcile_attempts":reconcile_attempts,
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
    .await?;
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
        owner,
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
            owner,
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
    owner: &BrowserTargetOwner,
    target: &Value,
    manage_browser: bool,
) -> Result<bool, ApiError> {
    let mut submission = target["last_submission_evidence"].clone();
    let provisional = submission["submission_evidence"] == "optimistic_echo_unconfirmed";
    if stored_verified_submission_evidence(&submission).is_none() && !provisional {
        return Ok(false);
    }
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
    let event_label = target["last_delivery_event_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let (prompt, bootstrap_seed) = build_browser_prompt(
        turn,
        target,
        target_url,
        actor_id,
        delivery_id,
        &event_label,
    )?;
    if provisional {
        let surface = state.browser_surfaces.info(surface_key()).await;
        let automatic = state
            .browser_surfaces
            .web_model_auto_close
            .load(std::sync::atomic::Ordering::Acquire);
        if manage_browser && surface["active"] == true && !automatic {
            // A manually opened production page may contain a draft. Never refresh it.
            return Ok(false);
        }
        if surface["active"] == true && surface["url"] == target["url"] {
            let current = state
                .browser_surfaces
                .inspect_staged_prompt(surface_key(), target_url, &prompt)
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?;
            submission["observed"] = current["observed"].clone();
        }
        if stored_verified_submission_evidence(&submission).is_none()
            && submission["observed"]["response_started"] != true
        {
            if !manage_browser
                || target["last_delivery_reconcile_attempts"]
                    .as_u64()
                    .unwrap_or(0)
                    >= 1
            {
                return Ok(false);
            }
            if surface["active"] != true || surface["url"] != target["url"] {
                state
                    .browser_surfaces
                    .web_model_auto_close
                    .store(true, std::sync::atomic::Ordering::Release);
                if super::web_model_browser::ensure_open_for_actor_locked(
                    state, group_id, actor_id, 1366, 900,
                )
                .await
                .is_err()
                {
                    return Ok(false);
                }
            }
            if !update_target(
                state,
                group_id,
                actor_id,
                owner,
                json!({"last_delivery_reconcile_attempts":1}),
            )? {
                return Ok(false);
            }
            submission = state
                .browser_surfaces
                .refresh_optimistic_submission(surface_key(), target_url, &prompt, &submission)
                .await;
            if submission["submitted"] != true {
                update_target(
                    state,
                    group_id,
                    actor_id,
                    owner,
                    json!({
                        "last_submission_evidence":submission,
                        "last_error":"The optimistic browser echo remained unverified after one bounded refresh; the message will not be submitted again."
                    }),
                )?;
                return Ok(false);
            }
        }
    }
    let Some(submission_evidence) = stored_verified_submission_evidence(&submission) else {
        return Ok(false);
    };
    let observed_url = submission["observed"]["url"].as_str().unwrap_or("");
    let conversation_url = conversation_url_for_target(target_url, observed_url);
    if let Some(seed) = &bootstrap_seed {
        mark_bootstrap_seed_delivered(
            state,
            group_id,
            actor_id,
            owner,
            conversation_url.as_deref().unwrap_or(target_url),
            seed,
        )?;
    }
    if target["kind"] == "new_chat"
        && let Some(conversation_url) = &conversation_url
    {
        bind_new_chat_target(state, group_id, actor_id, owner, conversation_url)?;
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
        owner,
        json!({
            "last_delivery_status":if pending_new_chat_bind {"pending_new_chat_bind"} else {"submitted"},
            "last_delivery_at":cccc_contracts::utc_now(),
            "last_submission_evidence":recovered_submission,
            "last_error":if pending_new_chat_bind {"conversation_url_pending"} else {""}
        }),
    )?;
    record_delivery(
        state,
        group_id,
        actor_id,
        turn_id,
        target["last_delivery_event_ids"].clone(),
        delivery_id,
        "submitted",
        submission_evidence,
        json!({"target_url":target_url,"recovered_from":"submission_ambiguous"}),
    )
    .await?;
    record_connector(state, group_id, actor_id, owner, "submitted", turn_id, "")?;
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
    owner: &BrowserTargetOwner,
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
            owner,
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
            owner,
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
    if !update_target(
        state,
        group_id,
        actor_id,
        owner,
        json!({
            "last_delivery_id":delivery_id,
            "last_delivery_turn_id":turn_id,
            "last_delivery_event_ids":event_ids,
            "last_delivery_status":"legacy_recovery_submitting",
            "last_delivery_started_at":cccc_contracts::utc_now(),
            "last_error":""
        }),
    )? {
        return Ok(DeliveryOutcome::Idle);
    }
    let target_owner = owner.for_delivery(&delivery_id);
    let owner = &target_owner;
    let browser = match state
        .browser_surfaces
        .submit_prompt_with_attachment_before_dispatch(
            session_key,
            target_url,
            &browser_prompt,
            attachment.as_deref(),
            &delivery_id,
            || {
                acquire_dispatch_permit(
                    state,
                    group_id,
                    actor_id,
                    owner,
                    "legacy_recovery_submitting",
                )
            },
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
                owner,
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
                owner,
                json!({
                    "last_delivery_status":"submission_ambiguous",
                    "last_submission_evidence":browser,
                    "last_error":message,
                    "last_delivery_at":cccc_contracts::utc_now()
                }),
            )?;
            record_connector(
                state,
                group_id,
                actor_id,
                owner,
                "ambiguous",
                turn_id,
                message,
            )?;
            return Ok(DeliveryOutcome::Ambiguous);
        }
        Err(error) => {
            update_target(
                state,
                group_id,
                actor_id,
                owner,
                json!({"last_delivery_status":"failed","last_error":error.to_string()}),
            )?;
            return Err(ApiError::unavailable(
                "web_model_legacy_recovery_failed",
                error.to_string(),
            ));
        }
    };
    if let Some(seed) = &bootstrap_seed {
        mark_bootstrap_seed_delivered(state, group_id, actor_id, owner, target_url, seed)?;
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
        bind_new_chat_target(state, group_id, actor_id, owner, &conversation_url)?;
    }
    update_target(
        state,
        group_id,
        actor_id,
        owner,
        json!({
            "last_delivery_status":if pending {"pending_new_chat_bind"} else {"submitted"},
            "last_delivery_at":cccc_contracts::utc_now(),
            "last_submission_evidence":browser,
            "last_error":if pending {"conversation_url_pending"} else {""}
        }),
    )?;
    record_connector(state, group_id, actor_id, owner, "submitted", turn_id, "")?;
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
    owner: &BrowserTargetOwner,
    target_url: &str,
    seed: &BootstrapSeed,
) -> Result<(), ApiError> {
    update_target(
        state,
        group_id,
        actor_id,
        owner,
        json!({
            "bootstrap_seed_delivered_at":cccc_contracts::utc_now(),
            "bootstrap_seed_version":BOOTSTRAP_SEED_VERSION,
            "bootstrap_seed_digest":seed.digest,
            "bootstrap_seed_conversation_url":target_url
        }),
    )
    .map(|_| ())
}

async fn resolve_pending_new_chat(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    owner: &BrowserTargetOwner,
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
            owner,
            json!({"last_delivery_status":"pending_new_chat_bind","last_error":"conversation_url_pending"}),
        )?;
        return Ok(DeliveryOutcome::Ambiguous);
    };
    bind_new_chat_target(state, group_id, actor_id, owner, &conversation_url)?;
    update_target(
        state,
        group_id,
        actor_id,
        owner,
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
        .await?;
    }
    Ok(DeliveryOutcome::Submitted)
}

fn bind_new_chat_target(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    owner: &BrowserTargetOwner,
    conversation_url: &str,
) -> Result<(), ApiError> {
    let now = cccc_contracts::utc_now();
    update_target(
        state,
        group_id,
        actor_id,
        owner,
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
    .map(|_| ())
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    value[key]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad(format!("runtime turn missing {key}")))
}

#[cfg(test)]
mod retry_integration_tests {
    use super::super::web_model_delivery_state::target as load_target;
    use super::*;
    use cccc_core::{GroupStore, HomeLayout, ledger, web_model_connectors};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::{Duration, timeout};

    async fn web_group(state: &AppState, title: &str) -> String {
        let call =
            |op, args: Value| daemon_call(state, op, args.as_object().cloned().expect("args"));
        let created = call("group_create", json!({"title":title}))
            .await
            .expect("group");
        let gid = created["group"]["group_id"].as_str().expect("group id");
        call(
            "actor_add",
            json!({
                "group_id":gid,"actor_id":"web","runtime":"web_model","by":"user",
                "env":{"CCCC_WEB_MODEL_DELIVERY_MODE":"browser"}
            }),
        )
        .await
        .expect("web member");
        call(
            "actor_start",
            json!({"group_id":gid,"actor_id":"web","by":"user"}),
        )
        .await
        .expect("start member");
        gid.to_owned()
    }

    async fn wait_for_original_receipt(state: &AppState, gid: &str, id: &str) {
        let store = GroupStore::new(state.home.clone()).expect("store");
        timeout(Duration::from_secs(12), async {
            loop {
                let operation = state.browser_surfaces.web_model_operation.lock().await;
                let events =
                    ledger::read_all(&store.ledger_path(gid).expect("ledger")).expect("events");
                let accepted = events
                    .iter()
                    .filter(|event| {
                        event.kind == "runtime.delivery"
                            && event.data["source_event_id"] == id
                            && event.data["state"] == "accepted"
                    })
                    .count();
                assert!(accepted <= 1, "duplicate receipt for {id}");
                if accepted == 1 {
                    break;
                }
                drop(operation);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("original report has one durable receipt");
    }

    /// Shared isolated home and daemon; tests keep their own HTTP routes and assertions.
    struct BrowserHarness {
        _chrome_guard: tokio::sync::OwnedMutexGuard<()>,
        api: axum::Router,
        state: AppState,
        home: HomeLayout,
        browser: Arc<crate::browser_surface::BrowserSurfaces>,
        daemon: tokio::task::JoinHandle<anyhow::Result<()>>,
        shutdown: tokio::sync::broadcast::Sender<()>,
        _temp: tempfile::TempDir,
    }

    async fn browser_harness(label: &str, poll: Duration) -> BrowserHarness {
        let chrome_guard = crate::browser_surface::t05_chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("isolated test home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let (api, _, browser, state) = crate::app_with_shutdown(
            home.clone(),
            shutdown.clone(),
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            label.into(),
        );
        let daemon_home = home.clone();
        let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        for _ in 0..100 {
            if daemon_call(&state, "ping", Default::default())
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(poll).await;
        }
        BrowserHarness {
            _chrome_guard: chrome_guard,
            api,
            state,
            home,
            browser,
            daemon,
            shutdown,
            _temp: temp,
        }
    }

    impl BrowserHarness {
        fn profile(&self) -> std::path::PathBuf {
            self._temp.path().join("browser")
        }
    }

    async fn finish_browser_test(
        harness: BrowserHarness,
        servers: Vec<tokio::task::JoinHandle<std::io::Result<()>>>,
    ) {
        let _ = harness.shutdown.send(());
        let _ = harness.browser.close(surface_key()).await;
        let _ = daemon_call(&harness.state, "shutdown", Default::default()).await;
        let _ = timeout(Duration::from_secs(5), harness.daemon).await;
        for server in servers {
            server.abort();
            let _ = server.await;
        }
    }

    #[test]
    fn rejected_session_guard_cannot_release_the_current_owner() {
        for registry in [&WORKERS, &IN_FLIGHT] {
            let key = format!("guard-contention-{}", uuid::Uuid::new_v4());
            let held = SessionGuard::acquire(registry, key.clone()).expect("first owner");
            for _ in 0..10 {
                assert!(
                    SessionGuard::acquire(registry, key.clone()).is_none(),
                    "rejected acquisition erased the live owner's registration"
                );
                assert!(
                    registry
                        .get()
                        .expect("registry")
                        .lock()
                        .expect("lock")
                        .contains(&key),
                    "a rejected contender released another worker's guard"
                );
            }
            drop(held);
            let next = SessionGuard::acquire(registry, key).expect("owner released normally");
            drop(next);
        }
    }

    // Native router and middleware, without a second TCP server for health reads.
    async fn health_action(api: &axum::Router, gid: &str) -> Value {
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let request = axum::http::Request::builder()
            .uri(format!(
                "/api/v1/web-model/browser-session?group_id={gid}&actor_id=web"
            ))
            .header(
                axum::http::header::AUTHORIZATION,
                "Bearer t05-health-fixture",
            )
            .body(axum::body::Body::empty())
            .expect("health request");
        let response = api.clone().oneshot(request).await.expect("health route");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("health body")
            .to_bytes();
        serde_json::from_slice::<Value>(&bytes).expect("health JSON")
            ["result"]["health_snapshot"]["next_action"]["recommended"].clone()
    }

    async fn queue_mail(state: &AppState, gid: &str, text: &str) -> String {
        let source = daemon_call(
            state,
            "send",
            json!({
                "group_id":gid,"by":"user","to":["web"],"text":text,"message_mode":"mail"
            })
            .as_object()
            .cloned()
            .expect("report args"),
        )
        .await
        .expect("mail");
        let id = source["event"]["id"]
            .as_str()
            .expect("original event ID")
            .to_owned();
        daemon_call(
            state,
            "message_deliver",
            json!({
                "group_id":gid,"by":"user","source_event_id":id,"actor_ids":["web"]
            })
            .as_object()
            .cloned()
            .expect("promotion args"),
        )
        .await
        .expect("promote original");
        id
    }

    #[tokio::test]
    async fn on_demand_browser_reopens_original_reports_after_shared_cooldown() {
        assert!(
            crate::system_browser_path().is_some(),
            "real Chrome required"
        );
        let harness = browser_harness("on-demand", Duration::from_millis(10)).await;
        let state = harness.state.clone();
        let ready = Arc::new(AtomicUsize::new(0));
        let visits = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(Vec::<String>::new()));
        let (page_ready, page_visits, sink) = (ready.clone(), visits.clone(), received.clone());
        let page = r#"<html><body><form><textarea id="prompt-textarea"></textarea><button type="button" aria-label="Send prompt" onclick="const t=document.querySelector('textarea'); const n=document.createElement('div'); n.dataset.messageAuthorRole='user'; n.textContent=t.value;document.body.append(n);fetch('/received',{method:'POST',body:t.value});t.value=''">Send</button></form>BUSY</body></html>"#;
        let app = axum::Router::new()
            .route(
                "/{group}",
                axum::routing::get(
                    move |axum::extract::Path(group): axum::extract::Path<String>| {
                        let (ready, visits) = (page_ready.clone(), page_visits.clone());
                        async move {
                            if !matches!(group.as_str(), "a" | "b") {
                                return axum::response::Html(String::new());
                            }
                            eprintln!("DELIVERY_PAGE_VISIT={group}");
                            visits.fetch_add(1, Ordering::SeqCst);
                            if group == "a" && ready.load(Ordering::SeqCst) == 2 {
                                return axum::response::Html(
                                    "<html><body>Loading</body></html>".into(),
                                );
                            }
                            axum::response::Html(page.replace(
                                "BUSY",
                                if group == "b" || ready.load(Ordering::SeqCst) != 0 {
                                    ""
                                } else {
                                    r#"<button aria-label="Stop streaming">Stop</button>"#
                                },
                            ))
                        }
                    },
                ),
            )
            .route(
                "/received",
                axum::routing::post(move |body: String| {
                    let sink = sink.clone();
                    async move {
                        sink.lock().expect("sink").push(body);
                        "ok"
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let result=futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
            let call=|op,values:Value|daemon_call(&state,op,values.as_object().cloned().expect("args"));
            let mut groups=Vec::new();
            for name in ["a","b"] {
                let gid = web_group(&state, name).await;
                web_model_connectors::save_browser_target(&state.home,&gid,"web",Some(json!({"kind":"existing_chat","url":format!("{url}/{name}")}))).expect("target");
                assert!(matches!(deliver_pending(&state,&gid,"web").await.expect("no work"),DeliveryOutcome::Idle));
                groups.push(gid);
            }
            assert_eq!(visits.load(Ordering::SeqCst),0,"enabled members alone must not open Chrome");
            let mut sources=Vec::new();
            for (i,gid) in groups.iter().enumerate() {
                let source=call("send",json!({"group_id":gid,"by":"user","to":["web"],"text":format!("REPORT_{i}"),"message_mode":"send"})).await.expect("report");
                sources.push(source["event"]["id"].as_str().expect("id").to_owned());
            }
            let (a,b)=tokio::join!(biased;
                visit_pending(&state,&groups[0],"web",true),
                visit_pending(&state,&groups[1],"web",true));
            a.expect("busy visit"); b.expect("concurrent group respects cooldown");
            assert_eq!(state.browser_surfaces.info(surface_key()).await["active"],false,"busy check must close Chrome");
            assert_eq!(visits.load(Ordering::SeqCst),1,"one navigation, no confirmation tab or extra reload");
            let deadline=super::super::web_model_browser::next_delivery_check(&state.home).expect("deadline").expect("persisted");
            assert!((deadline-chrono::Utc::now()).num_seconds()>285,"five-minute cooldown");
            let process_view=HomeLayout::from_path(state.home.root().to_path_buf()).expect("reopened home");
            assert_eq!(super::super::web_model_browser::next_delivery_check(&process_view).expect("restart"),Some(deadline));
            ready.store(1,Ordering::SeqCst); // Only the server changes; no message or UI refresh.
            for _ in 0..20 { for gid in &groups { deliver_pending(&state,gid,"web").await.expect("early tick"); } }
            assert_eq!(visits.load(Ordering::SeqCst),1,"another group or tick bypassed the account cooldown");
            assert!(received.lock().expect("sink").is_empty());
            assert_eq!(super::super::web_model_browser::next_delivery_check(&state.home).expect("unchanged"),Some(deadline));
            // Another report joins the original native batch without resetting the deadline.
            call("send",json!({"group_id":groups[0],"by":"user","to":["web"],"text":"JOINED_REPORT","message_mode":"send"})).await.expect("late report");
            deliver_pending(&state,&groups[0],"web").await.expect("new event while cooling down");
            assert_eq!(visits.load(Ordering::SeqCst),1);
            // A genuinely incomplete next page also closes, without falsely recording delivery.
            ready.store(2,Ordering::SeqCst);
            cccc_core::fs::write_json(&state.home.root().join("state/web_model_browser/_shared/delivery_check.json"),&json!({"not_before":chrono::Utc::now()-chrono::Duration::seconds(1)})).expect("first elapsed clock");
            super::super::web_model_supervisor::ensure_running_actor(&state,None).await;
            wait_for_original_receipt(&state,&groups[1],&sources[1]).await;
            // Preserve the native 30-second composer deadline, plus normal close time.
            timeout(Duration::from_secs(40),async {
                loop {
                    let guard=state.browser_surfaces.web_model_operation.lock().await;
                    let target=load_target(&state,&groups[0],"web").expect("loading status");
                    if visits.load(Ordering::SeqCst) == 3 && target["last_delivery_status"] == "deferred"
                        && state.browser_surfaces.info(surface_key()).await["active"] == false { break; }
                    drop(guard); tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }).await.expect("A rechecked after B received its report");
            assert_eq!(state.browser_surfaces.info(surface_key()).await["active"],false);
            assert_eq!(visits.load(Ordering::SeqCst),3);
            assert_eq!(received.lock().expect("sink").len(),1,"incomplete A page received a false submission");
            assert!(received.lock().expect("sink")[0].contains("REPORT_1"),"busy A starved ready B");
            ready.store(1,Ordering::SeqCst);
            // Advance the persisted clock boundary, never change the production interval.
            cccc_core::fs::write_json(&state.home.root().join("state/web_model_browser/_shared/delivery_check.json"),&json!({"not_before":chrono::Utc::now()-chrono::Duration::seconds(1)})).expect("elapsed clock");
            for (gid,id) in groups.iter().zip(&sources) {
                super::super::web_model_supervisor::ensure_running_actor(&state,Some(gid)).await;
                wait_for_original_receipt(&state,gid,id).await;
                assert_eq!(state.browser_surfaces.info(surface_key()).await["active"],false,"verified report must release the delivery browser");
            }
            assert_eq!(received.lock().expect("sink").len(),2,"each original batch sent once");
            assert!(received.lock().expect("sink")[1].contains("JOINED_REPORT"),"late report was lost");
            for (i,text) in received.lock().expect("sink").iter().enumerate() {
                let expected=1-i;
                assert!(text.contains(&format!("REPORT_{expected}")),"wrong group");
                assert!(!text.contains(&format!("REPORT_{i}")),"mixed groups");
            }
            let sent_visits=visits.load(Ordering::SeqCst);
            for gid in &groups { deliver_pending(&state,gid,"web").await.expect("no more work"); }
            assert_eq!(visits.load(Ordering::SeqCst),sent_visits,"idle browser reopened");
            assert!(super::super::web_model_browser::automation_hold(&state.home).is_none(),"automatic close must not act like a user's Stop");
            // Explicit setup remains open; only a real admitted report can take it over.
            super::super::web_model_browser::ensure_open_for_actor(&state,&groups[0],"web",800,600).await.expect("explicit setup");
            visit_pending(&state,&groups[0],"web",true).await.expect("empty manual setup");
            assert_eq!(state.browser_surfaces.info(surface_key()).await["active"],true);
            let page=state.browser_surfaces.sessions.lock().await.get(surface_key()).expect("test page").page.clone();
            page.evaluate("document.querySelector('textarea').value='HUMAN_DRAFT'").await.expect("own fixture draft");
            call("send",json!({"group_id":groups[0],"by":"user","to":["web"],"text":"AFTER_DRAFT","message_mode":"send"})).await.expect("report during setup");
            visit_pending(&state,&groups[0],"web",true).await.expect("protected draft");
            assert_eq!(state.browser_surfaces.info(surface_key()).await["active"],true,"manual draft was closed");
            assert_eq!(page.evaluate("document.querySelector('textarea').value").await.expect("draft").into_value::<String>().expect("text"),"HUMAN_DRAFT");
            page.evaluate("document.querySelector('textarea').value=''").await.expect("user clears fixture draft");
            visit_pending(&state,&groups[0],"web",true).await.expect("handoff to automatic delivery");
            assert_eq!(state.browser_surfaces.info(surface_key()).await["active"],false,"manual setup never yielded to automatic delivery");
            assert_eq!(received.lock().expect("sink").len(),3);
            let final_visits=visits.load(Ordering::SeqCst);
            super::super::web_model_browser::hold_on_restriction(&state,&json!({"submission_evidence":"not_sent_rate_limited"})).expect("rate-limit hold");
            call("send",json!({"group_id":groups[1],"by":"user","to":["web"],"text":"RETAIN_DURING_HOLD","message_mode":"send"})).await.expect("retained report");
            visit_pending(&state,&groups[1],"web",true).await.expect("held account");
            assert_eq!(visits.load(Ordering::SeqCst),final_visits,"another group's report bypassed account restriction");
            eprintln!("ON_DEMAND: no-work closed; busy/loading close; durable 300s shared gate; native supervisor resumes originals once; manual setup/draft protected then handed off; account hold blocks all groups");
        })).await;
        finish_browser_test(harness, vec![server]).await;
        result.expect("on-demand assertions");
    }

    #[tokio::test]
    async fn two_group_ten_report_draft_wait_has_one_worker_per_group() {
        assert!(
            crate::system_browser_path().is_some(),
            "real Chrome required"
        );
        let harness = browser_harness("two-group-browser", Duration::from_millis(10)).await;
        cccc_core::access_tokens::AccessTokenStore::new(harness.home.clone())
            .expect("test token store")
            .create("health test", Vec::new(), true, Some("t05-health-fixture"))
            .expect("isolated test administrator");
        let state = &harness.state;
        let browser = &harness.browser;
        let records = Arc::new(Mutex::new(Vec::<Value>::new()));
        let sink = Arc::clone(&records);
        let page = r#"<!doctype html><body>
<button type="button" style="position:fixed;left:350px;top:10px;width:130px;height:36px" onclick="document.querySelector('textarea').value=''">Clear fixture draft</button>
<form><textarea id="prompt-textarea" style="position:fixed;left:10px;top:70px;width:650px;height:120px"></textarea>
<button type="button" aria-label="Send prompt" style="position:fixed;left:10px;top:230px;width:100px;height:35px" onclick="const t=document.querySelector('textarea');if(!t.value)return;const p=t.value;t.value='';const messages=JSON.parse(localStorage.getItem(location.pathname)||'[]');messages.push(p);localStorage.setItem(location.pathname,JSON.stringify(messages));render(p);fetch('/received',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:location.pathname,prompt:p})})">Send</button></form>
<script>function render(p){const d=document.createElement('div');d.dataset.messageAuthorRole='user';d.textContent=p;d.style='margin-top:300px';document.body.append(d)}JSON.parse(localStorage.getItem(location.pathname)||'[]').forEach(render);</script></body>"#;
        let app = axum::Router::new()
            .route(
                "/{group}",
                axum::routing::get(move || async move { axum::response::Html(page) }),
            )
            .route(
                "/received",
                axum::routing::post(move |axum::Json(value): axum::Json<Value>| {
                    let sink = Arc::clone(&sink);
                    async move {
                        sink.lock().expect("records").push(value);
                        "ok"
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let result = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(timeout(
            Duration::from_secs(35), async {
                let mut groups = Vec::new();
                for label in ["a", "b"] {
                    let gid = web_group(state, label).await;
                    web_model_connectors::save_browser_target(&harness.home, &gid, "web",
                        Some(json!({"kind":"existing_chat","url":format!("{base}/{label}")}))).expect("target");
                    groups.push((label, gid));
                }
                browser.ensure_open(surface_key(), &harness.profile(), &format!("{base}/a"), 800, 600)
                    .await.expect("shared browser");
                let initial_browser = browser.info(surface_key()).await;
                browser.command(surface_key(), &json!({"t":"click","x":100,"y":110})).await.expect("focus");
                browser.command(surface_key(), &json!({"t":"text","text":"PRESERVE_GROUP_A_DRAFT"})).await.expect("draft");
                let mut sources = Vec::new();
                for round in 0..5 {
                    for (label, gid) in &groups {
                        let marker = format!("ROUTED_{label}_{round}");
                        let id = queue_mail(state, gid, &marker).await;
                        sources.push((gid.clone(), id, marker));
                    }
                }
                for _ in 0..10 {
                    for (_, gid) in &groups { deliver_pending(state, gid, "web").await.expect("contended visit"); }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                let store = GroupStore::new(harness.home.clone()).expect("store");
                let read_events = |gid: &str| ledger::read_all(&store.ledger_path(gid).expect("path")).expect("ledger");
                for (_, gid) in &groups {
                    let events = read_events(gid);
                    assert!(!events.iter().any(|e| e.kind == "web_model.browser_delivery.submitting"), "crossed Send: {gid}");
                    for (_, id, _) in sources.iter().filter(|(g, _, _)| g == gid) {
                        assert_eq!(events.iter().filter(|e| e.kind == "runtime.delivery"
                            && e.data["source_event_id"] == *id && e.data["state"] == "claimed").count(), 1,
                            "reclaimed original: {id}");
                    }
                }
                assert!(records.lock().expect("records").is_empty(), "sent through A's draft");
                assert_eq!(health_action(&harness.api, &groups[0].1).await, "resolve_draft");
                assert_eq!(browser.info(surface_key()).await["url"], format!("{base}/a"));
                browser.command(surface_key(), &json!({"t":"click","x":390,"y":28})).await.expect("clear draft");
                for (_, gid) in &groups { deliver_pending(state, gid, "web").await.expect("resume batch"); }
                for (gid, id, _) in &sources { wait_for_original_receipt(state, gid, id).await; }
                timeout(Duration::from_secs(12), async {
                    while records.lock().expect("records").len() < 2 { tokio::time::sleep(Duration::from_millis(20)).await; }
                }).await.expect("server received both batches");
                let received = records.lock().expect("records").clone();
                assert_eq!(received.len(), 2);
                for (label, gid) in &groups {
                    let own = received.iter().find(|r| r["path"] == format!("/{label}")).expect("correct target");
                    let text = own["prompt"].as_str().expect("prompt");
                    for (source_gid, id, marker) in &sources {
                        assert_eq!(text.contains(marker), source_gid == gid, "cross-group marker: {marker}");
                        if source_gid == gid { assert!(text.contains(id), "missing original ID: {id}"); }
                    }
                    let unread = daemon_call(state, "inbox_peek", json!({"group_id":gid,"actor_id":"web","by":"web"})
                        .as_object().cloned().expect("inbox args")).await.expect("unread Mail");
                    assert_eq!(unread["messages"].as_array().expect("messages").len(), 5);
                }
                for _ in 0..10 {
                    for (_, gid) in &groups { deliver_pending(state, gid, "web").await.expect("duplicate visit"); }
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
                assert_eq!(records.lock().expect("records").len(), 2, "batch repeated");
                assert_eq!(browser.info(surface_key()).await["started_at"], initial_browser["started_at"]);
                // A later report must recover from a foreign archived page without a new wake event.
                let (_, gid) = &groups[1];
                let page = browser.sessions.lock().await.get(surface_key()).expect("page").page.clone();
                page.evaluate("document.body.insertAdjacentHTML('beforeend','<button id=busy aria-label=\"Stop streaming\">Stop</button>')").await.expect("busy page");
                let id = queue_mail(state, gid, "AFTER_FOREIGN_ARCHIVE").await;
                deliver_pending(state, gid, "web").await.expect("defer later report");
                timeout(Duration::from_secs(8), async {
                    loop {
                        let target = load_target(state, gid, "web").expect("target");
                        if target["last_delivery_status"] == "deferred"
                            && target["last_delivery_event_ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == &id)) { break; }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                }).await.expect("original report deferred");
                assert_eq!(health_action(&harness.api, gid).await, "wait_for_reply");
                page.evaluate("history.replaceState({},'', '/archived');document.body.innerHTML='<main><p>This conversation is archived</p><button>Unarchive</button></main>'").await.expect("foreign archive");
                for (_, group) in &groups { deliver_pending(state, group, "web").await.expect("resume original"); }
                wait_for_original_receipt(state, gid, &id).await;
                timeout(Duration::from_secs(12), async {
                    while records.lock().expect("records").len() < 3 { tokio::time::sleep(Duration::from_millis(20)).await; }
                }).await.expect("archive recovery received");
                let received = records.lock().expect("records");
                assert_eq!(received.len(), 3, "duplicate later report");
                assert_eq!(received[2]["path"], "/b");
                assert!(received[2]["prompt"].as_str().expect("prompt").contains(&id));
                let events = read_events(gid);
                let transitions = events.iter().filter(|e| e.kind == "runtime.delivery" && e.data["source_event_id"] == id)
                    .filter_map(|e| e.data["state"].as_str()).collect::<Vec<_>>();
                assert_eq!(transitions, ["claimed", "failed", "claimed", "accepted"]);
            }
        ))).await;
        finish_browser_test(harness, vec![server]).await;
        result
            .expect("two-group assertions")
            .expect("bounded two-group flow");
    }

    #[tokio::test]
    async fn composer_failure_retries_original_report_without_a_false_receipt() {
        assert!(
            crate::system_browser_path().is_some(),
            "real Chrome required"
        );
        let harness = browser_harness("presend", Duration::from_millis(20)).await;
        let state = harness.state.clone();
        let home = harness.home.clone();
        let browser = Arc::clone(&harness.browser);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let url = format!("http://{}/", listener.local_addr().expect("address"));
        let server_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let page_ready = Arc::clone(&server_ready);
        let app = axum::Router::new().route("/", axum::routing::get(move || {
            let ready = Arc::clone(&page_ready);
            async move { axum::response::Html(if ready.load(Ordering::SeqCst) {
                r#"<!doctype html><html><body><section data-testid="conversation-turn-1" data-turn-id="done"><div data-message-author-role="assistant" data-message-id="done">Finished answer</div><button data-testid="copy-turn-action-button">Copy</button></section><textarea id="prompt-textarea" placeholder="Message"></textarea><button data-testid="send-button" type="button">Send</button><script>globalThis.sends=0;document.querySelector('[data-testid="send-button"]').onclick=()=>{sends++;let n=document.createElement('div');n.dataset.messageAuthorRole='user';n.textContent=document.querySelector('textarea').value;document.body.append(n);document.querySelector('textarea').value=''}</script></body></html>"#
            } else { "<!doctype html><html><body>Loading conversation</body></html>" }) }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let operation = async {
            let call =
                |op, args: Value| daemon_call(&state, op, args.as_object().cloned().expect("args"));
            let group_id = web_group(&state, "pre-send recovery").await;
            let gid = group_id.as_str();
            web_model_connectors::save_browser_target(
                &home,
                gid,
                "web",
                Some(json!({"kind":"existing_chat","url":url})),
            )
            .expect("target");
            let report = call("send",json!({"group_id":gid,"by":"user","to":["web"],"text":"ORIGINAL_PRESEND_REPORT","message_mode":"mail"})).await.expect("report");
            let id = report["event"]["id"].as_str().expect("id");
            call(
                "message_deliver",
                json!({"group_id":gid,"by":"user","source_event_id":id,"actor_ids":["web"]}),
            )
            .await
            .expect("promote");
            browser
                .ensure_open(surface_key(), &harness.profile(), &url, 800, 600)
                .await
                .expect("chrome");
            let first = deliver_pending(&state, gid, "web")
                .await
                .expect("composer failure");
            assert!(
                matches!(first, DeliveryOutcome::Idle),
                "missing composer must remain retryable"
            );
            let target = load_target(&state, gid, "web").expect("state");
            assert_eq!(
                target["last_submission_evidence"]["submission_evidence"],
                "not_sent_composer_unavailable"
            );
            let status = call("ledger_statuses", json!({"group_id":gid,"event_ids":[id]}))
                .await
                .expect("status");
            assert_eq!(
                status["statuses"][id]["obligation_status"]["web"]["delivery_state"], "failed",
                "a budget check retained a claim and disabled manual retry"
            );
            let store = GroupStore::new(home.clone()).expect("store");
            let ledger_path = store.ledger_path(gid).expect("path");
            let events = ledger::read_all(&ledger_path).expect("events");
            assert!(
                !events.iter().any(|e| e.kind == "runtime.turn.completed"
                    || e.kind == "web_model.browser_delivery.submitting"),
                "preparation crossed the send fence"
            );
            let page = browser
                .sessions
                .lock()
                .await
                .get(surface_key())
                .expect("test page")
                .page
                .clone();
            server_ready.store(true, Ordering::SeqCst);
            // This fixture is explicitly opened by the user, so automation must
            // not replace it. Automatic cold-page recovery is tested separately.
            browser
                .command(surface_key(), &json!({"t":"navigate","url":url}))
                .await
                .expect("user restores own page");
            assert!(matches!(
                deliver_pending(&state, gid, "web")
                    .await
                    .expect("recovered input"),
                DeliveryOutcome::Submitted
            ));
            assert!(matches!(
                deliver_pending(&state, gid, "web")
                    .await
                    .expect("duplicate poll"),
                DeliveryOutcome::Idle
            ));
            assert_eq!(
                page.evaluate("globalThis.sends")
                    .await
                    .expect("counter")
                    .into_value::<u64>()
                    .expect("count"),
                1
            );
            let events = ledger::read_all(&ledger_path).expect("final events");
            assert_eq!(
                events.iter().filter(|e| e.kind == "chat.message").count(),
                1,
                "report was replaced"
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e.kind == "runtime.delivery"
                        && e.data["source_event_id"] == id
                        && e.data["state"] == "accepted")
                    .count(),
                1
            );
            let report2 = call(
                "send",
                json!({"group_id":gid,"by":"user","to":["web"],
                "text":"RESTART_PREPARING","message_mode":"send"}),
            )
            .await
            .expect("restart report");
            let wait = call(
                "runtime_wait_next_turn",
                Value::Object(browser_wait_args(gid, "web")),
            )
            .await
            .expect("reserve");
            assert_eq!(wait["status"], "work_available");
            let turn = &wait["turn"];
            update_target(&state,gid,"web", &snapshot(&state,gid,"web").expect("target owner").1, json!({"last_delivery_status":"preparing",
                "last_delivery_turn_id":turn["turn_id"],"last_delivery_event_ids":turn["event_ids"],
                "last_delivery_id":browser_delivery_id("web",turn["turn_id"].as_str().expect("turn")),
                "last_submission_evidence":null})).expect("persist pre-send interruption");
            assert!(matches!(
                deliver_pending(&state, gid, "web").await.expect("release"),
                DeliveryOutcome::Deferred
            ));
            assert!(matches!(
                deliver_pending(&state, gid, "web").await.expect("retry"),
                DeliveryOutcome::Submitted
            ));
            assert_eq!(
                load_target(&state, gid, "web").expect("target")["last_delivery_event_ids"],
                json!([report2["event"]["id"]])
            );
            page.evaluate("document.querySelector('[data-testid=send-button]').onclick=()=>{sends++;document.querySelector('textarea').value=''}")
                .await.expect("simulate missing acknowledgement after actual click");
            call(
                "send",
                json!({"group_id":gid,"by":"user","to":["web"],
                "text":"CLICK_WITHOUT_RECEIPT","message_mode":"send"}),
            )
            .await
            .expect("uncertain report");
            assert!(matches!(
                deliver_pending(&state, gid, "web").await.expect("send"),
                DeliveryOutcome::Ambiguous
            ));
            let _ = deliver_pending(&state, gid, "web")
                .await
                .expect("duplicate poll");
            assert_eq!(
                page.evaluate("globalThis.sends")
                    .await
                    .expect("counter")
                    .into_value::<u64>()
                    .expect("count"),
                3
            );
            assert_eq!(
                load_target(&state, gid, "web").expect("target")["last_delivery_status"],
                "submission_ambiguous"
            );
            page.evaluate(r#"document.querySelector('[data-testid=send-button]').onclick=()=>{sends++;const t=document.querySelector('textarea');const s=document.createElement('section');s.dataset.testid='conversation-turn-pending';s.dataset.turnId='request-still-present';const n=document.createElement('div');n.dataset.messageAuthorRole='user';n.textContent=t.value;s.append(n);document.body.append(s);t.value=''}"#)
                .await.expect("late server receipt fixture");
            let late = call(
                "send",
                json!({"group_id":gid,"by":"user","to":["web"],
                "text":"LATE_SERVER_RECEIPT","message_mode":"send"}),
            )
            .await
            .expect("late report");
            assert!(matches!(
                deliver_pending(&state, gid, "web")
                    .await
                    .expect("provisional send"),
                DeliveryOutcome::Ambiguous
            ));
            assert!(matches!(
                deliver_pending(&state, gid, "web")
                    .await
                    .expect("still unconfirmed"),
                DeliveryOutcome::Idle
            ));
            page.evaluate(r#"const answer=document.createElement('div');answer.dataset.messageAuthorRole='assistant';answer.dataset.messageId='server-late-answer';answer.textContent='Received';document.querySelector('[data-turn-id=request-still-present]').append(answer)"#)
                .await.expect("server responds without changing the container id");
            assert!(matches!(
                deliver_pending(&state, gid, "web")
                    .await
                    .expect("late receipt"),
                DeliveryOutcome::Submitted
            ));
            let _ = deliver_pending(&state, gid, "web")
                .await
                .expect("duplicate receipt check");
            assert_eq!(
                page.evaluate("globalThis.sends")
                    .await
                    .expect("count")
                    .into_value::<u64>()
                    .expect("number"),
                4
            );
            let events = ledger::read_all(&ledger_path).expect("late receipt ledger");
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e.kind == "runtime.delivery"
                        && e.data["source_event_id"] == late["event"]["id"]
                        && e.data["state"] == "accepted")
                    .count(),
                1,
                "late verified receipt must settle the original source exactly once"
            );
        };
        let caught = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(timeout(
            Duration::from_secs(60),
            operation,
        )))
        .await;
        finish_browser_test(harness, vec![server]).await;
        caught
            .expect("pre-send assertions")
            .expect("bounded pre-send flow");
    }
    #[derive(Clone, Copy, PartialEq)]
    enum BindingRace {
        BeforeSend,
        ContentEditable,
        EditedContentEditable,
        EditedDraft,
        AfterSend,
        AfterUnverifiedSend,
    }

    #[tokio::test]
    async fn rebinding_cancels_the_old_inflight_delivery() {
        binding_race(BindingRace::BeforeSend).await;
    }

    #[tokio::test]
    async fn rebinding_preserves_a_user_edited_draft() {
        binding_race(BindingRace::EditedDraft).await;
    }

    #[tokio::test]
    async fn rebinding_after_send_preserves_the_receipt_without_overwriting_the_new_target() {
        binding_race(BindingRace::AfterSend).await;
    }

    #[tokio::test]
    async fn rebinding_after_send_preserves_an_uncertain_receipt_without_retrying() {
        binding_race(BindingRace::AfterUnverifiedSend).await;
    }

    #[tokio::test]
    async fn rebinding_clears_only_its_contenteditable_draft() {
        binding_race(BindingRace::ContentEditable).await;
        binding_race(BindingRace::EditedContentEditable).await;
    }

    async fn binding_race(phase: BindingRace) {
        let edited = matches!(
            phase,
            BindingRace::EditedDraft | BindingRace::EditedContentEditable
        );
        let already_sent = matches!(
            phase,
            BindingRace::AfterSend | BindingRace::AfterUnverifiedSend
        );
        assert!(
            crate::system_browser_path().is_some(),
            "real Chrome required"
        );
        let harness = browser_harness("rebind-test", Duration::from_millis(20)).await;
        let state = harness.state.clone();
        let home = harness.home.clone();
        let browser = Arc::clone(&harness.browser);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let url = format!("http://{}/", listener.local_addr().expect("addr"));
        let app = axum::Router::new().fallback(axum::routing::get(|| async {
            axum::response::Html(r#"<!doctype html><textarea id="prompt-textarea"></textarea><button data-testid="send-button" disabled>Send</button><script>
            window.sends=0;window.holdReceipt=false;
            window.draft=()=>{const c=document.querySelector('#prompt-textarea');return c.isContentEditable?c.innerText:c.value};
            window.replaceDraft=text=>{const c=document.querySelector('#prompt-textarea');if(c.isContentEditable)c.innerText=text;else c.value=text};
            window.releaseReceipt=()=>{let n=document.createElement('div');n.dataset.messageAuthorRole='user';n.textContent=draft();document.body.append(n);replaceDraft('')};
            document.querySelector('button').onclick=()=>{sends++;if(!holdReceipt)releaseReceipt()};
            if(location.pathname==='/new')document.querySelector('button').disabled=false;
            </script>"#)
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let operation = async {
            let call =
                |op, args: Value| daemon_call(&state, op, args.as_object().cloned().expect("args"));
            let group_id = web_group(&state, "rebind race").await;
            let gid = group_id.as_str();
            let (connector, _) = web_model_connectors::create(&home, gid, "web", "chatgpt", "test")
                .expect("connector");
            let cid = connector["connector_id"].as_str().expect("cid");
            let code = web_model_connectors::prepare_binding(&home, cid, 600).expect("code");
            web_model_connectors::bind_session(
                &home,
                cid,
                code["code"].as_str().expect("F1 browser fixture value"),
                "old-chat",
            )
            .expect("old binding");
            web_model_connectors::save_browser_target(
                &home,
                gid,
                "web",
                Some(json!({"kind":"existing_chat","url":url})),
            )
            .expect("target");
            let source=call("send",json!({"group_id":gid,"by":"user","to":["web"],"text":"OLD_CHAT_ONLY_REPORT","message_mode":"send"})).await.expect("source");
            browser
                .ensure_open(surface_key(), &harness.profile(), &url, 800, 600)
                .await
                .expect("browser");
            let page = browser
                .sessions
                .lock()
                .await
                .get(surface_key())
                .expect("test page")
                .page
                .clone();
            if matches!(
                phase,
                BindingRace::ContentEditable | BindingRace::EditedContentEditable
            ) {
                page.evaluate("document.querySelector('#prompt-textarea').outerHTML='<div id=prompt-textarea contenteditable=true role=textbox style=width:600px;min-height:80px></div>'")
                    .await.expect("real contenteditable composer");
            }
            if already_sent {
                page.evaluate(
                    "window.holdReceipt=true;document.querySelector('button').disabled=false",
                )
                .await
                .expect("hold receipt after Send");
            }
            let change_binding = async {
                timeout(Duration::from_secs(8), async {
                    loop {
                        let condition = if already_sent {
                            "window.sends===1"
                        } else {
                            "window.draft().includes('OLD_CHAT_ONLY_REPORT')"
                        };
                        if page
                            .evaluate(condition)
                            .await
                            .expect("browser boundary")
                            .into_value::<bool>()
                            .expect("F1 browser fixture value")
                        {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("browser reached binding boundary");
                if edited {
                    page.evaluate("window.replaceDraft('My changed unsent draft')")
                        .await
                        .expect("human edits the draft");
                }
                let old_snapshot = snapshot(&state, gid, "web").expect("original attempt");
                let binding_home = home.clone();
                let connector_id = cid.to_owned();
                tokio::task::spawn_blocking(move || {
                    let replacement =
                        web_model_connectors::prepare_binding(&binding_home, &connector_id, 600)
                            .expect("replacement");
                    web_model_connectors::bind_session(
                        &binding_home,
                        &connector_id,
                        replacement["code"]
                            .as_str()
                            .expect("F1 browser fixture value"),
                        "new-chat",
                    )
                    .expect("rebound");
                })
                .await
                .expect("binding thread");
                assert!(
                    web_model_connectors::find_session(&home, "old-chat")
                        .expect("F1 browser fixture value")
                        .is_none()
                );
                assert_eq!(
                    web_model_connectors::browser_target(&home, gid, "web")
                        .expect("F1 browser fixture value"),
                    json!({})
                );
                if already_sent {
                    web_model_connectors::save_browser_target(&home, gid, "web", Some(json!({
                        "kind":"existing_chat","url":format!("{url}new"),
                        "last_delivery_id":"new-binding-sentinel","last_delivery_status":"new_binding"
                    }))).expect("new target before old receipt");
                    web_model_connectors::update_connector(&home, cid, |item| {
                        item["last_call_status"] = json!("new_binding");
                    })
                    .expect("new connector activity");
                    if phase == BindingRace::AfterSend {
                        page.evaluate("window.releaseReceipt()")
                            .await
                            .expect("release old receipt");
                    }
                } else {
                    page.evaluate("document.querySelector('button').disabled=false")
                        .await
                        .expect("ready after rebind");
                }
                old_snapshot
            };
            let (delivered, (old_target, old_owner)) =
                tokio::join!(deliver_pending(&state, gid, "web"), change_binding);
            let sends = page
                .evaluate("window.sends")
                .await
                .expect("count")
                .into_value::<u64>()
                .expect("F1 browser fixture value");
            let store = GroupStore::new(home.clone()).expect("store");
            let ledger_path = store.ledger_path(gid).expect("ledger");
            let source_id = source["event"]["id"]
                .as_str()
                .expect("F1 browser fixture value");
            if already_sent {
                assert_eq!(sends, 1, "Send already started before binding changed");
                assert!(if phase == BindingRace::AfterSend {
                    matches!(delivered, Ok(DeliveryOutcome::Submitted))
                } else {
                    matches!(delivered, Ok(DeliveryOutcome::Ambiguous))
                });
                let replacement = load_target(&state, gid, "web").expect("replacement target");
                assert_eq!(replacement["last_delivery_id"], "new-binding-sentinel");
                assert_eq!(replacement["last_delivery_status"], "new_binding");
                assert_eq!(
                    web_model_connectors::load(&home)
                        .expect("F1 browser fixture value")
                        .iter()
                        .find(|item| item["connector_id"] == cid)
                        .expect("F1 browser fixture value")["last_call_status"],
                    "new_binding"
                );
                bind_new_chat_target(
                    &state,
                    gid,
                    "web",
                    &old_owner,
                    "https://chatgpt.com/c/late-old-chat",
                )
                .expect("stale URL result");
                retry_unsubmitted_turn(
                    &state,
                    gid,
                    "web",
                    DeliveryAttempt {
                        owner: &old_owner,
                        turn_id: old_target["last_delivery_turn_id"]
                            .as_str()
                            .expect("F1 browser fixture value"),
                        event_ids: old_target["last_delivery_event_ids"].clone(),
                        delivery_id: old_target["last_delivery_id"]
                            .as_str()
                            .expect("F1 browser fixture value"),
                    },
                    "late old failure",
                )
                .await
                .expect("stale failure receipt");
                assert_eq!(
                    load_target(&state, gid, "web").expect("F1 browser fixture value"),
                    replacement,
                    "late URL/failure overwrote new target"
                );
                assert!(matches!(
                    deliver_pending(&state, gid, "web")
                        .await
                        .expect("F1 browser fixture value"),
                    DeliveryOutcome::Idle
                ));
            } else {
                assert_eq!(
                    sends, 0,
                    "old chat received the report after binding was revoked"
                );
                assert!(matches!(delivered, Ok(DeliveryOutcome::Deferred)));
                assert_eq!(
                    load_target(&state, gid, "web").expect("F1 browser fixture value"),
                    json!({}),
                    "cancelled attempt revived old target"
                );

                let draft = page
                    .evaluate("window.draft()")
                    .await
                    .expect("F1 browser fixture value")
                    .into_value::<String>()
                    .expect("F1 browser fixture value");
                if edited {
                    assert_eq!(draft, "My changed unsent draft");
                } else {
                    assert!(
                        browser
                            .relay_surface_idle(surface_key())
                            .await
                            .expect("composer state"),
                        "cancelled program draft still blocks the shared browser"
                    );
                }
                let events = ledger::read_all(&ledger_path).expect("cancelled ledger");
                assert!(
                    events.iter().any(|event| event.kind == "runtime.delivery"
                        && event.data["source_event_id"] == source_id
                        && event.data["state"] == "failed"),
                    "original report was not released for retry"
                );
                if edited {
                    page.evaluate("window.replaceDraft('')")
                        .await
                        .expect("human clears own draft");
                }
                web_model_connectors::save_browser_target(
                    &home,
                    gid,
                    "web",
                    Some(json!({"kind":"existing_chat","url":format!("{url}new")})),
                )
                .expect("select new conversation");
                assert!(matches!(
                    deliver_pending(&state, gid, "web")
                        .await
                        .expect("retry original report"),
                    DeliveryOutcome::Submitted
                ));
                assert_eq!(
                    page.evaluate("window.sends")
                        .await
                        .expect("F1 browser fixture value")
                        .into_value::<u64>()
                        .expect("F1 browser fixture value"),
                    1
                );
                assert!(matches!(
                    deliver_pending(&state, gid, "web")
                        .await
                        .expect("F1 browser fixture value"),
                    DeliveryOutcome::Idle
                ));
            }
            let events = ledger::read_all(&ledger_path).expect("final ledger");
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == "chat.message")
                    .count(),
                1,
                "original report was duplicated"
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == "runtime.delivery"
                        && event.data["source_event_id"] == source_id
                        && event.data["state"]
                            == if phase == BindingRace::AfterUnverifiedSend {
                                "ambiguous"
                            } else {
                                "accepted"
                            })
                    .count(),
                1,
                "report must have one terminal handoff fact"
            );
        };
        let caught = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(timeout(
            Duration::from_secs(20),
            operation,
        )))
        .await;
        finish_browser_test(harness, vec![server]).await;
        caught
            .expect("binding race assertions")
            .expect("bounded binding race flow");
    }
}
