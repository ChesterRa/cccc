use super::*;

#[test]
fn shared_vectors_cover_valid_and_invalid_frames() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/deepseek_acp_vectors.json"
    )))
    .expect("fixture");
    assert_eq!(
        fixture["protocol_version"],
        cccc_contracts::DEEPSEEK_PROTOCOL_VERSION
    );
    assert_eq!(
        fixture["acp_sdk_version"],
        cccc_contracts::DEEPSEEK_ACP_SDK_VERSION
    );
    let mut session = NdjsonSession::default();
    session.register(&json!(1)).expect("register response");
    for frame in fixture["frames"].as_array().expect("frames") {
        let line = frame["line"].as_str().expect("line");
        let result = session.feed_line(line.as_bytes());
        assert_eq!(
            result.is_ok(),
            frame["valid"].as_bool().expect("valid"),
            "{}",
            frame["name"]
        );
    }
    let cancelled = &fixture["cancelled_terminal"]["frame"];
    assert_eq!(terminal_stop_reason(cancelled), Some("cancelled"));
    assert_ne!(terminal_stop_reason(cancelled), Some("end_turn"));
    assert_eq!(
        fixture["update_idempotency"]["dedupe_key"],
        "deepseek.update:event-1:{attempt}:{ordinal}"
    );
    assert_eq!(fixture["update_idempotency"]["expected_durable_updates"], 2);
}

#[test]
fn handshake_and_permission_shapes_are_minimal() {
    let initialize = initialize_request("0.4.34");
    assert_eq!(
        initialize["params"]["protocolVersion"],
        DEEPSEEK_PROTOCOL_VERSION
    );
    assert!(
        initialize["params"]["clientCapabilities"]
            .as_object()
            .expect("clientCapabilities object")
            .is_empty()
    );
    let session = session_new_request("/tmp/work").expect("absolute cwd");
    assert_eq!(session["params"]["mcpServers"], json!([]));
    assert_eq!(
        permission_outcome(&json!([{ "optionId": "reject-once" }]), false)["outcome"]["outcome"],
        "selected"
    );
    assert_eq!(
        permission_outcome(&json!([]), false)["outcome"]["outcome"],
        "cancelled"
    );
    let update = json!({
        "jsonrpc":"2.0",
        "method":"session/update",
        "params":{"sessionId":"session-1","update":{"sessionUpdate":"agent_message_chunk"}}
    });
    validate_session_update(&update, "session-1").expect("current session update");
    assert!(validate_session_update(&update, "stale-session").is_err());
    let init = json!({"result":{"protocolVersion":DEEPSEEK_PROTOCOL_VERSION,"agentInfo":{"name":"dsh","version":"0.1.0-rc.6"}}});
    validate_initialize_result(&init).expect("initialize result");
    let mut seen = HashSet::new();
    let created = json!({"result":{"sessionId":"session-1"}});
    assert_eq!(
        validate_session_new_result(&created, &mut seen).expect("session id"),
        "session-1"
    );
    assert!(validate_session_new_result(&created, &mut seen).is_err());
}

#[test]
fn session_new_accepts_unix_and_windows_absolute_paths() {
    assert!(session_new_request("/tmp/work").is_ok());
    assert!(session_new_request(r"C:\\work").is_ok());
    assert!(session_new_request(r"\\\\server\\share\\work").is_ok());
    assert!(session_new_request("relative/work").is_err());
    assert!(session_new_request(r"C:relative").is_err());
}
