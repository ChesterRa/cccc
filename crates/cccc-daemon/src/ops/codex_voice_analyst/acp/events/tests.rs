use super::*;

#[test]
fn grok_duplicate_terminals_cannot_settle_a_later_internal_turn() {
    let (events, mut receiver) = broadcast::channel(8);
    let mut active = Some(ActiveTurn {
        turn_id: "cccc-turn-b".into(),
        external: false,
        admitted: true,
        provider_prompt_id: None,
        provider_start_sequence: None,
    });
    let mut tool_calls = HashMap::new();

    handle_notification(
        "_x.ai/session_notification",
        &json!({
            "params": {
                "sessionId": "session-1",
                "update": {
                    "sessionUpdate": "turn_completed",
                    "prompt_id": "provider-turn-a",
                    "stopReason": "end_turn"
                }
            }
        }),
        &events,
        "generation-1",
        "session-1",
        &mut active,
        &mut tool_calls,
    );
    handle_notification(
        "_x.ai/session/prompt_complete",
        &json!({
            "params": {
                "sessionId": "session-1",
                "promptId": "provider-turn-a",
                "stopReason": "end_turn"
            }
        }),
        &events,
        "generation-1",
        "session-1",
        &mut active,
        &mut tool_calls,
    );

    assert_eq!(
        active.as_ref().map(|turn| turn.turn_id.as_str()),
        Some("cccc-turn-b")
    );
    assert!(receiver.try_recv().is_err());
}

#[test]
fn durable_terminal_settles_an_external_tui_turn() {
    let (events, mut receiver) = broadcast::channel(8);
    let mut active = Some(ActiveTurn {
        turn_id: "tui-turn-a".into(),
        external: true,
        admitted: true,
        provider_prompt_id: Some("provider-turn-a".into()),
        provider_start_sequence: None,
    });
    let mut tool_calls = HashMap::new();

    handle_notification(
        "_x.ai/session_notification",
        &json!({
            "params": {
                "sessionId": "session-1",
                "update": {
                    "sessionUpdate": "turn_completed",
                    "prompt_id": "provider-turn-a",
                    "stopReason": "cancelled"
                }
            }
        }),
        &events,
        "generation-1",
        "session-1",
        &mut active,
        &mut tool_calls,
    );

    assert!(active.is_none());
    assert_eq!(
        receiver.try_recv().expect("item terminal").message["method"],
        "item/completed"
    );
    let completed = receiver.try_recv().expect("turn terminal");
    assert_eq!(completed.message["method"], "turn/completed");
    assert_eq!(completed.message["params"]["turn"]["id"], "tui-turn-a");
    assert_eq!(completed.message["params"]["turn"]["status"], "cancelled");
}

#[test]
fn grok_persisted_cancel_releases_owned_turn_before_redirected_input() {
    let (events, mut receiver) = broadcast::channel(16);
    let mut active = Some(ActiveTurn {
        turn_id: "old-owned".into(),
        external: false,
        admitted: true,
        provider_prompt_id: None,
        provider_start_sequence: None,
    });
    let mut calls = HashMap::new();
    for message in [
        json!({"method":"session/update","params":{"sessionId":"session-1",
            "_meta":{"promptId":"provider-old"},
            "update":{"sessionUpdate":"agent_message_chunk","content":{"text":"Waiting"}}}}),
        json!({"method":"_x.ai/session/update","params":{"sessionId":"session-1",
            "update":{"sessionUpdate":"turn_completed","prompt_id":"provider-old","stop_reason":"cancelled"}}}),
    ] {
        handle_notification(
            message["method"].as_str().expect("fixture method"),
            &message,
            &events,
            "generation-1",
            "session-1",
            &mut active,
            &mut calls,
        );
    }
    assert!(
        active.is_none(),
        "durable cancellation must release the old turn before native input"
    );
    let completed = std::iter::from_fn(|| receiver.try_recv().ok())
        .find(|event| event.message["method"] == "turn/completed")
        .expect("completion");
    assert_eq!(completed.message["params"]["turn"]["id"], "old-owned");
    assert_eq!(completed.message["params"]["turn"]["status"], "cancelled");
}

#[test]
fn empty_grok_turn_uses_persisted_start_boundary_and_ignores_older_activity() {
    let (events, mut receiver) = broadcast::channel(16);
    let mut active = None;
    let mut calls = HashMap::new();
    let mut send = |message: Value| {
        handle_notification(
            message["method"].as_str().expect("fixture method"),
            &message,
            &events,
            "generation",
            "session-1",
            &mut active,
            &mut calls,
        )
    };
    send(
        json!({"method":"session/update","params":{"sessionId":"session-1",
        "_meta":{"eventId":"session-1-200"},
        "update":{"sessionUpdate":"user_message_chunk","content":{"text":"new input"}}}}),
    );
    send(
        json!({"method":"_x.ai/session/update","params":{"sessionId":"session-1",
        "_meta":{"eventId":"session-1-199"},
        "update":{"sessionUpdate":"turn_completed","prompt_id":"old","stop_reason":"end_turn"}}}),
    );
    send(
        json!({"method":"session/update","params":{"sessionId":"session-1",
        "_meta":{"eventId":"session-1-198","promptId":"old"},
        "update":{"sessionUpdate":"agent_message_chunk","content":{"text":"stale"}}}}),
    );
    send(
        json!({"method":"_x.ai/session/update","params":{"sessionId":"session-1",
        "_meta":{"eventId":"session-1-201"},
        "update":{"sessionUpdate":"turn_completed","prompt_id":"new","stop_reason":"cancelled"}}}),
    );
    assert!(active.is_none());
    let observed = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert_eq!(
        observed.len(),
        3,
        "only start, item completion and turn completion"
    );
    assert_eq!(observed[2].message["params"]["turn"]["status"], "cancelled");
}
