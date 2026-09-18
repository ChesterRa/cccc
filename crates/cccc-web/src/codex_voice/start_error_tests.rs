use super::*;
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use serde_json::json;

#[tokio::test]
async fn preserves_failure_stage_and_safe_metadata_in_http_response() {
    for (status, code) in [
        (401, "codex_voice_realtime_auth_failed"),
        (403, "codex_voice_realtime_rejected"),
        (429, "codex_voice_realtime_rate_limited"),
        (503, "codex_voice_realtime_rejected"),
    ] {
        let error = Error::new(RealtimeCallError::HttpStatus(status))
            .context("private-path-and-provider-explanation")
            .context(StartStage::Realtime);
        let diagnostic = StartDiagnostic::from_error(&error, 1234);
        let details = diagnostic.details();
        assert_eq!(details["stage"], "realtime");
        assert_eq!(details["http_status"], status);
        assert_eq!(details["elapsed_ms"], 1234);
        assert_eq!(details["code"], code);
        let response = diagnostic.into_api_error().into_response();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("json");
        assert!(!text.contains("private-path-and-provider-explanation"));
        let value: serde_json::Value = serde_json::from_str(&text).expect("response");
        assert_eq!(value["error"]["code"], code);
        assert_eq!(value["error"]["details"], details);
    }
}

#[test]
fn analyst_timeout_credentials_and_recording_conflicts_stay_distinct() {
    let timeout = Error::new(io::Error::new(io::ErrorKind::TimedOut, "private-command"))
        .context(StartStage::Analyst);
    assert_eq!(
        StartDiagnostic::from_error(&timeout, 10_000).code,
        "codex_voice_analyst_start_timeout"
    );
    let credentials = Error::msg("private-auth-path")
        .context(RealtimeCallError::Credentials)
        .context(StartStage::Realtime);
    assert_eq!(
        StartDiagnostic::from_error(&credentials, 10).code,
        "codex_voice_realtime_auth_failed"
    );
    let lease = Error::new(LeaseError {
        code: "assistant_voice_recording_busy",
        message: "private-owner".into(),
        details: serde_json::Map::new(),
    })
    .context(StartStage::Recording);
    assert_eq!(
        StartDiagnostic::from_error(&lease, 20).details(),
        json!({
            "code":"codex_voice_recording_busy", "stage":"recording", "elapsed_ms":20
        })
    );
    let lost = Error::msg("Codex Voice microphone lease was lost").context(StartStage::Recording);
    assert_eq!(
        StartDiagnostic::from_error(&lost, 30).code,
        "codex_voice_recording_failed"
    );
    let configuration = Error::msg("private-setting");
    assert_eq!(
        StartDiagnostic::from_error(&configuration, 1).code,
        "codex_voice_setup_failed"
    );
}

#[tokio::test]
async fn request_timeout_and_connection_failure_do_not_become_login_errors() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_millis(100))
        .build()
        .expect("client");
    // TCP can connect but no HTTP answer is sent; no provider traffic.
    let error = client
        .get(format!("http://{address}/?private-token=secret"))
        .send()
        .await
        .expect_err("timeout");
    let error = Error::new(error).context(StartStage::Realtime);
    let diagnostic = StartDiagnostic::from_error(&error, 100);
    assert_eq!(diagnostic.code, "codex_voice_realtime_timeout");
    assert!(!diagnostic.details().to_string().contains("secret"));
    drop(listener);
    let error = client
        .get(format!("http://{address}/"))
        .send()
        .await
        .expect_err("closed port");
    let error = Error::new(error).context(StartStage::Realtime);
    assert_eq!(
        StartDiagnostic::from_error(&error, 2).code,
        "codex_voice_realtime_connection_failed"
    );
}
