use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_core::GroupStore;
use cccc_core::integration_state;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::browser_surface::{
    conversation_target_matches, is_chatgpt_url, normalized_chatgpt_conversation_url,
};

pub(super) const TARGETS_KEY: &str = "web_model_browser_targets";
const DELIVERY_PREFERENCES_KEY: &str = "web_model_delivery_preferences";

#[derive(Debug, Deserialize)]
struct SessionQuery {
    group_id: String,
    actor_id: String,
    #[serde(default)]
    inspect: bool,
    #[serde(default)]
    mode: String,
}

#[derive(Debug, Default, Deserialize)]
struct InspectQuery {
    #[serde(default)]
    inspect: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/web-model/browser-session", get(info))
        .route("/api/v1/web-model/browser-session/open", post(open))
        .route("/api/v1/web-model/browser-session/close", post(close))
        .route(
            "/api/v1/web-model/browser-session/grok-bind",
            post(bind_grok),
        )
        .route("/api/v1/web-model/browser-session/reload", post(reload))
        .route(
            "/api/v1/web-model/browser-session/bind-current",
            post(bind_current),
        )
        .route(
            "/api/v1/web-model/browser-session/delivery-preference",
            post(update_delivery_preference),
        )
        .route(
            "/api/v1/web-model/browser-session/resume-delivery",
            post(resume_delivery),
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
    validate_actor(state, group_id, actor_id)?;
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(group_id)
        .map_err(io_error)?;
    let actor = group
        .actors
        .iter()
        .find(|a| a.id == actor_id)
        .ok_or_else(|| ApiError::not_found("Web Model Actor was removed"))?;
    let generation = crate::browser_surface::actor_identity(actor);
    let provider = actor
        .runtime
        .web_model_provider()
        .expect("validated runtime");
    let target = super::web_model_delivery_state::target(state, group_id, actor_id)?;
    let open_url = browser_open_url(&target, provider_url(provider))?;
    let profile = super::web_model_shared_browser::profile(state, provider);
    state
        .browser_surfaces
        .ensure_open_shared_actor(
            &key(group_id, actor_id),
            &profile,
            &open_url,
            (width, height),
            &generation,
        )
        .await
        .map_err(|error| ApiError::bad(format!("{error:#}")))?;
    let session_key = key(group_id, actor_id);
    // An existing surface may be midway through sign-in or navigation. Opening
    // its viewer must not send it away from that page to the saved conversation.
    let readiness = state
        .browser_surfaces
        .prompt_readiness(&session_key)
        .await
        .unwrap_or_default();
    if readiness["ready"] != true
        || readiness["composer_chars"].as_u64().unwrap_or_default() > 0
        || readiness["composer_has_attachments"] == true
        || readiness["running"] == true
    {
        return Ok(state.browser_surfaces.info(&session_key).await);
    }
    match target["kind"].as_str() {
        Some("existing_chat") if is_chatgpt_url(&open_url) => {
            if normalized_chatgpt_conversation_url(&open_url).is_some()
                && let Err(error) = state
                    .browser_surfaces
                    .align_chatgpt_conversation_target(
                        &session_key,
                        &open_url,
                        std::time::Duration::from_secs(5),
                    )
                    .await
            {
                tracing::warn!(group_id, actor_id, %error, "saved ChatGPT conversation could not be opened");
            }
        }
        Some("existing_chat" | "new_chat")
            if !conversation_target_matches(
                &open_url,
                readiness["tab_url"].as_str().unwrap_or_default(),
            ) =>
        {
            if let Err(error) = state
                .browser_surfaces
                .navigate_to_url(&session_key, &open_url)
                .await
            {
                tracing::warn!(group_id, actor_id, %error, "saved Web-model target could not be opened");
            }
        }
        _ => {}
    }
    Ok(state.browser_surfaces.info(&session_key).await)
}

async fn close(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    require_stopped(&state, &group_id, &actor_id)?;
    let _control = super::web_model_delivery::control_guard(&group_id, &actor_id)?;
    state
        .browser_surfaces
        .close(&key(&group_id, &actor_id))
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?;
    payload(&state, &group_id, &actor_id, false).await
}

// Grok uses the user-selected Bot URL and a revocable Actor credential.
async fn bind_grok(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    let _control = super::web_model_delivery::control_guard(&group_id, &actor_id)?;
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(&group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))?;
    let actor = group
        .actors
        .iter()
        .find(|a| a.id == actor_id)
        .ok_or_else(|| ApiError::not_found(format!("actor not found: {actor_id}")))?;
    if actor.runtime.web_model_provider() != Some("grok_web") {
        return Err(ApiError::bad(
            "Actor must use the Grok Bot Web Model runtime",
        ));
    }
    let identity = crate::browser_surface::actor_identity(actor);
    require_stopped(&state, &group_id, &actor_id)?;
    let url =
        cccc_core::web_model_connectors::grok_bot_url(body["url"].as_str().unwrap_or_default())
            .map_err(io_error)?;
    let connector = super::web_model_connector_store::load(&state)?
        .into_iter()
        .find(|c| c["provider"] == "grok_web" && c["revoked"] != true)
        .ok_or_else(|| {
            ApiError::bad("Configure the Grok connector in global Web Model settings first")
        })?;
    let surface_key = key(&group_id, &actor_id);
    let has_page = state.browser_surfaces.info(&surface_key).await["active"] == true;
    if has_page {
        require_idle_grok_page(&state, &surface_key).await?;
    }
    super::web_model_delivery_completion::call(&state, "web_model_grok_bind",
        json!({"by":"user","group_id":group_id,"actor_id":actor_id,"connector_id":connector["connector_id"],"url":url}).as_object().expect("object").clone()).await?;
    if has_page {
        // A stopped Actor retains its Page. Complete the explicit save by
        // aligning that Page, rather than waiting for someone to open its viewer.
        // Recheck after IPC so a draft typed during persistence is preserved.
        require_idle_grok_page(&state, &surface_key).await?;
        let surface = state.browser_surfaces.info(&surface_key).await;
        if surface["metadata"]["actor_identity"] != identity {
            // Stop/update retain the old Page. A provider or generation change
            // must retire it, never navigate Grok inside another login profile.
            state
                .browser_surfaces
                .close(&surface_key)
                .await
                .map_err(|e| ApiError::bad(format!("{e:#}")))?;
            ensure_open_for_actor(
                &state,
                &group_id,
                &actor_id,
                dimension(&surface, "width", 1366, 640, 2560),
                dimension(&surface, "height", 900, 480, 1600),
            )
            .await?;
            return payload(&state, &group_id, &actor_id, false).await;
        }
        let observed = state
            .browser_surfaces
            .navigate_to_url(&surface_key, &url)
            .await
            .map_err(|e| {
                ApiError::bad(format!(
                    "Bot URL saved, but its window could not be opened: {e:#}"
                ))
            })?;
        if !conversation_target_matches(&url, &observed) {
            return Err(ApiError::bad(
                "Bot URL saved, but its window is on another page. Check login and retry saving.",
            ));
        }
    }
    payload(&state, &group_id, &actor_id, false).await
}

async fn require_idle_grok_page(state: &AppState, surface_key: &str) -> Result<(), ApiError> {
    let ready = state
        .browser_surfaces
        .prompt_readiness(surface_key)
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    if ready["composer_chars"].as_u64().unwrap_or_default() > 0
        || ready["composer_has_attachments"] == true
        || ready["running"] == true
    {
        return Err(ApiError::bad(
            "Send or clear the existing draft and wait for Grok before changing the Bot",
        ));
    }
    Ok(())
}

// ChatGPT URL navigation proposes a target; it still requires host-session pairing.
async fn bind_current(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let _control = super::web_model_delivery::control_guard(&group_id, &actor_id)?;
    require_stopped(&state, &group_id, &actor_id)?;
    if actor_provider(&state, &group_id, &actor_id)? != "chatgpt_web" {
        return Err(ApiError::bad("Use the Grok Bot URL setting for this Actor"));
    }
    let url = if body["new_chat"] == true {
        "https://chatgpt.com/".to_owned()
    } else {
        cccc_core::web_model_connectors::conversation_url(
            body["conversation_url"].as_str().unwrap_or_default(),
        )
        .map_err(io_error)?
    };
    ensure_open_for_actor(&state, &group_id, &actor_id, 1366, 900).await?;
    let key = key(&group_id, &actor_id);
    let readiness = state
        .browser_surfaces
        .prompt_readiness(&key)
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    if readiness["composer_chars"].as_u64().unwrap_or_default() > 0
        || readiness["composer_has_attachments"] == true
        || readiness["running"] == true
    {
        return Err(ApiError::bad(
            "Send or clear the existing draft before opening another conversation",
        ));
    }
    state
        .browser_surfaces
        .navigate_to_url(&key, &url)
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    // A proposed URL is only a startup destination, never routing authority.
    // Keep it across restarts without overwriting historical delivery evidence.
    if super::web_model_connector_store::for_actor(&state, &group_id, &actor_id).is_none() {
        let groups = GroupStore::new(state.home.clone()).map_err(io_error)?;
        integration_state::group_update(&groups, &group_id, TARGETS_KEY, |targets| {
            if !targets.is_object() {
                *targets = json!({});
            }
            if !targets[&actor_id].is_object() {
                targets[&actor_id] = json!({});
            }
            targets[&actor_id]["setup_url"] = json!(url);
            Ok(())
        })
        .map_err(io_error)?;
    }
    payload(&state, &group_id, &actor_id, false).await
}

fn require_stopped(state: &AppState, group_id: &str, actor_id: &str) -> Result<(), ApiError> {
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(group_id)
        .map_err(io_error)?;
    if group.actors.iter().any(|a| a.id == actor_id && a.enabled) {
        return Err(ApiError::bad(
            "Stop this Actor before changing its browser window",
        ));
    }
    Ok(())
}

async fn reload(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let _control = super::web_model_delivery::control_guard(&group_id, &actor_id)?;
    state
        .browser_surfaces
        .command(&key(&group_id, &actor_id), &json!({"t":"refresh"}))
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    payload(&state, &group_id, &actor_id, false).await
}

async fn resume_delivery(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let delivery_id = required(&body, "delivery_id")?;
    super::web_model_delivery::resume_after_review(&state, &group_id, &actor_id, &delivery_id)
        .await?;
    super::web_model_delivery::ensure_worker(state.clone(), group_id.clone(), actor_id.clone())
        .await;
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
    let session_key = key(group_id, actor_id);
    if query.mode.trim().eq_ignore_ascii_case("vnc") {
        return Err(ApiError::forbidden(
            "Use global Web Model settings for the shared browser desktop",
        ));
    }
    let vnc = false;
    let viewer_mode = "screencast".to_owned();
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
                &session_key,
                state.shutdown.subscribe(),
            )
            .await;
        } else {
            crate::browser_surface::serve_socket(
                socket,
                &state.browser_surfaces,
                &session_key,
                &viewer_mode,
                state.shutdown.subscribe(),
            )
            .await;
        }
    }))
}

async fn payload(state: &AppState, group_id: &str, actor_id: &str, inspect: bool) -> ApiResult {
    let session_key = key(group_id, actor_id);
    let mut surface = state.browser_surfaces.info(&session_key).await;
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    let mut target = super::web_model_delivery_state::target(state, group_id, actor_id)?;
    let preferences = integration_state::group_get(&store, group_id, DELIVERY_PREFERENCES_KEY)
        .map_err(io_error)?;
    let stored_preference = preferences
        .get(actor_id)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let provider = actor_provider(state, group_id, actor_id)?;
    let delivery_mode = match stored_preference["mode"].as_str() {
        Some("image_compat") if provider == "chatgpt_web" => "image_compat",
        _ => "standard",
    };
    let delivery_preference = json!({
        "mode":delivery_mode,
        "updated_at":stored_preference["updated_at"].as_str().unwrap_or(""),
        "updated_by":stored_preference["updated_by"].as_str().unwrap_or("")
    });
    let active = surface["active"].as_bool().unwrap_or(false);
    let readiness = if active && inspect {
        let readiness = state
            .browser_surfaces
            .prompt_readiness(&session_key)
            .await
            .unwrap_or_else(|error| {
                json!({
                    "ready":false,
                    "login_required":true,
                    "tab_url":surface["url"],
                    "message":error.to_string()
                })
            });
        surface = state.browser_surfaces.info(&session_key).await;
        readiness
    } else if active {
        cached_readiness(&surface)
    } else {
        json!({"ready":false,"login_required":false,"tab_url":surface["url"]})
    };
    surface["viewer"] = json!({"kind":"screencast"});
    let metadata = surface
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let ready = readiness["ready"].as_bool().unwrap_or(false);
    let login_required = readiness["login_required"].as_bool().unwrap_or(false);
    let verification_required = readiness["verification_required"]
        .as_bool()
        .unwrap_or(false);
    let url = readiness["tab_url"]
        .as_str()
        .or_else(|| surface["url"].as_str())
        .unwrap_or("");
    let kind = target["kind"].as_str().unwrap_or("").to_owned();
    let stored_target_url = target["url"].as_str().unwrap_or("").to_owned();
    let chatgpt_existing = kind == "existing_chat" && is_chatgpt_url(&stored_target_url);
    let grok_existing = kind == "existing_chat" && provider == "grok_web";
    let normalized_target_url = if grok_existing {
        cccc_core::web_model_connectors::grok_bot_url(&stored_target_url).ok()
    } else {
        chatgpt_existing
            .then(|| normalized_chatgpt_conversation_url(&stored_target_url))
            .flatten()
    };
    let invalid_target = (chatgpt_existing || grok_existing) && normalized_target_url.is_none();
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
        target["label"] = json!("Reconnect conversation");
        target["detail"] =
            json!("The saved conversation URL is invalid and cannot receive deliveries.");
    } else if target_mismatch {
        target["state"] = json!("existing_chat_unavailable");
        target["next_delivery"] = json!("blocked");
        target["label"] = json!("Saved chat unavailable");
        target["detail"] = json!(
            "The live browser page does not match the saved conversation; delivery is blocked until it is reopened or rebound."
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
    let send_selector = submission["send_selector"].as_str().unwrap_or("");
    let pending_new_chat_last_tab_url = submission["tab_url"]
        .as_str()
        .or_else(|| submission["observed"]["url"].as_str())
        .unwrap_or("");
    let last_error = target["last_error"].as_str().unwrap_or("");
    let delivery_state = match internal_delivery_status {
        "pending_new_chat_bind" => "pending_bind",
        "submitting" | "deferred" | "legacy_recovery_submitting" => "submitting",
        "submission_ambiguous"
        | "submission_ambiguous_completion_pending"
        | "completion_ambiguous"
        | "legacy_submission_unverified"
        | "ambiguous" => "ambiguous",
        "draft_blocked" => "blocked",
        "failed" | "completion_conflict" => "failed",
        "submitted" => "submitted",
        "bound" => "bound",
        _ => "idle",
    };
    let (target_state, target_label, target_reason) = if invalid_target {
        (
            "invalid",
            "Reconnect conversation",
            "The saved conversation URL is invalid and cannot receive deliveries.",
        )
    } else if target_mismatch {
        (
            "unavailable",
            "Saved chat unavailable",
            "The live browser page does not match the saved conversation; delivery is blocked until it is reopened or rebound.",
        )
    } else {
        match kind.as_str() {
            "existing_chat" => (
                "bound",
                "Saved conversation",
                "Next delivery goes to the saved conversation URL.",
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
                if provider == "grok_web" {
                    "Save the Grok Bot URL in Actor settings."
                } else {
                    "Save an existing ChatGPT chat or choose new-chat delivery."
                },
            ),
        }
    };
    let (delivery_label, delivery_reason) = match delivery_state {
        "blocked" => (
            "Unsent draft",
            "The conversation has an unsent draft. Send or clear it in the browser; queued CCCC messages will then continue.",
        ),
        "pending_bind" => (
            "Binding chat",
            "Prompt was submitted; waiting for ChatGPT to assign the chat URL.",
        ),
        "submitting" if internal_delivery_status == "deferred" => (
            "Waiting to submit",
            "The Web Model is responding or its Send control is not ready.",
        ),
        "submitting" => (
            "Submitting",
            "CCCC is submitting this batch in the Actor browser window.",
        ),
        "ambiguous" => (
            "Delivery unverified",
            if last_error.is_empty() {
                "CCCC attempted to submit the prompt, but could not verify whether the website accepted it."
            } else {
                last_error
            },
        ),
        "failed" => (
            "Delivery failed",
            if last_error.is_empty() {
                "The last browser delivery did not complete."
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
    let (next_action, next_label, next_reason) = if !active {
        (
            "open_chatgpt",
            "Open browser",
            "Open the shared browser to sign in or inspect the page.",
        )
    } else if verification_required {
        (
            "verify_browser",
            "Complete security verification",
            "Complete the website's security verification in this browser. Delivery is waiting.",
        )
    } else if login_required {
        (
            "login_chatgpt",
            "Sign in",
            "Sign in using the shared browser for this provider.",
        )
    } else if matches!(target_state, "missing" | "invalid" | "unavailable") {
        ("bind_chat", "Choose a conversation", target_reason)
    } else if delivery_state == "pending_bind" {
        (
            "wait_for_chat_bind",
            "Wait for ChatGPT chat binding",
            delivery_reason,
        )
    } else if matches!(delivery_state, "ambiguous" | "blocked") {
        ("inspect_error", "Inspect delivery", delivery_reason)
    } else if delivery_state == "failed" {
        ("retry_delivery", "Retry delivery", delivery_reason)
    } else {
        (
            "none",
            "No action needed",
            "The Web Model is ready for browser delivery.",
        )
    };
    let tone = if delivery_state == "failed" {
        "error"
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
            "state":if verification_required{"verification_required"}else if ready{"ready"}else if login_required{"sign_in_required"}else if active{"open"}else{"closed"},
            "label":if verification_required{"Needs verification"}else if ready{"Ready"}else if login_required{"Needs sign-in"}else if active{"Open"}else{"Not open"},
            "reason":readiness["message"].as_str().unwrap_or(if active {
                "Sign in using the shared browser for this provider."
            } else {
                "Open the shared browser to sign in or inspect the page."
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
        "verification_required":verification_required,
        "pid":metadata["pid"],
        "cdp_port":metadata["cdp_port"],
        "profile_dir":metadata["profile_dir"],
        "visibility":metadata["visibility"],
        "started_at":surface["started_at"],
        "updated_at":surface["updated_at"],
        "state":if verification_required{"verification_required"}else if ready{"ready"}else if login_required{"sign_in_required"}else if active{"open"}else{"idle"},
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
            "can_resume_delivery":kind == "existing_chat" && internal_delivery_status == "submission_ambiguous",
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
        "browser_session":browser,"browser_surface":surface,"health_snapshot":health,
        "pairing":pairing_state(state,group_id,actor_id)?
    })))
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
        "message":"Browser is open; sign-in readiness has not been checked yet."
    })
}

fn actor_provider(state: &AppState, group: &str, actor: &str) -> Result<&'static str, ApiError> {
    GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(group)
        .map_err(io_error)?
        .actors
        .iter()
        .find(|a| a.id == actor)
        .and_then(|a| a.runtime.web_model_provider())
        .ok_or_else(|| ApiError::bad("Actor must use a Web Model runtime"))
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
    if !actor.runtime.is_web_model() {
        return Err(ApiError::bad("Browser sessions require a Web Model Actor"));
    }
    Ok(())
}

fn pairing_state(state: &AppState, group: &str, actor: &str) -> Result<Value, ApiError> {
    let groups = GroupStore::new(state.home.clone()).map_err(io_error)?;
    let doc = groups.load(group).map_err(io_error)?;
    let active = doc.running
        && !matches!(
            doc.state,
            cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
        );
    let enabled = doc
        .actors
        .iter()
        .find(|a| a.id == actor)
        .is_some_and(|a| a.enabled);
    let provider = doc
        .actors
        .iter()
        .find(|a| a.id == actor)
        .and_then(|a| a.runtime.web_model_provider())
        .unwrap_or("chatgpt_web");
    let connector = super::web_model_connector_store::load(state)?
        .into_iter()
        .find(|c| c["revoked"] != true && c["provider"] == provider);
    let Some(connector) = connector else {
        return Ok(json!({"state":"connector_required","actor_enabled":enabled}));
    };
    let pair = cccc_core::web_model_connectors::pairing_for_actor(&connector, group, actor);
    let binding = super::web_model_connector_store::for_actor(state, group, actor);
    let groups = GroupStore::new(state.home.clone()).map_err(io_error)?;
    let saved = integration_state::group_get(&groups, group, TARGETS_KEY).map_err(io_error)?;
    let expired = pair.as_ref().is_some_and(|p| {
        matches!(
            p["state"].as_str(),
            Some("waiting" | "awaiting_confirmation")
        ) && p["expires_at_ms"]
            .as_i64()
            .is_none_or(|t| t <= chrono::Utc::now().timestamp_millis())
    });
    let interrupted = pair.as_ref().is_some_and(|p| {
        matches!(
            p["state"].as_str(),
            Some("waiting" | "awaiting_confirmation")
        ) && !super::web_model_pairing::is_connecting(
            state,
            p["pairing_id"].as_str().unwrap_or_default(),
        )
    });
    Ok(
        json!({"state":if expired {json!("failed")} else if interrupted { json!("interrupted") } else { pair.as_ref().map(|p|p["state"].clone()).unwrap_or_else(||json!(if binding.is_some(){"bound"}else if enabled && active && provider == "chatgpt_web" {"waiting_to_connect"}else{"unpaired"})) },
        "error_code":if expired {json!("pairing_timeout")} else if interrupted {json!("pairing_interrupted")} else {pair.as_ref().map(|p|p["error_code"].clone()).unwrap_or(Value::Null)},
        "previous_url":saved[actor]["url"],
        "pairing_id":pair.as_ref().map(|p|&p["pairing_id"]),"expires_at_ms":pair.as_ref().map(|p|&p["expires_at_ms"]),
        "url":binding.as_ref().map(|b|&b["url"]),"actor_enabled":enabled}),
    )
}

fn provider_url(provider: &str) -> &'static str {
    match provider.trim().to_ascii_lowercase().as_str() {
        "claude" => "https://claude.ai/",
        "gemini" => "https://gemini.google.com/",
        "grok" | "grok_web" => "https://grok.com/",
        _ => "https://chatgpt.com/",
    }
}

fn browser_open_url(target: &Value, provider_url: &str) -> Result<String, ApiError> {
    if provider_url == "https://grok.com/" {
        // A Grok Actor owns a configured Bot, never the shared login/home page.
        return (target["kind"] == "existing_chat")
            .then(|| target["url"].as_str())
            .flatten()
            .and_then(|url| cccc_core::web_model_connectors::grok_bot_url(url).ok())
            .ok_or_else(|| {
                ApiError::bad_code(
                    "grok_bot_url_required",
                    "Save a valid Grok Bot URL in this Actor's settings before opening its window.",
                    json!({}),
                )
            });
    }
    if target["kind"] == "none" {
        if provider_url != "https://chatgpt.com/" {
            return Ok(provider_url.to_owned());
        }
        let setup = target["setup_url"].as_str().unwrap_or_default();
        return Ok(cccc_core::web_model_connectors::conversation_url(setup)
            .unwrap_or_else(|_| provider_url.to_owned()));
    }
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
        Ok(stored.to_owned())
    } else {
        Ok(provider_url.to_owned())
    }
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
mod startup_url_tests {
    #[test]
    fn grok_requires_a_bound_bot_instead_of_a_homepage_or_new_chat() {
        use serde_json::json;
        let bot = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
        for target in [
            json!({}),
            json!({"kind":"none","url":bot}),
            json!({"kind":"new_chat","url":bot}),
            json!({"kind":"existing_chat","url":"https://grok.com/"}),
            json!({"kind":"existing_chat","url":"https://chatgpt.com/c/old"}),
        ] {
            assert!(super::browser_open_url(&target, "https://grok.com/").is_err());
        }
        assert_eq!(
            super::browser_open_url(
                &json!({"kind":"existing_chat","url":bot}),
                "https://grok.com/"
            )
            .expect("saved Grok Bot"),
            bot
        );
    }

    #[test]
    fn unpaired_startup_destination_never_uses_historical_delivery_url() {
        assert!(
            super::browser_open_url(
                &serde_json::json!({"kind":"none","setup_url":"https://chatgpt.com/c/old"}),
                "https://grok.com/"
            )
            .is_err(),
            "Grok requires its own saved Bot, even after a runtime change"
        );
        let fallback = "https://chatgpt.com/";
        assert_eq!(
            super::browser_open_url(
                &serde_json::json!({"kind":"none","url":"https://chatgpt.com/c/old"}),
                fallback
            )
            .expect("ChatGPT URL"),
            fallback
        );
        assert_eq!(
            super::browser_open_url(
                &serde_json::json!({"kind":"none","setup_url":"https://chatgpt.com/c/chosen"}),
                fallback
            )
            .expect("ChatGPT URL"),
            "https://chatgpt.com/c/chosen"
        );
        assert_eq!(
            super::browser_open_url(
                &serde_json::json!({"kind":"none","setup_url":"https://other.example/c/a"}),
                fallback
            )
            .expect("ChatGPT URL"),
            fallback
        );
        assert_eq!(
            super::browser_open_url(
                &serde_json::json!({"kind":"existing_chat","url":"https://chatgpt.com/c/bound","setup_url":"https://chatgpt.com/c/chosen"}),
                fallback
            ).expect("ChatGPT URL"),
            "https://chatgpt.com/c/bound"
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "web_model_browser/draft_tests.rs"]
mod draft_tests;
