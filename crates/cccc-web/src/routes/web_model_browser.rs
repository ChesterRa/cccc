use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::{ActorRuntime, utc_now};
use cccc_core::GroupStore;
use cccc_core::integration_state;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::fs;
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
    group_id: String,
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
    validate_actor(state, group_id, actor_id)?;
    let provider = super::web_model_connector_store::for_actor(state, group_id, actor_id)
        .and_then(|item| item["provider"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "chatgpt".into());
    let target = super::web_model_delivery_state::target(state, group_id, actor_id)?;
    let open_url = browser_open_url(&target, provider_url(&provider));
    let profile = browser_profile_path(state.home.root(), group_id, actor_id)?;
    state
        .browser_surfaces
        .ensure_open_system(&key(group_id, actor_id), &profile, &open_url, width, height)
        .await
        .map_err(|error| ApiError::bad(format!("{error:#}")))?;
    let session_key = key(group_id, actor_id);
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
        Some("existing_chat" | "new_chat") => {
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
    state
        .browser_surfaces
        .close(&key(&group_id, &actor_id))
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?;
    payload(&state, &group_id, &actor_id, false).await
}

async fn bind_current(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    validate_actor(&state, &group_id, &actor_id)?;
    let clear = body.get("clear").and_then(Value::as_bool).unwrap_or(false);
    let current = state
        .browser_surfaces
        .info(&key(&group_id, &actor_id))
        .await;
    let mut url = body
        .get("conversation_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if url.is_empty() {
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
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    integration_state::group_update(&store, &group_id, TARGETS_KEY, |value| {
        let targets = ensure_object(value);
        if clear {
            targets.remove(&actor_id);
        } else {
            targets.insert(actor_id.clone(), target);
        }
        Ok(())
    })
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
    let session_key = key(group_id, actor_id);
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
    let targets = integration_state::group_get(&store, group_id, TARGETS_KEY).map_err(io_error)?;
    let mut target = targets.get(actor_id).cloned().unwrap_or_else(|| json!({}));
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
        "failed" | "completion_conflict" => "failed",
        "submitted" => "submitted",
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
        "pending_bind" => (
            "Binding chat",
            "Prompt was submitted; waiting for ChatGPT to assign the chat URL.",
        ),
        "submitting" if internal_delivery_status == "deferred" => (
            "Waiting to submit",
            "ChatGPT is responding and no safe Send prompt control is available yet.",
        ),
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
    let (next_action, next_label, next_reason) = if !active {
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
    } else {
        (
            "none",
            "No action needed",
            "ChatGPT Web Model is ready for browser delivery.",
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

fn browser_profile_path(home: &Path, group_id: &str, actor_id: &str) -> Result<PathBuf, ApiError> {
    let group_id = safe_segment(group_id)?;
    let actor_id = safe_segment(actor_id)?;
    let shared = home.join("state/web_model_browser/_shared/chatgpt_web/chrome_profile");
    let legacy = home
        .join("browser-profiles/web-model")
        .join(group_id)
        .join(actor_id);
    if directory_has_content(&shared) || !directory_has_content(&legacy) {
        Ok(shared)
    } else {
        Ok(legacy)
    }
}

fn directory_has_content(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .and_then(|mut entries| entries.next())
        .is_some()
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

fn safe_segment(value: &str) -> Result<&str, ApiError> {
    (!value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    .then_some(value)
    .ok_or_else(|| ApiError::bad("invalid browser profile identifier"))
}

fn dimension(body: &Value, key: &str, default: u32, min: u32, max: u32) -> u32 {
    body.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(default)
        .clamp(min, max)
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("object initialized")
}

fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
