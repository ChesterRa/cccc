use super::*;

#[test]
fn queue_is_bounded_and_generation_changes_on_stop() {
    let mut supervisor = DeepSeekSupervisor::default();
    let root = tempfile::tempdir().expect("tempdir");
    let command = if cfg!(windows) {
        vec!["cmd".into(), "/C".into(), "more".into()]
    } else {
        vec!["sh".into(), "-c".into(), "cat".into()]
    };
    let generation = supervisor.start(&command, root.path(), &[]).expect("start");
    for _ in 0..MAX_PENDING_REQUESTS {
        supervisor.enqueue("prompt").expect("queue");
    }
    assert!(matches!(
        supervisor.enqueue("overflow"),
        Err(SupervisorError::QueueFull)
    ));
    supervisor.stop().expect("stop");
    assert_ne!(generation, supervisor.generation());
}

#[cfg(unix)]
#[test]
fn fake_acp_handshake_consumes_initialize_and_session_new() {
    let mut supervisor = DeepSeekSupervisor::default();
    let root = tempfile::tempdir().expect("tempdir");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake","version":"0.1.0-rc.6"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
elif printf '%s' "$line" | grep -q '"prompt":\[' && printf '%s' "$line" | grep -q '"type":"text"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
else
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"error":{"message":"prompt must be ContentBlock[]"}}'
fi
done"#
    .replace(
        "\"protocolVersion\":1",
        &format!("\"protocolVersion\":{}", cccc_contracts::DEEPSEEK_PROTOCOL_VERSION),
    );
    let command = vec!["sh".into(), "-c".into(), script];
    supervisor.start(&command, root.path(), &[]).expect("start");
    let session = supervisor
        .handshake(root.path(), Duration::from_secs(2))
        .expect("handshake");
    assert_eq!(session, "fake-session");
    assert_eq!(supervisor.enqueue("hello").expect("enqueue"), 3);
    assert_eq!(supervisor.flush_one(&session).expect("flush"), Some(3));
    assert_eq!(
        supervisor
            .next_frame(Duration::from_secs(2))
            .expect("terminal")["result"]["stopReason"],
        "end_turn"
    );
    supervisor.stop().expect("stop");
}

#[cfg(unix)]
#[test]
fn stop_escalates_after_term_timeout() {
    let mut supervisor = DeepSeekSupervisor::default();
    let root = tempfile::tempdir().expect("tempdir");
    let ready = root.path().join("term-trap-ready");
    let command = vec![
        "sh".into(),
        "-c".into(),
        "trap '' TERM; : > \"$CCCC_DEEPSEEK_TEST_READY\"; sleep 30".into(),
    ];
    supervisor
        .start(
            &command,
            root.path(),
            &[(
                "CCCC_DEEPSEEK_TEST_READY".into(),
                ready.to_string_lossy().into_owned(),
            )],
        )
        .expect("start");
    let ready_deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() && Instant::now() < ready_deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "TERM trap fixture did not become ready");

    let started = std::time::Instant::now();
    supervisor.stop().expect("stop");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!supervisor.is_running());
}
