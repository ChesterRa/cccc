use super::{tests::harness, *};

fn user(prompt_id: &str, text: &str) -> Value {
    json!({"type":"user","sessionId":"session-1","promptId":prompt_id,"message":{"content":text}})
}

#[test]
fn both_interrupt_markers_release_the_turn_and_accept_the_next_delivery() {
    for marker in [INTERRUPTION_MARKER, TOOL_INTERRUPTION_MARKER] {
        let (mut state, mut events) = harness();
        state
            .ingest(&user("initial", "inspect"), None)
            .expect("ingest transcript record");
        events.try_recv().expect("receive queued event");
        state
            .ingest(&user("initial", marker), None)
            .expect("ingest transcript record");
        events.try_recv().expect("receive queued event"); // completed message
        let ended = events.try_recv().expect("receive queued event");
        assert_eq!(ended.message["params"]["turn"]["status"], "cancelled");
        assert!(state.active_turn_id().is_none());
        assert_eq!(
            state
                .ingest(
                    &user("next", "continue"),
                    Some(PendingPrompt {
                        delegation_id: "next-delivery",
                        text: "continue",
                        turn_id: "next-turn"
                    })
                )
                .expect("ingest transcript record")
                .as_deref(),
            Some("next-turn")
        );
        assert_eq!(
            events
                .try_recv()
                .expect("receive queued event")
                .requested_delegation_id
                .as_deref(),
            Some("next-delivery")
        );
    }
}

#[test]
fn interruption_of_an_admitted_followup_cancels_the_same_turn() {
    for native in [false, true] {
        let (mut state, mut events) = harness();
        state
            .ingest(&user("initial", "inspect"), None)
            .expect("ingest transcript record");
        events.try_recv().expect("receive queued event");
        let outcome = state
            .ingest_with_native(
                &user("followup", "new constraint"),
                None,
                native.then_some(PendingNativeInput {
                    delegation_id: "followup-delivery",
                    text: "new constraint",
                }),
            )
            .expect("complete ingest with native in fixture");
        if native {
            assert_eq!(outcome, IngestOutcome::Native("followup-delivery".into()));
            assert_eq!(
                events.try_recv().expect("receive queued event").message["params"]["turnId"],
                "claude-initial"
            );
        }
        state
            .ingest(&user("followup", INTERRUPTION_MARKER), None)
            .expect("ingest transcript record");
        events.try_recv().expect("receive queued event");
        let ended = events.try_recv().expect("receive queued event");
        assert_eq!(ended.message["params"]["turn"]["id"], "claude-initial");
        assert_eq!(ended.message["params"]["turn"]["status"], "cancelled");
        assert!(state.active_turn_id().is_none());

        // Old or unadmitted prompt ids cannot cancel a subsequent turn.
        state
            .ingest(&user("next", "next request"), None)
            .expect("ingest transcript record");
        assert!(
            state
                .ingest(&user("followup", TOOL_INTERRUPTION_MARKER), None)
                .is_err()
        );
        assert_eq!(state.active_turn_id(), Some("claude-next"));
    }
}
