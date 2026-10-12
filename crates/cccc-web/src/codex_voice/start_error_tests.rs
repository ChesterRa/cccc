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

fn no_proxy_client(timeout: std::time::Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(timeout)
        .build()
        .expect("client")
}

#[tokio::test]
async fn request_timeout_does_not_become_a_login_error() {
    // TCP can connect but no HTTP answer is sent; no provider traffic.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let error = no_proxy_client(std::time::Duration::from_millis(100))
        .get(format!("http://{address}/?private-token=secret"))
        .send()
        .await
        .expect_err("timeout");
    let error = Error::new(error).context(StartStage::Realtime);
    let diagnostic = StartDiagnostic::from_error(&error, 100);
    assert_eq!(diagnostic.code, "codex_voice_realtime_timeout");
    assert!(!diagnostic.details().to_string().contains("secret"));
}

#[tokio::test]
async fn connection_failure_does_not_become_a_login_error() {
    const SYNTHETIC_OS_ERROR: i32 = 111;
    struct FailedDns;
    impl reqwest::dns::Resolve for FailedDns {
        fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            assert_eq!(name.as_str(), "connection-fixture.invalid");
            // Synthetic resolver error: test connector classification and
            // raw OS metadata without depending on the host's TCP behavior.
            Box::pin(async move {
                Err(Box::new(io::Error::from_raw_os_error(SYNTHETIC_OS_ERROR))
                    as Box<dyn std::error::Error + Send + Sync>)
            })
        }
    }
    // Fail inside the real connector without DNS/TCP traffic or a freed port.
    let error = reqwest::Client::builder()
        .no_proxy()
        .dns_resolver(std::sync::Arc::new(FailedDns))
        .build()
        .expect("client")
        .get("http://connection-fixture.invalid/")
        .send()
        .await
        .expect_err("connection failure");
    let error = Error::new(error).context(StartStage::Realtime);
    let details = StartDiagnostic::from_error(&error, 2).details();
    assert_eq!(
        details["request_kind"], "connect",
        "safe diagnostic: {details}"
    );
    assert_eq!(details["os_error"], SYNTHETIC_OS_ERROR);
    assert_eq!(
        StartDiagnostic::from_error(&error, 2).code,
        "codex_voice_realtime_connection_failed"
    );
}

#[test]
fn tls_diagnostic_omits_arbitrary_certificate_and_provider_details() {
    let error = Error::new(rustls::Error::General("private peer detail".into()))
        .context(StartStage::Realtime);
    let details = StartDiagnostic::from_error(&error, 700).details();
    assert_eq!(details["tls_error"], "tls_protocol");
    assert!(!details.to_string().contains("private"));
}

#[tokio::test]
async fn https_certificate_failure_preserves_tls_category_without_private_details() {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    use std::{sync::Arc, time::Duration};

    let fixture = include_bytes!("testdata/untrusted-localhost.pem");
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("TLS versions")
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from_pem_slice(fixture).expect("test certificate")],
        PrivateKeyDer::from_pem_slice(fixture).expect("public test key"),
    )
    .expect("server config");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let timeout = Duration::from_secs(5);
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .tls_built_in_root_certs(false)
        .no_proxy()
        .timeout(timeout)
        .build()
        .expect("client");
    let server = tokio::spawn(async move {
        let (socket, _) = tokio::time::timeout(timeout, listener.accept())
            .await
            .expect("accept deadline")
            .expect("accept");
        let mut socket = socket.into_std().expect("blocking socket");
        socket.set_nonblocking(false).expect("blocking mode");
        socket
            .set_read_timeout(Some(timeout))
            .expect("read timeout");
        socket
            .set_write_timeout(Some(timeout))
            .expect("write timeout");
        tokio::task::spawn_blocking(move || {
            let mut connection = rustls::ServerConnection::new(Arc::new(config)).expect("TLS");
            // The untrusted certificate must be rejected before any HTTP body is sent.
            connection
                .complete_io(&mut socket)
                .expect_err("untrusted certificate")
        })
        .await
        .expect("TLS server task")
    });
    let result = client
        .get(format!(
            "https://{address}/private-path?private-token=secret"
        ))
        .send()
        .await;
    server.await.expect("server joined");
    let error =
        Error::new(result.expect_err("certificate rejection")).context(StartStage::Realtime);
    let diagnostic = StartDiagnostic::from_error(&error, 700);
    assert_eq!(diagnostic.code, "codex_voice_realtime_connection_failed");
    let details = diagnostic.details();
    assert_eq!(details["request_kind"], "connect");
    assert_eq!(details["tls_error"], "invalid_certificate");
    let serialized = details.to_string();
    for private_detail in [
        "private-path",
        "private-token",
        "secret",
        "cccc-loopback.test",
    ] {
        assert!(
            !serialized.contains(private_detail),
            "diagnostic leaked a private detail"
        );
    }
}
