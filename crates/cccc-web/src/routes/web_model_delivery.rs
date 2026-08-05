use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::AppState;
use crate::api::ApiError;

use super::web_model_browser::key;
use super::web_model_delivery_completion::{args, call as daemon_call, complete_args, reconcile};
use super::web_model_delivery_state::{record_connector, target, update_target};

static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static WORKERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

pub(super) enum DeliveryOutcome {
    Submitted,
    Idle,
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
                Ok(DeliveryOutcome::Idle | DeliveryOutcome::Ambiguous) => {
                    retry_seconds = 1;
                    std::time::Duration::from_secs(5)
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
    let target = target(state, group_id, actor_id)?;
    let target_url = target["url"].as_str().unwrap_or("");
    if target_url.is_empty() && target["kind"] != "new_chat" {
        return Ok(DeliveryOutcome::Idle);
    }
    if matches!(
        target["last_delivery_status"].as_str(),
        Some("ambiguous" | "completion_conflict")
    ) {
        return Ok(if reconcile(state, group_id, actor_id, &target).await? {
            DeliveryOutcome::Submitted
        } else {
            DeliveryOutcome::Ambiguous
        });
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
    let prompt = required(turn, "coalesced_text")?;
    let delivery_id = format!("wmd_{}", &Uuid::new_v4().simple().to_string()[..16]);
    update_target(
        state,
        group_id,
        actor_id,
        json!({"last_delivery_id":delivery_id,"last_delivery_turn_id":turn_id,"last_delivery_event_ids":turn["event_ids"],"last_delivery_status":"submitting","last_delivery_started_at":cccc_contracts::utc_now(),"last_error":""}),
    )?;
    let submitted = state
        .browser_surfaces
        .submit_prompt(session_key, target_url, prompt)
        .await;
    let browser = match submitted {
        Ok(browser) => browser,
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
            json!({"last_delivery_status":"ambiguous","last_delivery_turn_id":turn_id,"last_delivery_event_ids":turn["event_ids"],"last_delivery_reconcile_attempts":0,"last_submission_evidence":browser,"last_error":error.to_string()}),
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
    update_target(
        state,
        group_id,
        actor_id,
        json!({"last_delivery_status":"submitted","last_delivery_at":cccc_contracts::utc_now(),"last_error":"","last_submission_evidence":browser}),
    )?;
    record_connector(state, group_id, actor_id, "submitted", turn_id, "")?;
    Ok(DeliveryOutcome::Submitted)
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    value[key]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad(format!("runtime turn missing {key}")))
}
