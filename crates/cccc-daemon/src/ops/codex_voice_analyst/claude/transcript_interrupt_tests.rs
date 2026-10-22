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

        // A marker under the previous turn's prompt ids cannot cancel the next turn, and it
        // does not end the session either.
        state
            .ingest(&user("next", "next request"), None)
            .expect("ingest transcript record");
        state
            .ingest(&user("followup", TOOL_INTERRUPTION_MARKER), None)
            .expect("a stale marker settles nothing");
        assert_eq!(state.active_turn_id(), Some("claude-next"));
    }
}

/// Real Claude Code 2.1.296 transcript records (2026-10-21): a typed prompt opens a turn;
/// while it runs the user queues "retry"; Claude Code aborts the turn and writes the
/// interruption marker under the queued prompt's id, then the queued prompt itself. Every
/// line is as captured (ids, timestamps, key order); only the working directory, the git
/// branch and the opening prompt's text read "[redacted]".
const QUEUED_INPUT_INTERRUPT: &str = include_str!("transcript_queued_interrupt.jsonl");

#[test]
fn input_queued_during_a_turn_cancels_it_and_opens_the_next() {
    let records = QUEUED_INPUT_INTERRUPT
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("captured record"))
        .collect::<Vec<_>>();
    let session_id = records[0]["sessionId"].as_str().expect("session id");
    let (sender, mut events) = broadcast::channel(32);
    let mut state = TranscriptState::new("generation-1".into(), session_id.into(), sender);
    for record in &records {
        state
            .ingest(record, None)
            .expect("the interruption keeps the session");
    }
    let messages = std::iter::from_fn(|| events.try_recv().ok())
        .map(|event| event.message)
        .collect::<Vec<_>>();
    let turns = messages
        .iter()
        .filter(|message| {
            matches!(
                message["method"].as_str(),
                Some("turn/started" | "turn/completed")
            )
        })
        .map(|message| {
            let turn = &message["params"]["turn"];
            let method = message["method"].as_str();
            (method, turn["id"].as_str(), turn["status"].as_str())
        })
        .collect::<Vec<_>>();
    let interrupted = Some("claude-327a5c53-0d23-4c91-bae2-4e575086e6af");
    let queued = Some("claude-7e73a7fa-8ae4-4bb0-92de-ef84d697f5ed");
    assert_eq!(
        turns,
        [
            (Some("turn/started"), interrupted, None),
            (Some("turn/completed"), interrupted, Some("cancelled")),
            (Some("turn/started"), queued, None),
        ]
    );
    assert_eq!(state.active_turn_id(), queued);
}
