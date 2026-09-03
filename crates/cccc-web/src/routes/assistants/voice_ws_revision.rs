use cccc_contracts::DaemonRequest;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::AppState;

pub(super) struct FinalRevision<'a> {
    pub(super) group_id: &'a str,
    pub(super) client_session_id: &'a str,
    pub(super) document_path: &'a str,
    pub(super) language: &'a str,
    pub(super) configured_model_id: &'a str,
    pub(super) trigger_kind: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PersistenceStatus {
    Persisted,
    SkippedPartial,
    NotApplicable,
    Failed,
}

pub(super) async fn persist_final_revision(
    state: &AppState,
    revision: FinalRevision<'_>,
    final_asr: &mut Value,
) -> PersistenceStatus {
    if final_asr["partial"].as_bool().unwrap_or(false) {
        annotate(final_asr, "skipped_partial", false, None);
        return PersistenceStatus::SkippedPartial;
    }
    let Some(args) = final_revision_args(
        revision.group_id,
        revision.client_session_id,
        revision.document_path,
        revision.language,
        revision.configured_model_id,
        revision.trigger_kind,
        final_asr,
    ) else {
        return PersistenceStatus::NotApplicable;
    };
    match state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: "assistant_voice_transcript_append".into(),
            args,
        })
        .await
    {
        Ok(response) if response.ok => {
            annotate(final_asr, "persisted", true, None);
            PersistenceStatus::Persisted
        }
        Ok(response) => {
            tracing::warn!(?response.error, "final SenseVoice revision was rejected");
            let error = response.error.map_or_else(
                || json!({"code":"daemon_rejected","message":"daemon rejected final transcript"}),
                |error| json!({"code":error.code,"message":error.message}),
            );
            annotate(final_asr, "failed", false, Some(error));
            PersistenceStatus::Failed
        }
        Err(error) => {
            tracing::warn!(%error, "final SenseVoice revision could not be persisted");
            annotate(
                final_asr,
                "failed",
                false,
                Some(json!({
                    "code":"daemon_unavailable",
                    "message":"final transcript could not be persisted"
                })),
            );
            PersistenceStatus::Failed
        }
    }
}

fn annotate(final_asr: &mut Value, status: &str, persisted: bool, error: Option<Value>) {
    final_asr["transcript_persistence"] = json!(status);
    final_asr["transcript_persisted"] = json!(persisted);
    if let Some(error) = error {
        final_asr["transcript_persistence_error"] = error;
    }
}

fn final_revision_args(
    group_id: &str,
    client_session_id: &str,
    document_path: &str,
    language: &str,
    configured_model_id: &str,
    trigger_kind: &str,
    final_asr: &Value,
) -> Option<Map<String, Value>> {
    let text = final_asr["text"].as_str()?.trim();
    if final_asr["ok"] == false
        || final_asr["partial"].as_bool().unwrap_or(false)
        || text.is_empty()
    {
        return None;
    }
    let session_id = if client_session_id.trim().is_empty() {
        format!("ws_{}", Uuid::new_v4().simple())
    } else {
        client_session_id.trim().to_owned()
    };
    let model_id = final_asr["model_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(configured_model_id)
        .trim();
    json!({
        "group_id":group_id,
        "by":"user",
        "session_id":session_id,
        "segment_id":"final-asr",
        "document_path":document_path,
        "text":text,
        "language":language,
        "is_final":true,
        "flush":true,
        "transcript_stage":"final",
        "revision_only":true,
        "supersede_stage":"live",
        "source_model_id":model_id,
        "trigger":{
            "trigger_kind":trigger_kind,
            "capture_mode":"service",
            "recognition_backend":"assistant_service_local_asr_final",
            "final_model_id":model_id,
        }
    })
    .as_object()
    .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_revision_is_revision_only_and_supersedes_live_segments() {
        let args = final_revision_args(
            "g_test",
            "session-1",
            "docs/voice-secretary/meeting.md",
            "zh-CN",
            "sense-default",
            "push_to_talk_stop",
            &json!({"ok":true,"text":"最终文本。","model_id":"sense-actual"}),
        )
        .expect("revision args");

        assert_eq!(args["transcript_stage"], "final");
        assert_eq!(args["revision_only"], true);
        assert_eq!(args["supersede_stage"], "live");
        assert_eq!(args["source_model_id"], "sense-actual");
        assert_eq!(args["trigger"]["trigger_kind"], "push_to_talk_stop");
        assert_eq!(
            args["trigger"]["recognition_backend"],
            "assistant_service_local_asr_final"
        );
        assert!(
            final_revision_args(
                "g_test",
                "session-1",
                "docs/voice-secretary/meeting.md",
                "zh-CN",
                "sense-default",
                "push_to_talk_stop",
                &json!({"ok":true,"partial":true,"text":"部分终稿"}),
            )
            .is_none(),
            "partial final ASR must preserve the complete live transcript"
        );
    }
}
