use cccc_contracts::{ActorRole, DaemonRequest, Event, utc_now};
use cccc_core::{GroupStore, HomeLayout, ledger, settings};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use crate::ops::{actor_delivery, actor_runtime, actor_secrets};

use super::voice_document_state;

const KEY: &str = "assistants";
const ASSISTANT_ID: &str = "voice_secretary";
const ACTOR_ID: &str = "voice-secretary";

pub fn index(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store.load(&group_id).map_err(OpError::not_found)?;
    let state = group.extra.get(KEY).cloned().unwrap_or_else(|| json!({}));
    let assistant = project_actor_runtime(&group, effective_assistant(&state));
    let docs = state["documents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|document| voice_document_state::is_active(document))
        .cloned()
        .collect::<Vec<_>>();
    let asks = state["ask_requests"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let documents_by_path = docs
        .iter()
        .filter_map(|item| {
            item["document_path"]
                .as_str()
                .map(|path| (path.to_owned(), item.clone()))
        })
        .collect::<Map<_, _>>();
    let configured_active_id = state["active_document_id"].as_str().unwrap_or_default();
    let configured_active_path = state["active_document_path"].as_str().unwrap_or_default();
    let active_document =
        voice_document_state::resolved_active(&docs, configured_active_id, configured_active_path);
    let active_document_id = active_document
        .and_then(|document| document["document_id"].as_str())
        .unwrap_or_default();
    let active_document_path = active_document
        .and_then(|document| document["document_path"].as_str())
        .unwrap_or_default();
    object(
        json!({"group_id":group_id,"assistants":[assistant],"assistants_by_id":{ASSISTANT_ID:assistant},"assistant":assistant,"documents":docs,"documents_by_path":documents_by_path,"active_document_id":active_document_id,"active_document_path":active_document_path,"capture_target_document_id":active_document_id,"capture_target_document_path":active_document_path,"new_input_available":state["input_latest_seq"].as_u64().unwrap_or(0)>state["input_read_cursor"].as_u64().unwrap_or(0),"prompt_draft":state["prompt_draft"],"ask_requests":asks,"latest_ask_request":asks.first().cloned()}),
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
    let assistant = store
        .mutate(&group_id, |group| {
            if enabled && !group.actors.iter().any(|actor| actor.id == ACTOR_ID) {
                let mut actor = group
                    .actors
                    .iter()
                    .find(|actor| {
                        cccc_core::actors::effective_role(group, &actor.id)
                            == Some(ActorRole::Foreman)
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
            let root = state.as_object_mut().expect("assistant state initialized");
            let assistant = root.entry("assistant").or_insert_with(default_assistant);
            assistant["enabled"] = json!(enabled);
            assistant["lifecycle"] = json!(if enabled { "idle" } else { "disabled" });
            if let Some(config) = patch.get("config").and_then(Value::as_object) {
                let target = assistant
                    .get_mut("config")
                    .and_then(Value::as_object_mut)
                    .expect("assistant config initialized");
                settings::merge(target, config);
            }
            assistant["updated_at"] = json!(utc_now());
            let assistant = assistant.clone();
            root.insert(ASSISTANT_ID.into(), assistant.clone());
            Ok(assistant)
        })
        .map_err(OpError::io)?;
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
    } else if !enabled && actor_existed {
        let mut forwarded = request.clone();
        forwarded.args.insert("actor_id".into(), json!(ACTOR_ID));
        forwarded.args.insert("clear".into(), json!(true));
        actor_secrets::update(home, &forwarded)?;
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
    let assistant = project_actor_runtime(&after, assistant);
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
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let lifecycle = string_arg(request, "lifecycle").unwrap_or_else(|| "idle".into());
    let health = request
        .args
        .get("health")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let assistant = store
        .mutate(&group_id, |group| {
            let state = group.extra.entry(KEY).or_insert_with(|| json!({}));
            if !state.is_object() {
                *state = json!({});
            }
            let root = state.as_object_mut().expect("state");
            let legacy = root.get(ASSISTANT_ID).cloned();
            let assistant = root
                .entry("assistant")
                .or_insert_with(|| legacy.unwrap_or_else(default_assistant));
            assistant["lifecycle"] = json!(lifecycle);
            assistant["health"] = health.clone();
            assistant["updated_at"] = json!(utc_now());
            let assistant = assistant.clone();
            root.insert(ASSISTANT_ID.into(), assistant.clone());
            Ok(assistant)
        })
        .map_err(OpError::io)?;
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
    state
        .get("assistant")
        .cloned()
        .or_else(|| state.get(ASSISTANT_ID).cloned())
        .unwrap_or_else(default_assistant)
}
pub fn default_assistant() -> Value {
    json!({"assistant_id":ASSISTANT_ID,"kind":ASSISTANT_ID,"enabled":false,"principal":"assistant:voice_secretary","lifecycle":"disabled","health":{},"policy":{"action_allowlist":[],"requires_user_confirmation":[]},"config":{"capture_mode":"document","recognition_backend":"browser_asr","recognition_language":"auto","retention_ttl_seconds":604800,"auto_document_enabled":true,"document_default_dir":"docs/voice-secretary","auto_document_quiet_ms":1200,"auto_document_min_chars":80,"auto_document_max_window_seconds":30,"service_model_id":"sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8","tts_enabled":false},"ui":{"title":"Voice Secretary"}})
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
