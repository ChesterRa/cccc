use super::*;

#[test]
fn speaker_analysis_admission_is_bounded() {
    let semaphore = reservation_semaphore();
    let first = semaphore
        .clone()
        .try_acquire_owned()
        .expect("first diarization reservation");
    assert!(semaphore.clone().try_acquire_owned().is_err());
    drop(first);
    assert!(semaphore.try_acquire_owned().is_ok());
}

#[test]
fn failure_uses_the_canonical_session_update_operation() {
    let request = completion_request(
        "g_test",
        "session-1",
        "docs/meeting.md",
        None,
        "diarization_failed",
        "model failed",
    );

    assert_eq!(request.op, "assistant_voice_session_update");
    assert_eq!(request.args["group_id"], "g_test");
    assert_eq!(request.args["session_id"], "session-1");
    assert_eq!(request.args["by"], "assistant:voice_secretary");
    let patch = &request.args["patch"];
    assert_eq!(patch["status"], "closed");
    assert_eq!(patch["document_path"], "docs/meeting.md");
    assert_eq!(patch["diarization_ready"], false);
    assert_eq!(patch["diarization_error"]["code"], "diarization_failed");
    assert_eq!(patch["error"], patch["diarization_error"]);
}

#[test]
fn successful_retry_requests_stale_failure_cleanup() {
    let result = json!({"speaker_segments":[{"speaker":"speaker-1"}]});

    let request = completion_request(
        "g_test",
        "session-1",
        "docs/meeting.md",
        Some(result.clone()),
        "",
        "",
    );

    let patch = &request.args["patch"];
    assert_eq!(patch["status"], "closed");
    assert_eq!(patch["diarization_ready"], true);
    assert_eq!(patch["diarization"], result);
    assert!(patch["error"].is_null());
    assert!(patch.get("diarization_error").is_none());
}
