use super::*;

#[test]
fn grok_duplicate_terminals_cannot_settle_a_later_internal_turn() {
    let (events, mut receiver) = broadcast::channel(8);
    let mut active = Some(ActiveTurn {
        turn_id: "cccc-turn-b".into(),
        external: false,
        admitted: true,
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
