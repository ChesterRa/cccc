use cccc_contracts::{ActorRole, DaemonRequest, Event, utc_now};
use cccc_core::{GroupStore, HomeLayout, assistant_state, ledger, settings, voice_recording_lease};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use crate::ops::{actor_delivery, actor_runtime, actor_secrets};

use super::{voice_document_state, voice_input};

const KEY: &str = "assistants";
const ASSISTANT_ID: &str = "voice_secretary";
const ACTOR_ID: &str = "voice-secretary";

pub fn index(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let assistant_id = string_arg(request, "assistant_id").unwrap_or_default();
    if !assistant_id.is_empty() && assistant_id != ASSISTANT_ID {
        return Err(OpError::new("assistant_not_found", "assistant not found"));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store.load(&group_id).map_err(OpError::not_found)?;
    let state = assistant_state::load(home, &group_id).map_err(OpError::io)?;
    let document_state = voice_document_state::load(home, &group_id).map_err(OpError::io)?;
    let (input_latest, input_covered) =
        voice_input::status(home, &group_id).map_err(OpError::io)?;
    let assistant = project_actor_runtime(&group, effective_assistant(&state));
    let docs = document_state["documents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|document| voice_document_state::is_active(document))
        .cloned()
        .collect::<Vec<_>>();
    let asks = state["ask_requests"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|item| {
            item["cleared_at"]
                .as_str()
                .is_none_or(|value| value.is_empty())
        })
        .collect::<Vec<_>>();
    let documents_by_path = docs
        .iter()
        .filter_map(|item| {
            item["document_path"]
                .as_str()
                .map(|path| (path.to_owned(), item.clone()))
        })
        .collect::<Map<_, _>>();
    let configured_active_id = document_state["active_document_id"]
        .as_str()
        .unwrap_or_default();
    let configured_active_path = document_state["active_document_path"]
        .as_str()
        .unwrap_or_default();
    let active_document =
        voice_document_state::resolved_active(&docs, configured_active_id, configured_active_path);
    let active_document_id = active_document
        .and_then(|document| document["document_id"].as_str())
        .unwrap_or_default();
    let active_document_path = active_document
        .and_then(|document| document["document_path"].as_str())
        .unwrap_or_default();
    object(
        json!({"group_id":group_id,"assistants":[assistant],"assistants_by_id":{ASSISTANT_ID:assistant},"assistant":assistant,"documents":docs,"documents_by_path":documents_by_path,"active_document_id":active_document_id,"active_document_path":active_document_path,"capture_target_document_id":active_document_id,"capture_target_document_path":active_document_path,"new_input_available":input_latest>input_covered,"prompt_draft":state["prompt_draft"],"ask_requests":asks,"latest_ask_request":asks.first().cloned(),"recording_lease":voice_recording_lease::current(home).map_err(|error|OpError::new(error.code,error.message))?}),
    )
}

pub fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let assistant_id = string_arg(request, "assistant_id").unwrap_or_else(|| ASSISTANT_ID.into());
    if assistant_id != ASSISTANT_ID {
        return Err(OpError::new("not_found", "assistant not found"));
    }
    let patch = request
        .args
        .get("patch")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    assistant_state::load(home, &group_id).map_err(OpError::io)?;
    let before = store.load(&group_id).map_err(OpError::not_found)?;
    let was_enabled = before
        .extra
        .get(KEY)
        .map(effective_assistant)
        .and_then(|assistant| assistant["enabled"].as_bool())
        .unwrap_or(false);
    let actor_existed = before.actors.iter().any(|actor| actor.id == ACTOR_ID);
    let actor_secrets_before = actor_secrets::values(home, &group_id, ACTOR_ID)?;
    let foreman_id = before
        .actors
        .iter()
        .find(|actor| {
            cccc_core::actors::effective_role(&before, &actor.id) == Some(ActorRole::Foreman)
        })
        .map(|actor| actor.id.clone());
    let enabled = patch
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(was_enabled);
    if enabled
        && !was_enabled
        && !before.actors.iter().any(|actor| actor.id == ACTOR_ID)
        && !before.actors.iter().any(|actor| {
            cccc_core::actors::effective_role(&before, &actor.id) == Some(ActorRole::Foreman)
        })
    {
        return Err(OpError::new(
            "voice_secretary_foreman_missing",
            "Voice Secretary needs a foreman actor to inherit its runtime configuration",
        ));
    }
    if !enabled && was_enabled && before.actors.iter().any(|actor| actor.id == ACTOR_ID) {
        actor_delivery::shutdown_actor(&group_id, ACTOR_ID);
        let _ = actor_runtime::apply(home, &before, ACTOR_ID, "actor.stop");
    }
    let retiring_actor = !enabled && actor_existed;
    if retiring_actor {
        actor_secrets::remove(home, &group_id, ACTOR_ID)?;
    }
    let mutation = store.mutate(&group_id, |group| {
        if enabled && !group.actors.iter().any(|actor| actor.id == ACTOR_ID) {
            let mut actor = group
                .actors
                .iter()
                .find(|actor| {
                    cccc_core::actors::effective_role(group, &actor.id) == Some(ActorRole::Foreman)
                })
                .cloned()
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "foreman actor not found")
                })?;
            actor.id = ACTOR_ID.into();
            actor.role = None;
            actor.title = "Voice Secretary".into();
            actor.internal_kind = Some(ASSISTANT_ID.into());
            actor.enabled = true;
            actor.profile_id.clear();
            actor.profile_owner.clear();
            actor.created_at = utc_now();
            actor.updated_at = utc_now();
            group.actors.push(actor);
        } else if !enabled {
            group.actors.retain(|actor| actor.id != ACTOR_ID);
        }
        let state = group.extra.entry(KEY).or_insert_with(|| json!({}));
        if !state.is_object() {
            *state = json!({});
        }
        let root = state.as_object_mut().expect("assistant config initialized");
        let assistant = root.entry(ASSISTANT_ID).or_insert_with(|| {
            json!({
                "enabled":false,
                "config":default_assistant()["config"].clone()
            })
        });
        if !assistant.is_object() {
            *assistant = json!({});
        }
        assistant["enabled"] = json!(enabled);
        if let Some(config) = patch.get("config").and_then(Value::as_object) {
            let target = assistant
                .as_object_mut()
                .expect("assistant config initialized")
                .entry("config")
                .or_insert_with(|| default_assistant()["config"].clone());
            if !target.is_object() {
                *target = json!({});
            }
            settings::merge(
                target
                    .as_object_mut()
                    .expect("assistant config map initialized"),
                config,
            );
        }
        Ok(())
    });
    if let Err(error) = mutation {
        if retiring_actor {
            actor_secrets::replace(home, &group_id, ACTOR_ID, actor_secrets_before.clone())?;
        }
        return Err(OpError::io(error));
    }
    let after = store.load(&group_id).map_err(OpError::not_found)?;
    if enabled && !actor_existed {
        if let Some(foreman_id) = foreman_id {
            let secrets = actor_secrets::values(home, &group_id, &foreman_id)?;
            if !secrets.is_empty() {
                let mut forwarded = request.clone();
                forwarded.args.insert("actor_id".into(), json!(ACTOR_ID));
                forwarded.args.insert(
                    "set".into(),
                    serde_json::to_value(secrets).map_err(OpError::invalid)?,
                );
                actor_secrets::update(home, &forwarded)?;
            }
        }
    }
    let actor_started = if enabled && after.running {
        match actor_runtime::apply(home, &after, ACTOR_ID, "actor.start") {
            Ok(Some(status)) if status.running => true,
            Ok(None)
                if after
                    .actors
                    .iter()
                    .find(|actor| actor.id == ACTOR_ID)
                    .is_some_and(actor_runtime::is_structured) =>
            {
                true
            }
            Ok(_) => {
                rollback_enable(home, &store, &before, actor_secrets_before)?;
                return Err(OpError::new(
                    "voice_secretary_start_failed",
                    "Voice Secretary actor did not remain running",
                ));
            }
            Err(error) => {
                let mut failure = OpError::new("voice_secretary_start_failed", error.message);
                failure.details = error.details;
                failure
                    .details
                    .insert("runtime_code".into(), json!(error.code));
                rollback_enable(home, &store, &before, actor_secrets_before)?;
                return Err(failure);
            }
        }
    } else {
        false
    };
    let runtime_state = assistant_state::update(home, &group_id, |state| {
        let legacy = state.get(ASSISTANT_ID).cloned();
        let assistant = state
            .entry("assistant")
            .or_insert_with(|| legacy.unwrap_or_else(default_assistant));
        assistant["lifecycle"] = json!(if enabled { "idle" } else { "disabled" });
        assistant["updated_at"] = json!(utc_now());
        let assistant = assistant.clone();
        state.insert(ASSISTANT_ID.into(), assistant);
        Ok(Value::Object(state.clone()))
    })
    .map_err(OpError::io)?;
    let assistant = project_actor_runtime(&after, effective_assistant(&runtime_state));
    let event = append_event(
        home,
        &group_id,
        "assistant.settings.update",
        request,
        json!({"assistant_id":ASSISTANT_ID,"patch":patch,"actor_started":actor_started,"actor_start_error":null}),
    )?;
    object(
        json!({"group_id":group_id,"assistant":assistant,"event":event,"actor_started":actor_started,"actor_start_error":null}),
    )
}

fn rollback_enable(
    home: &HomeLayout,
    store: &GroupStore,
    before: &cccc_core::GroupDoc,
    secrets: std::collections::BTreeMap<String, String>,
) -> Result<(), OpError> {
    actor_delivery::shutdown_actor(&before.group_id, ACTOR_ID);
    if let Ok(current) = store.load(&before.group_id) {
        let _ = actor_runtime::apply(home, &current, ACTOR_ID, "actor.stop");
    }
    store
        .mutate(&before.group_id, |group| {
            match before.extra.get(KEY) {
                Some(state) => {
                    group.extra.insert(KEY.into(), state.clone());
                }
                None => {
                    group.extra.remove(KEY);
                }
            }
            group.actors.retain(|actor| actor.id != ACTOR_ID);
            if let Some((index, actor)) = before
                .actors
                .iter()
                .enumerate()
                .find(|(_, actor)| actor.id == ACTOR_ID)
            {
                group
                    .actors
                    .insert(index.min(group.actors.len()), actor.clone());
            }
            Ok(())
        })
        .map_err(OpError::io)?;
    actor_secrets::replace(home, &before.group_id, ACTOR_ID, secrets)
}

pub fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lifecycle = string_arg(request, "lifecycle").unwrap_or_else(|| "idle".into());
    let health = request
        .args
        .get("health")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let state = assistant_state::update(home, &group_id, |state| {
        let legacy = state.get(ASSISTANT_ID).cloned();
        let assistant = state
            .entry("assistant")
            .or_insert_with(|| legacy.unwrap_or_else(default_assistant));
        assistant["lifecycle"] = json!(lifecycle);
        assistant["health"] = health.clone();
        assistant["updated_at"] = json!(utc_now());
        let assistant = assistant.clone();
        state.insert(ASSISTANT_ID.into(), assistant);
        Ok(Value::Object(state.clone()))
    })
    .map_err(OpError::io)?;
    let assistant = effective_assistant(&state);
    let event = append_event(
        home,
        &group_id,
        "assistant.status.update",
        request,
        json!({"assistant_id":ASSISTANT_ID,"lifecycle":lifecycle,"health":health}),
    )?;
    object(json!({"group_id":group_id,"assistant":assistant,"event":event}))
}

fn append_event(
    home: &HomeLayout,
    group_id: &str,
    kind: &str,
    request: &DaemonRequest,
    data: Value,
) -> Result<Event, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let mut event = Event::new(kind, group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(&store.ledger_path(group_id).map_err(OpError::io)?, &event)
        .map_err(OpError::io)?;
    Ok(event)
}
pub fn effective_assistant(state: &Value) -> Value {
    let candidate = state
        .get("assistant")
        .cloned()
        .or_else(|| state.get(ASSISTANT_ID).cloned())
        .unwrap_or_else(|| json!({}));
    let mut assistant = default_assistant();
    if let (Some(target), Some(source)) = (assistant.as_object_mut(), candidate.as_object()) {
        settings::merge(target, source);
    }
    assistant
}
pub fn default_assistant() -> Value {
    json!({"assistant_id":ASSISTANT_ID,"kind":ASSISTANT_ID,"enabled":false,"principal":"assistant:voice_secretary","lifecycle":"disabled","health":{},"policy":{"action_allowlist":["voice_secretary.request"],"requires_user_confirmation":[]},"config":{"capture_mode":"browser","recognition_backend":"browser_asr","recognition_language":"auto","retention_ttl_seconds":900,"auto_document_enabled":true,"document_default_dir":"docs/voice-secretary","auto_document_quiet_ms":5000,"auto_document_min_chars":700,"auto_document_max_window_seconds":300,"service_model_id":"","service_diarization_model_id":"","tts_enabled":false},"ui":{"surface":"composer_quick_strip","composer_control":"voice_secretary_workspace","title":"Voice Secretary"}})
}

fn project_actor_runtime(group: &cccc_core::GroupDoc, mut assistant: Value) -> Value {
    let actor = group.actors.iter().find(|actor| actor.id == ACTOR_ID);
    let status = actor_runtime::status(&group.group_id, ACTOR_ID);
    let local_headless = actor.is_some_and(super::super::local_headless::supports);
    let headless_status = local_headless
        .then(|| super::super::local_headless::status(&group.group_id, ACTOR_ID))
        .flatten();
    let running = actor.is_some_and(|actor| {
        if local_headless {
            headless_status.is_some()
        } else if actor_runtime::is_structured(actor) {
            actor.enabled && group.running
        } else {
            status.as_ref().is_some_and(|status| status.running)
        }
    });
    let enabled = assistant["enabled"].as_bool().unwrap_or(false);
    let lifecycle = assistant["lifecycle"].as_str().unwrap_or("idle").to_owned();
    assistant["health"]["actor"] = json!({
        "configured": actor.is_some(),
        "running": running,
        "pid": if local_headless {
            headless_status.as_ref().and_then(|status| status.pid)
        } else {
            status.as_ref().and_then(|status| status.pid)
        },
        "exit_code": if local_headless { None } else {
            status.as_ref().and_then(|status| status.exit_code)
        },
    });
    if !enabled {
        assistant["lifecycle"] = json!("disabled");
    } else if running && !matches!(lifecycle.as_str(), "working" | "waiting") {
        assistant["lifecycle"] = json!("running");
    } else if !running {
        assistant["lifecycle"] = json!(if group.running { "failed" } else { "idle" });
    }
    assistant
}
