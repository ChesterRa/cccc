use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::{ActorRuntime, utc_now};
use cccc_core::GroupStore;
use cccc_core::integration_state;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io;
use std::path::{Path, PathBuf};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::browser_surface::{
    conversation_target_matches, is_chatgpt_url, normalized_chatgpt_conversation_url,
};

pub(super) const TARGETS_KEY: &str = "web_model_browser_targets";
const DELIVERY_PREFERENCES_KEY: &str = "web_model_delivery_preferences";

#[derive(Debug, Deserialize)]
struct SessionQuery {
    #[serde(default)]
    group_id: String,
    #[serde(default)]
    actor_id: String,
    #[serde(default)]
    inspect: bool,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    viewer_mode: String,
}

#[derive(Debug, Default, Deserialize)]
struct InspectQuery {
    #[serde(default)]
    inspect: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/web-model/shared-browser", get(shared_info))
        .route("/api/v1/web-model/shared-browser/open", post(shared_open))
        .route("/api/v1/web-model/shared-browser/close", post(shared_close))
        .route("/api/v1/web-model/shared-browser/ws", get(shared_upgrade))
        .route("/api/v1/web-model/browser-session", get(info))
        .route("/api/v1/web-model/browser-session/open", post(open))
        .route("/api/v1/web-model/browser-session/close", post(close))
        .route(
            "/api/v1/web-model/browser-session/bind-current",
            post(bind_current),
        )
        .route(
            "/api/v1/web-model/browser-session/delivery-preference",
            post(update_delivery_preference),
        )
        .route("/api/v1/web-model/browser-session/ws", get(upgrade))
}

async fn info(State(state): State<AppState>, Query(query): Query<SessionQuery>) -> ApiResult {
    let group_id = required_identifier(&query.group_id, "group_id")?;
    let actor_id = required_identifier(&query.actor_id, "actor_id")?;
    validate_actor(&state, group_id, actor_id)?;
    // Status inspection is read-only; delivery is owned by the background worker.
    payload(&state, group_id, actor_id, query.inspect).await
}

async fn open(
    State(state): State<AppState>,
    Query(query): Query<InspectQuery>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let width = dimension(&body, "width", 1366, 640, 2560);
    let height = dimension(&body, "height", 900, 480, 1600);
    ensure_open_for_actor(&state, &group_id, &actor_id, width, height).await?;
    super::web_model_delivery::ensure_worker(state.clone(), group_id.clone(), actor_id.clone())
        .await;
    payload(&state, &group_id, &actor_id, query.inspect).await
}

pub(super) async fn ensure_open_for_actor(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    width: u32,
    height: u32,
) -> Result<Value, ApiError> {
    let _operation = state.browser_surfaces.web_model_operation.lock().await;
    state
        .browser_surfaces
        .web_model_auto_close
        .store(false, std::sync::atomic::Ordering::Release);
    ensure_open_for_actor_locked(state, group_id, actor_id, width, height).await
}

pub(super) async fn ensure_open_for_actor_locked(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    width: u32,
    height: u32,
) -> Result<Value, ApiError> {
    validate_actor(state, group_id, actor_id)?;
    if another_chat_is_pending(state, group_id, actor_id)? {
        return Err(ApiError::bad(
            "Another group's newly submitted chat is still acquiring its conversation URL; wait for it before navigating the shared browser",
        ));
    }
    if automation_hold(&state.home).is_some() {
        return Err(ApiError::unavailable(
            "browser_automation_paused",
            "Automatic browser access is paused; use Open browser after resolving the restriction.",
        ));
    }
    let provider = super::web_model_connector_store::for_actor(state, group_id, actor_id)
        .and_then(|item| item["provider"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "chatgpt".into());
    let target = super::web_model_delivery_state::target(state, group_id, actor_id)?;
    let open_url = browser_open_url(&target, provider_url(&provider));
    let profile = browser_profile_path(state.home.root());
    let headless = use_headless_browser(
        std::env::var("CCCC_WEB_MODEL_BROWSER_HEADLESS")
            .ok()
            .as_deref(),
    );
    state
        .browser_surfaces
        .ensure_open_system(surface_key(), &profile, &open_url, width, height, headless)
        .await
        .map_err(|error| ApiError::bad(format!("{error:#}")))?;
    let session_key = surface_key();
    match target["kind"].as_str() {
        Some("existing_chat") if is_chatgpt_url(&open_url) => {
            if normalized_chatgpt_conversation_url(&open_url).is_some()
                && let Err(error) = state
                    .browser_surfaces
                    .align_chatgpt_conversation_target(
                        session_key,
                        &open_url,
                        std::time::Duration::from_secs(5),
                    )
                    .await
            {
                tracing::warn!(group_id, actor_id, %error, "saved ChatGPT conversation could not be opened");
            }
        }
        Some("existing_chat" | "new_chat")
            if state.browser_surfaces.info(session_key).await["url"] != open_url =>
        {
            if let Err(error) = state
                .browser_surfaces
                .navigate_to_url(session_key, &open_url)
                .await
            {
                tracing::warn!(group_id, actor_id, %error, "saved Web-model target could not be opened");
            }
        }
        _ => {}
    }
    Ok(state.browser_surfaces.info(session_key).await)
}

async fn close(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let _operation = state.browser_surfaces.web_model_operation.lock().await;
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    pause_and_close_browser(&state).await?;
    payload(&state, &group_id, &actor_id, false).await
}

async fn bind_current(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let _operation = state.browser_surfaces.web_model_operation.lock().await;
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let clear = body.get("clear").and_then(Value::as_bool).unwrap_or(false);
    let current = state.browser_surfaces.info(surface_key()).await;
    let mut url = body
        .get("conversation_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if url.is_empty() {
        if another_chat_is_pending(&state, &group_id, &actor_id)? {
            return Err(ApiError::bad(
                "The shared browser is still assigning another group's chat URL; paste this group's existing chat URL explicitly",
            ));
        }
        url = current["url"].as_str().unwrap_or("").to_owned();
    }
    let new_chat = body
        .get("new_chat")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let target = if clear {
        json!({})
    } else if new_chat {
        let provider = super::web_model_connector_store::for_actor(&state, &group_id, &actor_id)
            .and_then(|item| item["provider"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "chatgpt".into());
        json!({"state":"new_chat_armed","kind":"new_chat","url":provider_url(&provider),"saved_at":utc_now(),"next_delivery":"new_chat"})
    } else {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ApiError::bad("a browser conversation URL is required"));
        }
        if is_chatgpt_url(&url) {
            url = normalized_chatgpt_conversation_url(&url).ok_or_else(|| {
                ApiError::bad(
                    "ChatGPT is still assigning the final conversation URL; wait for a stable /c/... address and save again",
                )
            })?;
        }
        json!({"state":"bound_existing_chat","kind":"existing_chat","url":url,"saved_at":utc_now(),"next_delivery":"existing_chat"})
    };
    let home = state.home.clone();
    let target_group = group_id.clone();
    let target_actor = actor_id.clone();
    tokio::task::spawn_blocking(move || {
        cccc_core::web_model_connectors::save_browser_target(
            &home,
            &target_group,
            &target_actor,
            (!clear).then_some(target),
        )
    })
    .await
    .map_err(|error| ApiError::bad(error.to_string()))?
    .map_err(io_error)?;
    if !clear && current["active"].as_bool().unwrap_or(false) {
        super::web_model_delivery::ensure_worker(state.clone(), group_id.clone(), actor_id.clone())
            .await;
    }
    payload(&state, &group_id, &actor_id, false).await
}

async fn update_delivery_preference(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let mode = required(&body, "mode")?;
    let mut request = super::web_model_delivery_completion::args(&group_id, &actor_id);
    request.insert("mode".into(), json!(mode));
    request.insert("by".into(), json!("user"));
    super::web_model_delivery_completion::call(
        &state,
        "web_model_delivery_preferences_update",
        request,
    )
    .await?;
    payload(&state, &group_id, &actor_id, false).await
}

async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let group_id = required_identifier(&query.group_id, "group_id")?;
    let actor_id = required_identifier(&query.actor_id, "actor_id")?;
    validate_actor(&state, group_id, actor_id)?;
    shared_upgrade(State(state), Query(query), ws).await
}

async fn shared_upgrade(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let session_key = surface_key();
    let vnc = query.mode.trim().eq_ignore_ascii_case("vnc");
    let viewer_mode = query.viewer_mode;
    if state.web_mode.is_read_only() {
        return Ok(ws.on_upgrade(|socket| async move {
            crate::readonly::reject_socket(
                socket,
                "read_only_browser_surface",
                "Web-model browser surface is disabled in read-only mode.",
            )
            .await;
        }));
    }
    Ok(ws.on_upgrade(move |socket| async move {
        if vnc {
            crate::browser_surface::serve_vnc_socket(
                socket,
                &state.browser_surfaces,
                session_key,
                state.shutdown.subscribe(),
            )
            .await;
        } else {
            crate::browser_surface::serve_socket(
                socket,
                &state.browser_surfaces,
                session_key,
                &viewer_mode,
                state.shutdown.subscribe(),
            )
            .await;
        }
    }))
}

// One shared browser owns sign-in. These admin-only endpoints never select a
// member, bind a conversation, change a return target or enqueue a delivery.
async fn shared_info(
    State(state): State<AppState>,
    Query(query): Query<InspectQuery>,
) -> ApiResult {
    shared_payload(&state, query.inspect).await
}

// One durable account-wide deadline, not another queue. Pending messages stay
// in the native runtime ledger. New events/groups/restarts cannot bypass it.
pub(super) fn next_delivery_check(
    home: &cccc_core::HomeLayout,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, ApiError> {
    let path = home
        .root()
        .join("state/web_model_browser/_shared/delivery_check.json");
    let value: Value = match cccc_core::fs::read_json(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    if !value.is_object() || value.get("not_before").is_none() {
        return Err(ApiError::bad("invalid browser delivery deadline"));
    }
    if value["not_before"].is_null() {
        return Ok(None);
    }
    let date = value["not_before"]
        .as_str()
        .ok_or_else(|| ApiError::bad("invalid browser delivery deadline"))?;
    chrono::DateTime::parse_from_rfc3339(date)
        .map(|date| Some(date.with_timezone(&chrono::Utc)))
        .map_err(|error| ApiError::bad(error.to_string()))
}

pub(super) fn schedule_delivery_check(
    home: &cccc_core::HomeLayout,
    retry: bool,
) -> Result<(), ApiError> {
    let deadline = retry.then(|| chrono::Utc::now() + chrono::Duration::minutes(5));
    cccc_core::fs::write_json(
        &home
            .root()
            .join("state/web_model_browser/_shared/delivery_check.json"),
        &json!({"not_before":deadline}),
    )
    .map_err(io_error)
}

fn automation_hold_path(home: &cccc_core::HomeLayout) -> std::path::PathBuf {
    home.root()
        .join("state/web_model_browser/_shared/automation_hold.json")
}

// One account-wide stop, stored beside the existing shared browser state.
// Status reads, new events, another group and process restarts cannot clear it.
pub(super) fn automation_hold(home: &cccc_core::HomeLayout) -> Option<Value> {
    match cccc_core::fs::read_json(&automation_hold_path(home)) {
        Ok(value) => Some(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            Some(json!({"reason":"unreadable_automation_hold","detail":error.to_string()}))
        }
    }
}

fn pause_automation(home: &cccc_core::HomeLayout, reason: &str) -> std::io::Result<()> {
    cccc_core::fs::write_json(
        &automation_hold_path(home),
        &json!({"reason":reason,"at":cccc_contracts::utc_now()}),
    )
}

fn resume_automation(home: &cccc_core::HomeLayout) -> std::io::Result<()> {
    // Only an explicit Open with a ready page reaches this function. It can
    // clear a stale/corrupt retry deadline without allowing timer-driven retries.
    for path in [
        home.root()
            .join("state/web_model_browser/_shared/delivery_check.json"),
        automation_hold_path(home),
    ] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn hold_on_restriction(state: &AppState, evidence: &Value) -> Result<(), ApiError> {
    let reason = evidence["submission_evidence"].as_str().unwrap_or("");
    if matches!(
        reason,
        "not_sent_rate_limited"
            | "not_sent_access_denied"
            | "not_sent_verification_required"
            | "not_sent_login_required"
    ) {
        pause_automation(&state.home, reason).map_err(|e| ApiError::bad(e.to_string()))?;
    }
    Ok(())
}

async fn shared_open(
    State(state): State<AppState>,
    Query(query): Query<InspectQuery>,
    Json(body): Json<Value>,
) -> ApiResult {
    let _operation = state.browser_surfaces.web_model_operation.lock().await;
    state
        .browser_surfaces
        .web_model_auto_close
        .store(false, std::sync::atomic::Ordering::Release);
    let width = dimension(&body, "width", 1366, 640, 2560);
    let height = dimension(&body, "height", 900, 480, 1600);
    let headless = use_headless_browser(
        std::env::var("CCCC_WEB_MODEL_BROWSER_HEADLESS")
            .ok()
            .as_deref(),
    );
    state
        .browser_surfaces
        .ensure_open_system(
            surface_key(),
            &browser_profile_path(state.home.root()),
            "https://chatgpt.com/",
            width,
            height,
            headless,
        )
        .await
        .map_err(|error| ApiError::bad(format!("{error:#}")))?;
    // Explicit user Open can release the hold only after visible readiness.
    let readiness = state
        .browser_surfaces
        .prompt_readiness(surface_key())
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    if readiness["ready"] == true {
        resume_automation(&state.home).map_err(|e| ApiError::bad(e.to_string()))?;
    }
    // ensure_open_system reuses an existing page without navigating it.
    shared_payload(&state, query.inspect).await
}

// Both explicit close endpoints hold web_model_operation before entering here.
async fn pause_and_close_browser(state: &AppState) -> Result<(), ApiError> {
    pause_automation(&state.home, "user_closed_browser").map_err(io_error)?;
    state
        .browser_surfaces
        .close(surface_key())
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(())
}

async fn shared_close(State(state): State<AppState>) -> ApiResult {
    let _operation = state.browser_surfaces.web_model_operation.lock().await;
    pause_and_close_browser(&state).await?;
    shared_payload(&state, false).await
}

async fn shared_payload(state: &AppState, inspect: bool) -> ApiResult {
    let (surface, readiness) = observe_browser(state, inspect).await;
    let hold = automation_hold(&state.home);
    let meta = &surface["metadata"];
    let browser = json!({"scope":"shared","active":surface["active"],"ready":hold.is_none() && readiness["ready"] == true,
        "automation_paused":hold.is_some(), "automation_hold":hold,
        "login_required":readiness["login_required"],"message":readiness["message"],
        "tab_url":readiness["tab_url"],"pid":meta["pid"],"cdp_port":meta["cdp_port"],
        "visibility":meta["visibility"],"profile_dir":meta["profile_dir"],"started_at":surface["started_at"]});
    Ok(success(
        json!({"browser_session":browser,"browser_surface":surface}),
    ))
}

async fn observe_browser(state: &AppState, inspect: bool) -> (Value, Value) {
    let session_key = surface_key();
    let mut surface = state.browser_surfaces.info(session_key).await;
    let active = surface["active"].as_bool().unwrap_or(false);
    let readiness = if active && inspect {
        let readiness = state
            .browser_surfaces
            .prompt_readiness(session_key)
            .await
            .unwrap_or_else(|error| {
                json!({
                    "ready":false,
                    "login_required":false,
                    "tab_url":surface["url"],
                    "message":error.to_string()
                })
            });
        surface = state.browser_surfaces.info(session_key).await;
        readiness
    } else if active {
        cached_readiness(&surface)
    } else {
        json!({"ready":false,"login_required":false,"tab_url":surface["url"]})
    };
    (surface, readiness)
}

async fn payload(state: &AppState, group_id: &str, actor_id: &str, inspect: bool) -> ApiResult {
    let (surface, readiness) = observe_browser(state, inspect).await;
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    let mut target = super::web_model_delivery_state::target(state, group_id, actor_id)?;
    if let Some(target) = target.as_object_mut() {
        target.remove("bound_session_hash");
    }
    let preferences = integration_state::group_get(&store, group_id, DELIVERY_PREFERENCES_KEY)
        .map_err(io_error)?;
    let stored_preference = preferences
        .get(actor_id)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let delivery_mode = match stored_preference["mode"].as_str() {
        Some("image_compat") => "image_compat",
        _ => "standard",
    };
    let delivery_preference = json!({
        "mode":delivery_mode,
        "updated_at":stored_preference["updated_at"].as_str().unwrap_or(""),
        "updated_by":stored_preference["updated_by"].as_str().unwrap_or("")
    });
    let active = surface["active"].as_bool().unwrap_or(false);
    let metadata = surface
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let ready = readiness["ready"].as_bool().unwrap_or(false);
    let login_required = readiness["login_required"].as_bool().unwrap_or(false);
    let url = readiness["tab_url"]
        .as_str()
        .or_else(|| surface["url"].as_str())
        .unwrap_or("");
    let kind = target["kind"].as_str().unwrap_or("").to_owned();
    let stored_target_url = target["url"].as_str().unwrap_or("").to_owned();
    let chatgpt_existing = kind == "existing_chat" && is_chatgpt_url(&stored_target_url);
    let normalized_target_url = chatgpt_existing
        .then(|| normalized_chatgpt_conversation_url(&stored_target_url))
        .flatten();
    let invalid_target = chatgpt_existing && normalized_target_url.is_none();
    let target_mismatch = active
        && normalized_target_url
            .as_deref()
            .is_some_and(|expected| !conversation_target_matches(expected, url));
    let conversation_url = if kind == "existing_chat" {
        if chatgpt_existing {
            normalized_target_url.clone().unwrap_or_default()
        } else {
            stored_target_url.clone()
        }
    } else {
        String::new()
    };
    let pending_new_chat_bind = kind == "new_chat";
    if invalid_target {
        target["state"] = json!("invalid_existing_chat");
        target["kind"] = json!("none");
        target["next_delivery"] = json!("blocked");
        target["label"] = json!("Rebind ChatGPT chat");
        target["detail"] =
            json!("The saved ChatGPT URL is provisional or invalid and cannot receive deliveries.");
    } else if target_mismatch {
        target["state"] = json!("existing_chat_unavailable");
        target["next_delivery"] = json!("blocked");
        target["label"] = json!("Saved chat unavailable");
        target["detail"] = json!(
            "The live ChatGPT page does not match the saved conversation; delivery is blocked until it is reopened or rebound."
        );
    }
    let internal_delivery_status = target["last_delivery_status"].as_str().unwrap_or("");
    let delivery_status = match internal_delivery_status {
        "pending_new_chat_bind" => "pending",
        "submission_ambiguous"
        | "submission_ambiguous_completion_pending"
        | "completion_ambiguous"
        | "legacy_submission_unverified" => "ambiguous",
        "deferred" | "legacy_recovery_submitting" => "submitting",
        value => value,
    };
    let submission = &target["last_submission_evidence"];
    let submission_evidence = submission["submission_evidence"]
        .as_str()
        .or_else(|| submission.as_str())
        .unwrap_or("");
    let waiting =
        (internal_delivery_status == "deferred").then(|| deferred_action(submission_evidence));
    let send_selector = submission["send_selector"].as_str().unwrap_or("");
    let pending_new_chat_last_tab_url = submission["tab_url"]
        .as_str()
        .or_else(|| submission["observed"]["url"].as_str())
        .unwrap_or("");
    let last_error = target["last_error"].as_str().unwrap_or("");
    let delivery_state = match internal_delivery_status {
        "pending_new_chat_bind" => "pending_bind",
        "preparing" | "submitting" | "deferred" | "legacy_recovery_submitting" => "submitting",
        "submission_ambiguous"
        | "submission_ambiguous_completion_pending"
        | "completion_ambiguous"
        | "legacy_submission_unverified"
        | "ambiguous" => "ambiguous",
        "failed" | "completion_conflict" => "failed",
        "submitted" => "submitted",
        "handled" => "handled",
        "bound" => "bound",
        _ => "idle",
    };
    let (target_state, target_label, target_reason) = if invalid_target {
        (
            "invalid",
            "Rebind ChatGPT chat",
            "The saved ChatGPT URL is provisional or invalid and cannot receive deliveries.",
        )
    } else if target_mismatch {
        (
            "unavailable",
            "Saved chat unavailable",
            "The live ChatGPT page does not match the saved conversation; delivery is blocked until it is reopened or rebound.",
        )
    } else {
        match kind.as_str() {
            "existing_chat" => (
                "bound",
                "Existing ChatGPT chat",
                "Next delivery goes to the saved ChatGPT conversation URL.",
            ),
            "new_chat" if internal_delivery_status == "pending_new_chat_bind" => (
                "new_chat_pending",
                "Binding new ChatGPT chat",
                "The first prompt was submitted; CCCC is waiting for ChatGPT to expose the final /c/... URL.",
            ),
            "new_chat" => (
                "new_chat_pending",
                "New ChatGPT chat on next delivery",
                "Next delivery starts a fresh ChatGPT chat, then binds its final /c/... URL.",
            ),
            _ => (
                "missing",
                "No target selected",
                "Save an existing ChatGPT chat or choose new-chat delivery.",
            ),
        }
    };
    let (delivery_label, delivery_reason) = match delivery_state {
        "handled" => (
            "Handled",
            "The original reports were already accepted through another delivery path; no browser resubmission is needed.",
        ),
        "pending_bind" => (
            "Binding chat",
            "Prompt was submitted; waiting for ChatGPT to assign the chat URL.",
        ),
        "submitting" if waiting.is_some() => {
            let (_, label, reason) = waiting.expect("deferred action");
            (label, reason)
        }
        "submitting" => (
            "Submitting",
            "CCCC is currently injecting this batch into the ChatGPT browser session.",
        ),
        "ambiguous" => (
            "Delivery unverified",
            if last_error.is_empty() {
                "CCCC attempted to submit the prompt, but could not verify whether ChatGPT accepted it."
            } else {
                last_error
            },
        ),
        "failed" => (
            "Delivery failed",
            if last_error.is_empty() {
                "The last ChatGPT delivery did not complete."
            } else {
                last_error
            },
        ),
        "submitted" => (
            "Submitted",
            if submission_evidence.is_empty() {
                "The last browser delivery was submitted."
            } else {
                submission_evidence
            },
        ),
        "bound" => (
            "Chat bound",
            "The submitted prompt has been bound to a ChatGPT conversation.",
        ),
        _ => (
            "No recent delivery",
            "No browser delivery has been recorded yet.",
        ),
    };
    let next_check = next_delivery_check(&state.home)?;
    let automatically_closed = !active && automation_hold(&state.home).is_none();
    let (next_action, next_label, next_reason) = if let Some(action) =
        waiting.filter(|_| automatically_closed)
    {
        action
    } else if automatically_closed && matches!(target_state, "bound" | "new_chat_pending") {
        (
            "none",
            "Browser opens when needed",
            "The dedicated browser stays closed until a report needs delivery. No action is required.",
        )
    } else if !active {
        (
            "open_chatgpt",
            "Open ChatGPT",
            "Open ChatGPT to sign in or inspect the page.",
        )
    } else if login_required {
        (
            "login_chatgpt",
            "Sign in to ChatGPT",
            "Open ChatGPT and sign in with this browser profile.",
        )
    } else if matches!(target_state, "missing" | "invalid" | "unavailable") {
        ("bind_chat", "Choose a target ChatGPT chat", target_reason)
    } else if let Some(action) = waiting {
        action
    } else if delivery_state == "pending_bind" {
        (
            "wait_for_chat_bind",
            "Wait for ChatGPT chat binding",
            delivery_reason,
        )
    } else if delivery_state == "ambiguous" {
        ("inspect_error", "Inspect ChatGPT delivery", delivery_reason)
    } else if delivery_state == "failed" {
        ("retry_delivery", "Retry ChatGPT delivery", delivery_reason)
    } else if !ready {
        (
            "inspect_browser",
            "Check browser readiness",
            "Browser readiness has not been confirmed for the current page.",
        )
    } else {
        (
            "none",
            "No action needed",
            "ChatGPT Web Model is ready for browser delivery.",
        )
    };
    let tone = if delivery_state == "failed" {
        "error"
    } else if next_action == "wait_for_reply" {
        "neutral"
    } else if next_action != "none" {
        "needs"
    } else if ready && matches!(target_state, "bound" | "new_chat_pending") {
        "ready"
    } else {
        "neutral"
    };
    let health = json!({
        "schema":"cccc.web_model.health.v1","group_id":group_id,"actor_id":actor_id,
        "tone":tone,
        "summary":next_label,
        "browser":{
            "state":if ready{"ready"}else if login_required{"sign_in_required"}else if active{"open"}else{"closed"},
            "label":if ready{"Ready"}else if login_required{"Needs sign-in"}else if active{"Open"}else{"Not open"},
            "reason":readiness["message"].as_str().unwrap_or(if active {
                "Open ChatGPT and sign in with this browser profile."
            } else {
                "Open ChatGPT to sign in or inspect the page."
            }),
            "active":active,"ready":ready,"logged_in_guess":ready,"url":url,
            "viewer_attached":surface["controller_attached"],
            "last_frame_at":surface["last_frame_at"]
        },
        "target":{
            "state":target_state,"label":target_label,"reason":target_reason,
            "url":if conversation_url.is_empty(){target["url"].as_str().unwrap_or("")}else{conversation_url.as_str()},
            "saved_at":target["saved_at"],"next_delivery":target["next_delivery"]
        },
        "delivery_target":target,
        "delivery":{
            "state":delivery_state,"label":delivery_label,"reason":delivery_reason,
            "next_check_at":next_check,
            "last_delivery_id":target["last_delivery_id"],
            "last_turn_id":target["last_delivery_turn_id"],
            "last_event_ids":target["last_delivery_event_ids"],
            "last_delivery_at":target["last_delivery_at"],
            "last_submission_evidence":submission_evidence,
            "last_send_selector":send_selector,
            "last_error":if delivery_state == "pending_bind" && last_error == "conversation_url_pending" {""} else {last_error},
            "mode":delivery_mode
        },
        "next_action":{"recommended":next_action,"label":next_label,"reason":next_reason}
    });
    let mut browser = json!({
        "active":active,
        "ready":ready,
        "login_required":login_required,
        "pid":metadata["pid"],
        "cdp_port":metadata["cdp_port"],
        "profile_dir":metadata["profile_dir"],
        "visibility":metadata["visibility"],
        "started_at":surface["started_at"],
        "updated_at":surface["updated_at"],
        "state":if ready{"ready"}else if login_required{"sign_in_required"}else if active{"open"}else{"idle"},
        "message":readiness["message"],
        "tab_url":url,
        "last_tab_url":url,
        "conversation_url":conversation_url,
        "pending_new_chat_bind":pending_new_chat_bind,
        "pending_new_chat_url":if pending_new_chat_bind {target["url"].as_str().unwrap_or("")} else {""},
        "pending_new_chat_bind_started_at":if pending_new_chat_bind {target["saved_at"].as_str().unwrap_or("")} else {""},
        "pending_new_chat_submitted":target["state"] == "new_chat_submitted",
        "pending_new_chat_submitted_at":target["submitted_at"],
        "pending_new_chat_delivery_id":target["delivery_id"],
        "pending_new_chat_last_turn_id":if pending_new_chat_bind {target["last_delivery_turn_id"].as_str().unwrap_or("")} else {""},
        "pending_new_chat_last_event_ids":if pending_new_chat_bind {target["last_delivery_event_ids"].clone()} else {json!([])},
        "pending_new_chat_last_tab_url":if pending_new_chat_bind {pending_new_chat_last_tab_url} else {""},
        "target_saved_at":target["saved_at"],
        "new_chat_bound_at":target["bound_at"],
        "delivery_target":target.clone()
    });
    browser
        .as_object_mut()
        .expect("browser session payload")
        .extend(
            json!({
            "bootstrap_seed_delivered_at":target["bootstrap_seed_delivered_at"],
            "bootstrap_seed_version":target["bootstrap_seed_version"],
            "bootstrap_seed_digest":target["bootstrap_seed_digest"],
            "bootstrap_seed_conversation_url":target["bootstrap_seed_conversation_url"],
            "last_delivery_at":target["last_delivery_at"],
            "last_delivery_started_at":target["last_delivery_started_at"],
            "last_delivery_id":target["last_delivery_id"],
            "last_delivery_status":delivery_status,
            "last_submission_evidence":submission_evidence,
            "last_send_selector":send_selector,
            "last_turn_id":target["last_delivery_turn_id"],
            "last_event_ids":target["last_delivery_event_ids"],
            "last_error":last_error,
            "delivery_mode":delivery_mode,
            "delivery_preference":delivery_preference,
            "health_snapshot":health
            })
            .as_object()
            .cloned()
            .expect("browser session details"),
        );
    Ok(success(json!({
        "browser_session":browser,"browser_surface":surface,"health_snapshot":health
    })))
}

fn deferred_action(evidence: &str) -> (&'static str, &'static str, &'static str) {
    match evidence {
        "not_sent_conversation_archived" => (
            "inspect_browser",
            "Choose an available conversation",
            "This conversation is archived. The report is retained; choose another target or restore this conversation yourself. Other groups can continue.",
        ),
        "not_sent_rate_limited" | "not_sent_access_denied" | "not_sent_verification_required" => (
            "inspect_browser",
            "Browser access needs attention",
            "The browser showed an access or request restriction. Automatic page recovery is stopped and the original report is retained.",
        ),
        "not_sent_composer_occupied" => (
            "resolve_draft",
            "Resolve the draft",
            "A draft is still in the input box. The report is retained and will resume after the draft is resolved.",
        ),
        "not_sent_chat_busy" => (
            "wait_for_reply",
            "Wait for the current reply",
            "ChatGPT is still responding. Automatic delivery closes the browser and checks again after five minutes; the original report stays queued.",
        ),
        "not_sent_composer_unavailable" => (
            "inspect_browser",
            "Restoring the conversation",
            "The ChatGPT input did not load. Automatic delivery closes the browser and checks again after five minutes; the original report is retained.",
        ),
        "not_sent_login_required" => (
            "login_chatgpt",
            "Sign in to ChatGPT",
            "Sign in using this browser before the queued report can be delivered.",
        ),
        _ => (
            "inspect_browser",
            "Check browser readiness",
            "The report has not been submitted. Inspect the browser before retrying.",
        ),
    }
}

fn cached_readiness(surface: &Value) -> Value {
    let current_url = surface["url"].as_str().unwrap_or_default();
    let cached = &surface["metadata"]["prompt_readiness"];
    let cached_url = cached["tab_url"].as_str().unwrap_or_default();
    if cached.is_object()
        && (current_url.is_empty() || cached_url.is_empty() || current_url == cached_url)
    {
        return cached.clone();
    }
    json!({
        "ready":false,
        "login_required":false,
        "tab_url":current_url,
        "message":"Browser is open; ChatGPT readiness has not been checked yet."
    })
}

fn validate_actor(state: &AppState, group_id: &str, actor_id: &str) -> Result<(), ApiError> {
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| ApiError::not_found(format!("actor not found: {actor_id}")))?;
    if actor.runtime != ActorRuntime::WebModel {
        return Err(ApiError::bad(
            "ChatGPT browser sessions can only be bound to actors using runtime=web_model",
        ));
    }
    Ok(())
}

fn browser_profile_path(home: &Path) -> PathBuf {
    home.join("state/web_model_browser/_shared/chatgpt_web/chrome_profile")
}

fn provider_url(provider: &str) -> &'static str {
    match provider.trim().to_ascii_lowercase().as_str() {
        "claude" => "https://claude.ai/",
        "gemini" => "https://gemini.google.com/",
        "grok" => "https://grok.com/",
        _ => "https://chatgpt.com/",
    }
}

fn browser_open_url(target: &Value, provider_url: &str) -> String {
    let stored = target["url"].as_str().map(str::trim).unwrap_or_default();
    let stored_is_http =
        reqwest::Url::parse(stored).is_ok_and(|url| matches!(url.scheme(), "http" | "https"));
    let stable_existing = target["kind"] != "existing_chat"
        || !is_chatgpt_url(stored)
        || normalized_chatgpt_conversation_url(stored).is_some();
    if matches!(target["kind"].as_str(), Some("existing_chat" | "new_chat"))
        && stored_is_http
        && stable_existing
    {
        stored.to_owned()
    } else {
        provider_url.to_owned()
    }
}

pub(super) fn surface_key() -> &'static str {
    crate::browser_surface::SHARED_WEB_MODEL_KEY
}

// Reuse persisted delivery facts as the owner fence, including after Web restarts.
pub(super) fn another_chat_is_pending(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
) -> Result<bool, ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    for meta in store.list().map_err(io_error)? {
        let group = store.load(&meta.group_id).map_err(io_error)?;
        let Some(targets) = group.extra.get(TARGETS_KEY).and_then(Value::as_object) else {
            continue;
        };
        for (actor, _) in targets {
            let target = cccc_core::web_model_connectors::browser_target(
                &state.home,
                &group.group_id,
                actor,
            )
            .map_err(io_error)?;
            if group.group_id == group_id && actor == actor_id {
                continue;
            }
            if target["kind"] == "new_chat"
                && matches!(
                    target["last_delivery_status"].as_str(),
                    Some(
                        "submitting"
                            | "submitted"
                            | "pending_new_chat_bind"
                            | "submission_ambiguous"
                            | "completion_ambiguous"
                            | "submission_ambiguous_completion_pending"
                    )
                )
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn key(group_id: &str, actor_id: &str) -> String {
    format!("web-model::{group_id}::{actor_id}")
}

fn required(body: &Value, key: &str) -> Result<String, ApiError> {
    let value = body.get(key).and_then(Value::as_str).unwrap_or_default();
    required_identifier(value, key).map(str::to_owned)
}

fn required_identifier<'a>(value: &'a str, key: &str) -> Result<&'a str, ApiError> {
    let value = value.trim();
    (!value.is_empty())
        .then_some(value)
        .ok_or_else(|| ApiError::bad(format!("{key} is required")))
}

fn dimension(body: &Value, key: &str, default: u32, min: u32, max: u32) -> u32 {
    body.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(default)
        .clamp(min, max)
}

fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}

#[cfg(test)]
mod wait_status_tests {
    #[test]
    fn pending_input_and_authentication_have_distinct_actions() {
        for (evidence, action) in [
            ("not_sent_composer_occupied", "resolve_draft"),
            ("not_sent_chat_busy", "wait_for_reply"),
            ("not_sent_login_required", "login_chatgpt"),
            ("new_unknown_preflight", "inspect_browser"),
        ] {
            let actual = super::deferred_action(evidence);
            assert_eq!(actual.0, action);
            assert!(!actual.1.is_empty() && !actual.2.is_empty());
        }
    }
}

// Reuse the original opt-in without changing Rust's verified default. Browser
// startup alone does not prove that a provider accepts authenticated headless use.
fn use_headless_browser(configured: Option<&str>) -> bool {
    matches!(
        configured
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on" | "enabled"
    )
}

#[cfg(test)]
mod launch_policy_tests {
    #[test]
    fn original_headless_override_does_not_replace_the_verified_default() {
        assert!(!super::use_headless_browser(None));
        for value in ["", "unknown", "0", "false", " NO ", "off", "disabled"] {
            assert!(!super::use_headless_browser(Some(value)));
        }
        for value in ["1", "true", " YES ", "on", "enabled"] {
            assert!(super::use_headless_browser(Some(value)));
        }
    }
}

#[cfg(test)]
mod automation_hold_tests {
    use super::*;

    #[test]
    fn delivery_deadline_is_account_wide_persistent_and_fail_closed() {
        let temp = tempfile::tempdir().expect("isolated state");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("init");
        assert!(next_delivery_check(&home).expect("new account").is_none());
        let before = chrono::Utc::now();
        schedule_delivery_check(&home, true).expect("busy close");
        let after = chrono::Utc::now();
        let restarted =
            cccc_core::HomeLayout::from_path(home.root().to_path_buf()).expect("restart");
        let deadline = next_delivery_check(&restarted)
            .expect("stored")
            .expect("deadline");
        assert!(deadline >= before + chrono::Duration::minutes(5));
        assert!(deadline <= after + chrono::Duration::minutes(5));
        for _ in 0..100 {
            assert_eq!(
                next_delivery_check(&home).expect("read-only"),
                Some(deadline)
            );
        }
        let path = home
            .root()
            .join("state/web_model_browser/_shared/delivery_check.json");
        for corrupt in ["invalid-json", "{}", "[]", r#"{"not_before":"invalid"}"#] {
            std::fs::write(&path, corrupt).expect("corrupt fixture");
            assert!(
                next_delivery_check(&home).is_err(),
                "bad state authorized browser access"
            );
        }
        schedule_delivery_check(&home, false).expect("completed visit");
        assert!(next_delivery_check(&home).expect("no waiting").is_none());
    }

    #[test]
    fn account_hold_survives_new_state_and_is_fail_closed() {
        let temp = tempfile::tempdir().expect("isolated state");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("init");
        assert!(automation_hold(&home).is_none());
        pause_automation(&home, "not_sent_rate_limited").expect("persist account hold");
        let restarted =
            cccc_core::HomeLayout::from_path(home.root().to_path_buf()).expect("new process view");
        for _ in 0..100 {
            assert_eq!(
                automation_hold(&restarted).expect("account hold")["reason"],
                "not_sent_rate_limited"
            );
        }
        std::fs::write(automation_hold_path(&home), b"invalid-json")
            .expect("storage error fixture");
        assert!(
            automation_hold(&restarted).is_some(),
            "corrupt storage must not permit traffic"
        );
        resume_automation(&home).expect("explicit user resume");
        assert!(automation_hold(&restarted).is_none());
    }

    #[tokio::test]
    async fn account_hold_prevents_actor_navigation_without_starting_a_browser() {
        let temp = tempfile::tempdir().expect("isolated state");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("init");
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let (_, _, browser, state) = crate::app_with_shutdown(
            home.clone(),
            shutdown,
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            "test".into(),
        );
        hold_on_restriction(
            &state,
            &json!({"submission_evidence":"not_sent_conversation_archived"}),
        )
        .expect("archive only affects its group");
        assert!(automation_hold(&home).is_none());
        hold_on_restriction(
            &state,
            &json!({"submission_evidence":"not_sent_rate_limited"}),
        )
        .expect("account restriction");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let mut ids = Vec::new();
        for name in ["blocked-source", "other-group", "new-group"] {
            let mut group = store.create(name, "").expect("group");
            group.running = true;
            let mut actor = cccc_contracts::Actor::new("web");
            actor.runtime = cccc_contracts::ActorRuntime::WebModel;
            actor.runner = cccc_contracts::RunnerKind::Headless;
            actor
                .env
                .insert("CCCC_WEB_MODEL_DELIVERY_MODE".into(), "browser".into());
            group.actors.push(actor);
            store.save(&group).expect("active actor");
            cccc_core::web_model_connectors::save_browser_target(
                &home,
                &group.group_id,
                "web",
                Some(json!({"kind":"existing_chat","url":"http://127.0.0.1:9/c/held"})),
            )
            .expect("target");
            assert!(super::super::web_model_supervisor::actor_delivery_enabled(
                &state,
                &group.group_id,
                "web"
            ));
            ids.push(group.group_id);
        }
        for gid in &ids {
            assert!(matches!(
                super::super::web_model_delivery::deliver_pending(&state, gid, "web")
                    .await
                    .expect("held delivery"),
                super::super::web_model_delivery::DeliveryOutcome::Stopped
            ));
        }
        for _ in 0..10 {
            super::super::web_model_supervisor::ensure_running_actor(&state, None).await;
            assert!(
                !browser.info(surface_key()).await["active"]
                    .as_bool()
                    .unwrap_or(false)
            );
        }
        assert!(automation_hold(&home).is_some());
    }
}
