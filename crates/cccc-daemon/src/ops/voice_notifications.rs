use crate::dispatch::{OpError, OpResult, object};
use cccc_contracts::DaemonRequest;
use cccc_contracts::voice_notifications::{VoiceMessageRef, VoicePreferences};
use cccc_core::{HomeLayout, voice_notifications as voice};
use serde_json::json;

pub(super) fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "voice_preferences_get" => voice::preferences(home)
            .map_err(OpError::io)
            .and_then(|preferences| object(json!({"preferences":preferences}))),
        "voice_preferences_set" => save(home, request),
        "voice_messages_viewed" => viewed(home, request),
        "voice_notifications_get" => voice::public_snapshot(home)
            .map_err(OpError::io)
            .and_then(object),
        _ => return None,
    })
}

fn save(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let preferences: VoicePreferences =
        serde_json::from_value(request.args.get("preferences").cloned().unwrap_or_default())
            .map_err(OpError::invalid)?;
    let preferences = voice::save_preferences(home, preferences).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            OpError::new("voice_preferences_conflict", error.to_string())
        } else {
            OpError::io(error)
        }
    })?;
    object(json!({"preferences":preferences}))
}

fn viewed(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let messages: Vec<VoiceMessageRef> =
        serde_json::from_value(request.args.get("messages").cloned().unwrap_or_default())
            .map_err(OpError::invalid)?;
    voice::mark_viewed(home, &messages).map_err(OpError::io)?;
    object(json!({"observed":messages.len()}))
}
