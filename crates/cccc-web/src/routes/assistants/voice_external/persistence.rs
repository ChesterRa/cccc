use super::{AsrError, active::Active};
use crate::{
    AppState,
    routes::assistants::voice_ws_revision::{self, FinalRevision},
};
use cccc_contracts::DaemonRequest;
use serde_json::json;

pub(super) async fn checkpoints(
    state: &AppState,
    group: &str,
    active: &mut Active,
    recovering: bool,
) -> Result<(), AsrError> {
    if !active.persist {
        return Ok(());
    }
    let flush = recovering || active.completed;
    if !flush && !active.checkpoint_schedule.due() {
        return Ok(());
    }
    let mut pending: Vec<_> = active
        .transcript
        .segments()
        .filter(|segment| {
            !active.persisted.contains(&segment.id)
                && !segment.text.trim().is_empty()
                && (flush || segment.finalized)
        })
        .cloned()
        .collect();
    pending.sort_by_key(|segment| (segment.start_ms, segment.end_ms));
    for segment in pending {
        let args = json!({"group_id":group,"by":"user","session_id":active.session_id,
            "segment_id":format!("external-{}",segment.id.replace(':',"-")),
            "document_path":active.document_path,"text":segment.text,"language":active.language,
            "is_final":true,"flush":flush,"transcript_stage":"live","source_model_id":active.model,
            "start_ms":segment.start_ms,"end_ms":segment.end_ms,
            "trigger":{"trigger_kind":if recovering{"external_asr_recovery"}else{"external_asr_checkpoint"},
                "capture_mode":"service","recognition_backend":"external_provider_asr_streaming"}});
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state.client.call(&DaemonRequest {
                v: 1,
                op: "assistant_voice_transcript_append".into(),
                args: args.as_object().cloned().unwrap_or_default(),
            }),
        )
        .await
        .map_err(|_| persistence_error())?
        .map_err(|_| persistence_error())?;
        if !response.ok {
            return Err(persistence_error());
        }
        active.persisted.insert(segment.id);
    }
    active.checkpoint_schedule.flushed();
    Ok(())
}

/// Provider completion does not imply that the last checkpoint reached the daemon.
/// Stable segment IDs make the retry safe even when only its response was lost.
pub(super) async fn recover(
    state: &AppState,
    group: &str,
    active: &mut Active,
) -> serde_json::Value {
    let _ = checkpoints(state, group, active, true).await;
    final_event(state, group, active).await
}

pub(super) async fn final_event(
    state: &AppState,
    group: &str,
    active: &Active,
) -> serde_json::Value {
    let mut event =
        active
            .transcript
            .final_event(&active.model, active.completed, active.stop_seq.clone());
    if active.persist {
        let _ = voice_ws_revision::persist_final_revision(
            state,
            FinalRevision {
                group_id: group,
                client_session_id: &active.session_id,
                document_path: &active.document_path,
                language: &active.language,
                configured_model_id: &active.model,
                trigger_kind: "external_asr_stop",
            },
            &mut event,
        )
        .await;
    }
    event
}

fn persistence_error() -> AsrError {
    AsrError::new(
        "external_asr_persistence_failed",
        "The recognized transcript could not be saved; recording was stopped",
    )
}
