use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Policy {
    Exact,
    FinalRevision,
    LiveCheckpoint,
}

pub(super) fn for_transcript(revision_only: bool, stage: &str) -> Policy {
    if revision_only {
        Policy::FinalRevision
    } else if stage == "live" {
        Policy::LiveCheckpoint
    } else {
        Policy::Exact
    }
}

pub(super) fn should_skip(values: &[Value], record: &Value, policy: Policy) -> bool {
    let session_id = record["session_id"].as_str().unwrap_or_default();
    let session_asr = |item: &Value| {
        item["session_id"] == session_id && item["kind"].as_str() == Some("asr_transcript")
    };
    match policy {
        Policy::Exact => false,
        Policy::FinalRevision => values.iter().any(session_asr),
        Policy::LiveCheckpoint => values.iter().any(|item| {
            session_asr(item)
                && item["trigger"]["recognition_backend"] == "assistant_service_local_asr_final"
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn final_and_live_race_settles_to_one_session_input_in_either_order() {
        let live = json!({
            "session_id":"session-1","kind":"asr_transcript",
            "trigger":{"recognition_backend":"assistant_service_local_asr_streaming"}
        });
        let final_input = json!({
            "session_id":"session-1","kind":"asr_transcript",
            "trigger":{"recognition_backend":"assistant_service_local_asr_final"}
        });

        assert!(should_skip(
            std::slice::from_ref(&live),
            &final_input,
            Policy::FinalRevision
        ));
        assert!(should_skip(
            std::slice::from_ref(&final_input),
            &live,
            Policy::LiveCheckpoint
        ));
        assert!(!should_skip(&[live], &final_input, Policy::Exact));
    }
}
