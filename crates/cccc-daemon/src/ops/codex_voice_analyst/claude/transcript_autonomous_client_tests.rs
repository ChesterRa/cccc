//! Provider boundary is simulated; the real client loop sees Claude answer hidden input.
use super::{ClaudeClient, MANAGED_AGENT_DISCONNECTED_METHOD};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

const SESSION_ID: &str = "52b41c61-e23c-4b7c-8b60-809c347451b5";

fn append(transcript: &Path, records: &[serde_json::Value]) {
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .expect("open transcript");
    for record in records {
        writeln!(file, "{record}").expect("append transcript");
    }
}

/// A message from another Claude session is answered in a turn Claude starts itself; the
/// managed session stays connected and keeps serving deliveries afterwards.
pub(super) async fn verify_hidden_input_turn(client: &ClaudeClient, transcript: &Path) {
    let mut events = client.subscribe();
    append(
        transcript,
        &[json!({
            "type":"user","sessionId":SESSION_ID,"promptId":"peer-prompt","isMeta":true,
            "origin":{"kind":"peer","from":"bridge:session_peer","name":"PEER"},
            "message":{"role":"user","content":[{"type":"text","text":
                "Another Claude session sent a message:\n<cross-session-message from=\"bridge:session_peer\">status?</cross-session-message>"}]}
        })],
    );
    append(
        transcript,
        &[
            json!({"type":"assistant","sessionId":SESSION_ID,
                "message":{"content":[{"type":"text","text":"all green"}]}}),
            json!({"type":"system","sessionId":SESSION_ID,"subtype":"turn_duration"}),
        ],
    );
    let mut started = None;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.expect("transcript event");
            let method = event.message["method"].as_str().unwrap_or_default();
            assert_ne!(method, MANAGED_AGENT_DISCONNECTED_METHOD, "{event:?}");
            if method == "turn/started" {
                started = event.message["params"]["turn"]["id"]
                    .as_str()
                    .map(str::to_owned);
            }
            if method == "turn/completed" {
                assert_eq!(event.message["params"]["turn"]["status"], "completed");
                break;
            }
        }
    })
    .await
    .expect("Claude's own turn must complete");
    assert_eq!(started.as_deref(), Some("claude-peer-prompt"));
    assert!(client.running());
}
