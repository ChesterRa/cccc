use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{
    Message,
    handshake::server::{Request, Response},
};

fn config(key: &str) -> config::Config {
    config::Config {
        api_key: key.into(),
        ..Default::default()
    }
}
fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    (temp, home)
}

#[path = "tests/bailian.rs"]
mod bailian_tests;
#[path = "tests/persistence.rs"]
mod persistence_tests;
#[path = "tests/volcengine.rs"]
mod volcengine_tests;

#[test]
fn rejects_foreign_tasks_malformed_packets_and_provider_errors_without_echoing_secrets() {
    assert!(
        bailian::parse(
            br#"{"header":{"task_id":"other","event":"task-started"}}"#,
            "owned"
        )
        .is_err()
    );
    assert!(matches!(bailian::parse(br#"{"header":{"task_id":"owned","event":"result-generated"},"payload":{"output":{"sentence":{"heartbeat":true}}}}"#,"owned"),Ok(transcript::Event::Ignore)));
    for bytes in [
        vec![],
        vec![0x11, 0x91, 0x10, 0],
        vec![0x11, 0x91, 0x10, 0, 0, 0, 0, 1, 255, 255, 255, 255],
    ] {
        assert!(volcengine::parse(&bytes).is_err());
    }
    let error=bailian::parse(br#"{"header":{"task_id":"owned","event":"task-failed","error_message":"secret-test-key"}}"#,"owned").err().expect("provider error");
    assert!(!error.message.contains("secret-test-key"));
}

#[test]
fn incomplete_transcripts_remain_partial_and_final_sentences_replace_partial_hypotheses() {
    use transcript::{Segment, Transcript};
    let mut value = Transcript::default();
    let sentence = |text: &str, finalized| Segment {
        id: "b:1".into(),
        text: text.into(),
        start_ms: 0,
        end_ms: 100,
        finalized,
    };
    assert_eq!(
        value
            .apply(vec![sentence("错字", false)], "bailian:fun-asr-realtime")
            .expect("partial")[0]["type"],
        "partial"
    );
    assert_eq!(
        value
            .apply(vec![sentence("正确", true)], "bailian:fun-asr-realtime")
            .expect("final")[0]["type"],
        "final"
    );
    assert_eq!(value.text(), "正确");
    assert!(
        value
            .apply(vec![sentence("正确", true)], "model")
            .expect("duplicate")
            .is_empty()
    );
    assert_eq!(value.final_event("model", false, json!(2))["partial"], true);
    assert_eq!(value.final_event("model", true, json!(2))["partial"], false);
}

#[tokio::test]
async fn resource_denials_are_distinct_from_authentication_and_do_not_echo_secrets() {
    for (status, body, code) in [
        (
            403,
            r#"{"error":"[resource_id=private] requested resource not granted"}"#,
            "external_asr_resource_not_granted",
        ),
        (
            403,
            r#"{"error":"private-credential"}"#,
            "external_asr_access_denied",
        ),
        (
            401,
            r#"{"error":"private-credential"}"#,
            "external_asr_auth_failed",
        ),
        (
            429,
            r#"{"error":"private-credential"}"#,
            "external_asr_rate_limited",
        ),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let result =
                tokio_tungstenite::accept_hdr_async(stream, move |_: &Request, _: Response| {
                    Err(tokio_tungstenite::tungstenite::http::Response::builder()
                        .status(status)
                        .body(Some(body.into()))
                        .expect("response"))
                })
                .await;
            assert!(result.is_err());
        });
        let error = connection::connect_at(
            Provider::Volcengine,
            config("private-credential"),
            "auto",
            &endpoint,
        )
        .await
        .err()
        .expect("rejected");
        assert_eq!(error.code, code);
        assert!(!error.message.contains("private"));
        server.await.expect("server");
    }
}

#[test]
fn new_provider_defaults_do_not_overwrite_existing_legacy_settings() {
    let (_temp, home) = home();
    let fresh = config::load(&home, Provider::Volcengine).expect("defaults");
    assert_eq!(fresh.auth_mode, "api_key");
    assert_eq!(fresh.resource_id, "volc.seedasr.sauc.duration");
    config::save(&home, Provider::Volcengine, serde_json::from_value(json!({"auth_mode":"app_token","resource_id":"volc.bigasr.sauc.duration","app_id":"123","access_token":"private-token"})).expect("patch")).expect("save");
    let loaded = config::load(&home, Provider::Volcengine).expect("load");
    assert_eq!(loaded.auth_mode, "app_token");
    assert_eq!(loaded.resource_id, "volc.bigasr.sauc.duration");
    assert!(loaded.configured(Provider::Volcengine));
}
