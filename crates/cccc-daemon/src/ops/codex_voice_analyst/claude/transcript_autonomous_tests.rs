use super::{tests::harness, *};

fn user(prompt_id: &str, text: &str) -> Value {
    json!({"type":"user","sessionId":"session-1","promptId":prompt_id,"message":{"content":text}})
}

fn turn_duration() -> Value {
    json!({"type":"system","sessionId":"session-1","subtype":"turn_duration","isMeta":false})
}

/// The hidden input Claude Code records when another Claude session messages this one. Claude
/// answers it in a turn of its own, without any terminal or CCCC input.
fn cross_session_message(prompt_id: &str) -> Value {
    json!({
        "type":"user","sessionId":"session-1","promptId":prompt_id,"isMeta":true,
        "origin":{"kind":"peer","from":"bridge:session_peer","name":"PEER"},
        "message":{"role":"user","content":[{"type":"text","text":
            "Another Claude session sent a message:\n<cross-session-message from=\"bridge:session_peer\">Thanks</cross-session-message>"}]}
    })
}

fn assistant(content: Value) -> Value {
    json!({"type":"assistant","sessionId":"session-1","message":{"content":content}})
}

fn drain(events: &mut broadcast::Receiver<AnalystEvent>) -> Vec<Value> {
    std::iter::from_fn(|| events.try_recv().ok().map(|event| event.message)).collect()
}

#[test]
fn reply_to_a_cross_session_message_is_tracked_instead_of_invalidating_the_session() {
    let (mut state, mut events) = harness();
    // An earlier CCCC-delivered turn completes normally.
    state
        .ingest(&user("delivered", "status?"), None)
        .expect("user");
    state
        .ingest(&assistant(json!([{"type":"text","text":"done"}])), None)
        .expect("assistant");
    state.ingest(&turn_duration(), None).expect("settle");
    drain(&mut events);

    // A peer session's message starts an autonomous turn that uses a tool and answers.
    assert_eq!(
        state
            .ingest(&cross_session_message("peer-prompt"), None)
            .expect("hidden input"),
        None
    );
    assert!(state.active_turn_id().is_none());
    assert!(events.try_recv().is_err());
    state
        .ingest(
            &assistant(json!([{"type":"tool_use","id":"tool-1","name":"TaskUpdate","input":{}}])),
            None,
        )
        .expect("autonomous assistant output");
    assert_eq!(state.active_turn_id(), Some("claude-peer-prompt"));
    state
        .ingest(
            &json!({
                "type":"user","sessionId":"session-1","promptId":"peer-prompt",
                "message":{"content":[{"type":"tool_result","tool_use_id":"tool-1","content":"ok"}]}
            }),
            None,
        )
        .expect("tool result");
    state
        .ingest(&assistant(json!([{"type":"text","text":"noted"}])), None)
        .expect("assistant text");
    state.ingest(&turn_duration(), None).expect("settle");

    let messages = drain(&mut events);
    assert_eq!(messages[0]["method"], "turn/started");
    assert_eq!(messages[0]["params"]["turn"]["id"], "claude-peer-prompt");
    let completed = messages.last().expect("completed turn");
    assert_eq!(completed["method"], "turn/completed");
    assert_eq!(completed["params"]["turn"]["status"], "completed");
    assert!(state.active_turn_id().is_none());

    // The session keeps accepting CCCC deliveries afterwards.
    assert_eq!(
        state
            .ingest(
                &user("next", "continue"),
                Some(PendingPrompt {
                    delegation_id: "next-delivery",
                    text: "continue",
                    turn_id: "next-turn",
                }),
            )
            .expect("next delivery")
            .as_deref(),
        Some("next-turn")
    );
}

#[test]
fn autonomous_turn_can_be_interrupted_by_its_hidden_prompt_id() {
    let (mut state, mut events) = harness();
    state
        .ingest(&cross_session_message("peer-prompt"), None)
        .expect("hidden input");
    state
        .ingest(&assistant(json!([{"type":"text","text":"working"}])), None)
        .expect("assistant");
    state
        .ingest(&user("peer-prompt", INTERRUPTION_MARKER), None)
        .expect("interruption of the autonomous turn");
    let ended = drain(&mut events).pop().expect("completed turn");
    assert_eq!(ended["params"]["turn"]["status"], "cancelled");
    assert!(state.active_turn_id().is_none());
}

#[test]
fn hidden_input_without_a_following_reply_opens_no_turn() {
    let (mut state, mut events) = harness();
    state
        .ingest(&cross_session_message("ignored"), None)
        .expect("hidden input");
    state.ingest(&turn_duration(), None).expect("settle");
    assert!(state.active_turn_id().is_none());
    assert!(events.try_recv().is_err());
    // The stale hidden prompt is not reused for a later CCCC turn.
    state
        .ingest(&user("delivered", "next"), None)
        .expect("user");
    assert_eq!(state.active_turn_id(), Some("claude-delivered"));
}

#[test]
fn hidden_input_during_a_turn_joins_it() {
    let (mut state, mut events) = harness();
    state
        .ingest(&user("delivered", "inspect"), None)
        .expect("user");
    state
        .ingest(&cross_session_message("peer-prompt"), None)
        .expect("hidden input");
    state
        .ingest(&assistant(json!([{"type":"text","text":"both"}])), None)
        .expect("assistant");
    assert_eq!(state.active_turn_id(), Some("claude-delivered"));
    state
        .ingest(&user("peer-prompt", TOOL_INTERRUPTION_MARKER), None)
        .expect("interruption carrying the hidden prompt id");
    let ended = drain(&mut events).pop().expect("completed turn");
    assert_eq!(ended["params"]["turn"]["status"], "cancelled");
}

#[test]
fn output_without_a_turn_or_hidden_input_still_fails_closed() {
    let (mut state, _) = harness();
    let error = state
        .ingest(&assistant(json!([{"type":"text","text":"orphan"}])), None)
        .expect_err("orphan output");
    assert!(
        error
            .to_string()
            .contains("without an active transcript turn")
    );
}

#[test]
fn settled_hidden_input_does_not_excuse_later_orphan_output() {
    let (mut state, _) = harness();
    state
        .ingest(&cross_session_message("stale"), None)
        .expect("hidden input");
    state.ingest(&turn_duration(), None).expect("settle");
    assert!(
        state
            .ingest(&assistant(json!([{"type":"text","text":"orphan"}])), None)
            .is_err()
    );
}

/// A scheduled (cron/loop) prompt fires while idle: hidden input, then Claude's own turn.
#[test]
fn scheduled_prompt_turn_is_tracked() {
    let (mut state, mut events) = harness();
    state
        .ingest(
            &json!({"type":"system","sessionId":"session-1","subtype":"scheduled_task_fire"}),
            None,
        )
        .expect("fire marker");
    state
        .ingest(
            &json!({
                "type":"user","sessionId":"session-1","promptId":"scheduled-1","isMeta":true,
                "promptSource":"system","turnOrigin":"scheduled",
                "message":{"role":"user","content":"Periodic check: report anything stalled."}
            }),
            None,
        )
        .expect("scheduled hidden input");
    state
        .ingest(
            &assistant(json!([{"type":"text","text":"nothing stalled"}])),
            None,
        )
        .expect("scheduled turn output");
    state.ingest(&turn_duration(), None).expect("settle");
    let messages = drain(&mut events);
    assert_eq!(messages[0]["params"]["turn"]["id"], "claude-scheduled-1");
    assert_eq!(
        messages.last().expect("completed")["params"]["turn"]["status"],
        "completed"
    );
}

#[test]
fn resume_acknowledgement_consumes_the_hidden_resume_prompt() {
    let (mut state, _) = harness();
    state
        .ingest(
            &json!({
                "type":"user","sessionId":"session-1","promptId":"resume","isMeta":true,
                "message":{"content":"Continue from where you left off."}
            }),
            None,
        )
        .expect("hidden resume prompt");
    state
        .ingest(
            &json!({
                "type":"assistant","sessionId":"session-1","isApiErrorMessage":false,
                "message":{"model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}
            }),
            None,
        )
        .expect("resume acknowledgement");
    assert!(
        state
            .ingest(&assistant(json!([{"type":"text","text":"orphan"}])), None)
            .is_err()
    );
}

#[test]
fn hidden_input_from_another_session_is_not_remembered() {
    let (mut state, _) = harness();
    let mut foreign = cross_session_message("foreign");
    foreign["sessionId"] = json!("another-session");
    state.ingest(&foreign, None).expect("foreign hidden input");
    assert!(
        state
            .ingest(&assistant(json!([{"type":"text","text":"orphan"}])), None)
            .is_err()
    );
}

/// Two hidden inputs can land before Claude answers (e.g. a held then denied delivery notice).
/// Either one may own an interruption of the turn Claude starts for them.
#[test]
fn every_pending_hidden_input_belongs_to_the_turn_it_starts() {
    let (mut state, mut events) = harness();
    state
        .ingest(&cross_session_message("first"), None)
        .expect("first hidden input");
    state
        .ingest(&cross_session_message("second"), None)
        .expect("second hidden input");
    assert!(state.hidden_input_pending());
    state
        .ingest(
            &assistant(json!([{"type":"text","text":"answering"}])),
            None,
        )
        .expect("assistant");
    assert!(!state.hidden_input_pending());
    assert_eq!(state.active_turn_id(), Some("claude-first"));
    state
        .ingest(&user("first", INTERRUPTION_MARKER), None)
        .expect("interruption carrying the earlier hidden prompt id");
    let ended = drain(&mut events).pop().expect("completed turn");
    assert_eq!(ended["params"]["turn"]["status"], "cancelled");
}
