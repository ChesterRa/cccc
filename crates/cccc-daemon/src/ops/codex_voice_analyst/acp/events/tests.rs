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
        &[],
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
        &[],
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
        &[],
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

/// A stop reason is matched without regard to case or surrounding space, but
/// redacted as the provider sent it: a lowercased or trimmed copy would no
/// longer match a private value.
#[test]
fn an_unrecognized_stop_reason_is_redacted_as_the_provider_sent_it() {
    assert_eq!(
        status_from_stop_reason(Some(" End_Turn "), &[]),
        ("completed", None)
    );
    assert_eq!(
        status_from_stop_reason(Some("Canceled"), &[]),
        ("cancelled", None)
    );
    assert_eq!(
        status_from_stop_reason(Some("max_tokens"), &[]),
        ("failed", Some("ACP turn stopped: max_tokens".into()))
    );

    let private = "Stop-Reason-Secret\n".to_owned();
    let (events, mut receiver) = broadcast::channel(8);
    let mut active = Some(ActiveTurn {
        turn_id: "tui-turn-a".into(),
        external: true,
        admitted: true,
        provider_prompt_id: Some("provider-turn-a".into()),
        provider_start_sequence: None,
    });
    handle_notification(
        "_x.ai/session_notification",
        &json!({
            "params": {
                "sessionId": "session-1",
                "update": {
                    "sessionUpdate": "turn_completed",
                    "prompt_id": "provider-turn-a",
                    "stopReason": private
                }
            }
        }),
        &events,
        "generation-1",
        "session-1",
        &mut active,
        &mut HashMap::new(),
        std::slice::from_ref(&private),
    );
    let completed = std::iter::from_fn(|| receiver.try_recv().ok())
        .find(|event| event.message["method"] == "turn/completed")
        .expect("turn terminal");
    assert_eq!(completed.message["params"]["turn"]["status"], "failed");
    assert_eq!(
        completed.message["params"]["turn"]["error"],
        "ACP turn stopped: [redacted]"
    );
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
            &[],
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
            &[],
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

#[test]
fn tool_activity_observation_is_correlated_and_does_not_include_arguments() {
    let (events, mut receiver) = broadcast::channel(8);
    let mut active = Some(ActiveTurn {
        turn_id: "turn-1".into(),
        external: false,
        admitted: true,
        provider_prompt_id: Some("prompt-1".into()),
        provider_start_sequence: None,
    });
    let mut tools = HashMap::new();
    let update = json!({"params":{"sessionId":"session-1","_meta":{"promptId":"prompt-1"},"update":{"sessionUpdate":"tool_call","toolCallId":"tool-1","title":"Read the requested file","rawInput":{"private_fixture":"not-for-display"}}}});
    handle_notification(
        "session/update",
        &update,
        &events,
        "generation-1",
        "session-1",
        &mut active,
        &mut tools,
        &[],
    );
    let observed = receiver.try_recv().expect("tool activity");
    assert_eq!(observed.generation, "generation-1");
    assert_eq!(observed.message["method"], "cccc/toolActivity");
    assert_eq!(observed.message["params"]["turnId"], "turn-1");
    assert_eq!(
        observed.message["params"]["title"],
        "Read the requested file"
    );
    assert!(!observed.message.to_string().contains("not-for-display"));
    let mut replay = update.clone();
    replay["params"]["_meta"]["isReplay"] = json!(true);
    handle_notification(
        "session/update",
        &replay,
        &events,
        "generation-1",
        "session-1",
        &mut active,
        &mut tools,
        &[],
    );
    let mut wrong = update;
    wrong["params"]["_meta"]["promptId"] = json!("old-prompt");
    handle_notification(
        "session/update",
        &wrong,
        &events,
        "generation-1",
        "session-1",
        &mut active,
        &mut tools,
        &[],
    );
    assert!(receiver.try_recv().is_err());
}

#[test]
fn tool_activity_names_each_call_from_its_announcement_to_its_finish() {
    let (events, mut receiver) = broadcast::channel(16);
    let mut active = Some(ActiveTurn {
        turn_id: "turn-1".into(),
        external: false,
        admitted: true,
        provider_prompt_id: None,
        provider_start_sequence: None,
    });
    let mut tools = HashMap::new();
    let mut observed = Vec::new();
    let mut frame = |update: Value| {
        handle_notification(
            "session/update",
            &json!({"params":{"sessionId":"session-1","update":update}}),
            &events,
            "generation-1",
            "session-1",
            &mut active,
            &mut tools,
            &[],
        );
        let event = receiver.try_recv().expect("tool activity");
        assert_eq!(event.message["method"], "cccc/toolActivity");
        assert_eq!(event.message["params"]["turnId"], "turn-1");
        observed.push(event.message.clone());
        let params = &event.message["params"];
        (
            params["title"].as_str().unwrap_or_default().to_owned(),
            params["itemId"].as_str().map(str::to_owned),
            params["phase"].as_str().map(str::to_owned),
        )
    };
    let named = |title: &str, id: &str, phase: &str| {
        (
            title.to_owned(),
            Some(id.to_owned()),
            Some(phase.to_owned()),
        )
    };

    // DeepSeek Harness rc.5's recorded shape: an announcement already running,
    // then a finish that repeats neither the title nor the input.
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call","toolCallId":"call-read","title":"read","kind":"other","status":"in_progress","rawInput":{"path":"private-argument"}})
        ),
        named("read", "call-read", "started")
    );
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"call-read","status":"completed","rawOutput":{"output":"private-result"}})
        ),
        named("read", "call-read", "completed")
    );
    // Progress between start and failure stays transient.
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call","toolCallId":"call-run","title":"Run cargo test","status":"pending"})
        ),
        named("Run cargo test", "call-run", "started")
    );
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"call-run","status":"in_progress"})
        ),
        ("Run cargo test".to_owned(), None, None)
    );
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"call-run","status":"failed"})
        ),
        named("Run cargo test", "call-run", "failed")
    );
    // An announcement that is already finished never reads as running.
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call","toolCallId":"call-grep","title":"Search notes","status":"completed"})
        ),
        named("Search notes", "call-grep", "completed")
    );
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call","toolCallId":"call-rm","title":"Remove cache","status":"failed"})
        ),
        named("Remove cache", "call-rm", "failed")
    );
    // Kilo's OpenCode event stream sends updates only: a finish with its own
    // title and identity stands alone, and its other states start nothing.
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"call-edit","title":"edit","status":"completed","rawInput":{"filePath":"private-argument"},"rawOutput":{"output":"private-result","metadata":{}}})
        ),
        named("edit", "call-edit", "completed")
    );
    for status in ["pending", "running", "error"] {
        assert_eq!(
            frame(
                json!({"sessionUpdate":"tool_call_update","toolCallId":"call-bash","title":"bash","status":status})
            ),
            ("bash".to_owned(), None, None),
            "{status}"
        );
    }
    // A status outside ACP's vocabulary does not start an announced call either.
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call","toolCallId":"call-odd","title":"odd","status":"running"})
        ),
        ("odd".to_owned(), None, None)
    );
    // An announcement without a status is pending. A title at the bound stays
    // whole; one past it is replaced whole, so no cut can split a private value.
    let bound = "x".repeat(400);
    assert_eq!(
        frame(json!({"sessionUpdate":"tool_call","toolCallId":"call-bound","title":bound})),
        named(&bound, "call-bound", "started")
    );
    assert_eq!(
        frame(
            json!({"sessionUpdate":"tool_call","toolCallId":"call-long","title":format!("{bound}private-token")})
        ),
        named("Tool call", "call-long", "started")
    );

    assert!(receiver.try_recv().is_err(), "one observation per frame");
    let all = serde_json::to_string(&observed).expect("observations");
    assert!(!all.contains("private"), "{all}");
}
