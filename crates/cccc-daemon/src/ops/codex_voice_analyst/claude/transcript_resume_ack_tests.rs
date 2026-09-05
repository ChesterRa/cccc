use super::{tests::harness, *};

fn resume_ack() -> Value {
    json!({
        "type":"assistant", "sessionId":"session-1", "isApiErrorMessage":false,
        "parentUuid":"resume-meta", "uuid":"resume-ack",
        "message":{"model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}
    })
}

#[test]
fn resume_ack_does_not_disconnect_or_consume_the_pending_user_message() {
    let (mut state, mut events) = harness();
    state
        .ingest(
            &json!({
                "type":"user","sessionId":"session-1","isMeta":true,"uuid":"resume-meta",
                "message":{"content":"Continue from where you left off."}
            }),
            None,
        )
        .expect("ingest transcript record");
    let pending = || PendingPrompt {
        delegation_id: "delivery-1",
        text: "start development",
        turn_id: "controlled-1",
    };
    assert_eq!(
        state
            .ingest(&resume_ack(), Some(pending()))
            .expect("ingest transcript record"),
        None
    );
    assert!(state.active_turn_id().is_none());
    assert!(events.try_recv().is_err());

    assert_eq!(
        state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1","promptId":"prompt-1",
                    "message":{"content":"start development"}
                }),
                Some(pending())
            )
            .expect("ingest transcript record")
            .as_deref(),
        Some("controlled-1")
    );
    let started = events.try_recv().expect("receive queued event");
    assert_eq!(started.message["method"], "turn/started");
    assert_eq!(
        started.requested_delegation_id.as_deref(),
        Some("delivery-1")
    );

    // A delayed acknowledgement must not become text in an already active turn either.
    state
        .ingest(&resume_ack(), None)
        .expect("ingest transcript record");
    assert_eq!(state.active_turn_id(), Some("controlled-1"));
    assert!(events.try_recv().is_err());
    state
        .ingest(
            &json!({
                "type":"assistant","sessionId":"session-1",
                "message":{"model":"claude","content":[{"type":"text","text":"ready"}]}
            }),
            None,
        )
        .expect("ingest transcript record");
    state
        .ingest(
            &json!({"type":"system","sessionId":"session-1","subtype":"turn_duration"}),
            None,
        )
        .expect("ingest transcript record");
    assert_eq!(
        events.try_recv().expect("receive queued event").message["params"]["delta"],
        "ready"
    );
    assert_eq!(
        events.try_recv().expect("receive queued event").message["params"]["item"]["text"],
        "ready"
    );
    assert_eq!(
        events.try_recv().expect("receive queued event").message["params"]["turn"]["status"],
        "completed"
    );
    assert!(state.active_turn_id().is_none());
    assert!(events.try_recv().is_err());
}

#[test]
fn resume_ack_exception_preserves_session_and_orphan_output_guards() {
    for field in [
        "real_model",
        "api_error",
        "error_field",
        "other_text",
        "extra_block",
        "foreign_session",
    ] {
        let (mut state, _) = harness();
        let mut record = resume_ack();
        match field {
            "real_model" => record["message"]["model"] = json!("claude"),
            "api_error" => record["isApiErrorMessage"] = json!(true),
            "error_field" => record["error"] = json!("provider_error"),
            "other_text" => record["message"]["content"][0]["text"] = json!("unexpected output"),
            "extra_block" => record["message"]["content"]
                .as_array_mut()
                .expect("complete as array mut in fixture")
                .push(json!({"type":"tool_use","id":"tool-1"})),
            "foreign_session" => record["sessionId"] = json!("another-session"),
            _ => unreachable!(),
        }
        assert!(
            state.ingest(&record, None).is_err(),
            "{field} must still fail closed"
        );
    }
}
