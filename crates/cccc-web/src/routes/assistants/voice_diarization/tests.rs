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
    assert_eq!(request.args["completion_event"], "diarization_failed");
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
    assert_eq!(request.args["completion_event"], "diarization_ready");
    assert_eq!(patch["diarization"], result);
    assert!(patch["error"].is_null());
    assert!(patch.get("diarization_error").is_none());
}

async fn completion_fixture() -> (
    tempfile::TempDir,
    cccc_core::HomeLayout,
    String,
    tokio::net::TcpListener,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("completion", "")
        .expect("group");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = cccc_contracts::DaemonAddress {
        v: 1,
        transport: cccc_contracts::Transport::Tcp,
        path: String::new(),
        host: "127.0.0.1".into(),
        port: listener.local_addr().expect("address").port(),
        pid: std::process::id(),
        version: "test".into(),
        ts: "test".into(),
    };
    std::fs::write(
        home.daemon_dir().join("ccccd.addr.json"),
        serde_json::to_vec(&address).expect("serialize"),
    )
    .expect("write address");
    (temp, home, group.group_id, listener)
}

#[tokio::test]
async fn completion_retries_io_and_lost_reply_without_duplicating_the_daemon_event() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (_temp, home, group_id, listener) = completion_fixture().await;
    let server_home = home.clone();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for attempt in 0..3 {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.expect("read request");
            let request: DaemonRequest = serde_json::from_str(&line).expect("request");
            let response = if attempt == 0 {
                cccc_contracts::DaemonResponse::failure("io_error", "temporary append failure")
            } else {
                cccc_daemon::handle_request(&server_home, &request)
            };
            requests.push(request);
            if attempt == 1 {
                assert!(response.ok, "{:?}", response.error);
                continue; // The operation committed, but the reply is lost.
            }
            let mut bytes = serde_json::to_vec(&response).expect("response");
            bytes.push(b'\n');
            stream
                .get_mut()
                .write_all(&bytes)
                .await
                .expect("write response");
        }
        requests
    });
    let client = cccc_client::DaemonClient::new(home.clone())
        .with_timeout(std::time::Duration::from_secs(2));
    persist_result(
        &client,
        &group_id,
        "completion-1",
        "docs/meeting.md",
        Some(json!({"speakers":[]})),
        "",
        "",
    )
    .await
    .expect("confirmed completion");
    let requests = server.await.expect("server");
    assert!(requests.windows(2).all(|pair| pair[0].args == pair[1].args));
    let path = cccc_core::GroupStore::new(home)
        .expect("store")
        .ledger_path(&group_id)
        .expect("ledger path");
    let events = cccc_core::ledger::read_all(&path).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "assistant.voice.session")
            .count(),
        1
    );
}

#[tokio::test]
async fn completion_does_not_retry_permission_errors_or_accept_an_unconfirmed_projection() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    for (response, expected) in [
        (
            cccc_contracts::DaemonResponse::failure("permission_denied", "not allowed"),
            "permission_denied",
        ),
        (
            cccc_contracts::DaemonResponse::success(Default::default()),
            "did not confirm",
        ),
    ] {
        let (_temp, home, group_id, listener) = completion_fixture().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.expect("read request");
            let mut bytes = serde_json::to_vec(&response).expect("response");
            bytes.push(b'\n');
            stream
                .get_mut()
                .write_all(&bytes)
                .await
                .expect("write response");
        });
        let client =
            cccc_client::DaemonClient::new(home).with_timeout(std::time::Duration::from_secs(2));
        let error = persist_result(
            &client,
            &group_id,
            "completion-1",
            "docs/meeting.md",
            None,
            "model_failed",
            "failed",
        )
        .await
        .expect_err("completion not confirmed");
        assert!(error.to_string().contains(expected), "{error}");
        server.await.expect("server");
    }
}
