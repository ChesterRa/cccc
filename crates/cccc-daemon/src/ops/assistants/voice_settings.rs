use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, assistant_state, settings, voice_recording_lease};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

use super::voice_document_state;

const ASSISTANT_ID: &str = "voice_secretary";

pub fn index(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let assistant_id = string_arg(request, "assistant_id").unwrap_or_default();
    if !assistant_id.is_empty() && assistant_id != ASSISTANT_ID {
        return Err(OpError::new("assistant_not_found", "assistant not found"));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    store.load(&group_id).map_err(OpError::not_found)?;
    let secretary =
        crate::ops::voice_secretary::group_projection(home, &group_id).map_err(OpError::io)?;
    if secretary["readiness_code"] == "invalid_configuration" && secretary["configured"] == false {
        return Err(OpError::new(
            "invalid_configuration",
            secretary["readiness_error"]
                .as_str()
                .unwrap_or("Secretary settings are unreadable"),
        ));
    }
    // Reconcile only after rejecting unreadable instance preferences. Its
    // legacy document read also projects those preferences into workflow state.
    super::document_reconcile::run(home, request)?;
    let state = assistant_state::load(home, &group_id).map_err(OpError::io)?;
    let prompt_draft =
        match string_arg(request, "prompt_request_id").filter(|id| !id.trim().is_empty()) {
            Some(id) => state["voice_prompt_drafts"]
                .get(id.trim())
                .filter(|draft| matches!(draft["status"].as_str(), Some("pending" | "no_change")))
                .cloned()
                .unwrap_or(Value::Null),
            None => state["prompt_draft"].clone(),
        };
    let document_state = voice_document_state::load(home, &group_id).map_err(OpError::io)?;
    let mut assistant = effective_assistant(&state);
    assistant["health"]
        .as_object_mut()
        .expect("assistant health")
        .remove("actor");
    assistant["health"]["secretary"] = secretary.clone();
    assistant["lifecycle"] = json!(if !assistant["enabled"].as_bool().unwrap_or(false) {
        "disabled"
    } else if secretary["busy"] == true {
        "working"
    } else if secretary["ready"] != true {
        if secretary["readiness_code"] == "not_configured" {
            "unconfigured"
        } else {
            "unavailable"
        }
    } else {
        "ready"
    });
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
        json!({"group_id":group_id,"assistants":[assistant],"assistants_by_id":{ASSISTANT_ID:assistant},"assistant":assistant,"documents":docs,"documents_by_path":documents_by_path,"active_document_id":active_document_id,"active_document_path":active_document_path,"capture_target_document_id":active_document_id,"capture_target_document_path":active_document_path,"new_input_available":secretary["pending"].as_bool().unwrap_or(false),"secretary_tasks":secretary["tasks"],"prompt_draft":prompt_draft,"ask_requests":asks,"latest_ask_request":asks.first().cloned(),"recording_lease":voice_recording_lease::current(home).map_err(|error|OpError::new(error.code,error.message))?}),
    )
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
    json!({"assistant_id":ASSISTANT_ID,"kind":ASSISTANT_ID,"enabled":true,"principal":"assistant:voice_secretary","lifecycle":"disabled","health":{},"config":{"guidance":include_str!("../../../../../resources/voice-secretary-guidance.md"),"capture_mode":"browser","recognition_backend":"browser_asr","recognition_language":"auto","retention_ttl_seconds":900,"auto_document_enabled":true,"document_default_dir":"docs/voice-secretary","auto_document_quiet_ms":5000,"auto_document_min_chars":700,"auto_document_max_window_seconds":300,"service_model_id":"","service_diarization_model_id":"","tts_enabled":false},"ui":{"surface":"composer_quick_strip","composer_control":"voice_secretary_workspace","title":"Voice Secretary"}})
}
