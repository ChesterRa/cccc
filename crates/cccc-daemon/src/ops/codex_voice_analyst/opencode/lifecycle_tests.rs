//! Exercise the actual HTTP SSE subscription and ACP lifecycle bridge without
//! an OpenCode installation, model calls, or a live daemon home.
#![cfg(unix)]

use super::super::MANAGED_AGENT_DISCONNECTED_METHOD;
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn oversized_event_preserves_native_receipt_output_and_session_lifecycle() {
    let temp = tempfile::tempdir().expect("isolated fixture");
    let script = r#"i=0
while IFS= read -r line; do
  i=$((i+1))
  printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"session-owned"}}\n' "$i"
done
"#;
    let (owner, stdin, stdout) = process::spawn_piped(
        &["/bin/sh".into(), "-c".into(), script.into()],
        temp.path(),
        &BTreeMap::new(),
        "sse-fixture",
    )
    .expect("spawn fixture ACP");
    let protocol = AcpClient::new(
        stdin,
        stdout,
        "generation-sse".into(),
        "opencode",
        PermissionPolicy::Reject,
        PromptCompletion::SessionEvents,
    )
    .expect("ACP bridge");
    protocol
        .request("initialize", json!({}), Duration::from_secs(5))
        .await
        .expect("initialize");
    protocol
        .request("session/new", json!({}), Duration::from_secs(5))
        .await
        .expect("owned session");
    let user_text = "user-content-".repeat(90_000);
    let assistant_text = "answer-content-".repeat(80_000);
    let image_url = format!("data:image/png;base64,{}", "A".repeat(3_131_070));
    protocol
        .register_native_input("large-native-input", &user_text)
        .await
        .expect("register exact input");
    let mut events = protocol.subscribe();
    let payloads = [
        // Resume replays stored parts before the new user message is observed.
        json!({"type":"message.updated","properties":{"info":{"id":"historical-assistant","sessionID":"session-owned","role":"assistant","parentID":"historical-user"}}}),
        json!({"type":"message.part.updated","properties":{"part":{"id":"historical-read","sessionID":"session-owned","messageID":"historical-assistant","type":"tool","callID":"historical-tool","tool":"read","state":{"status":"completed","input":{},"output":"image","metadata":{},"attachments":[{"type":"file","mime":"image/png","url":image_url}]}}}}),
        json!({"type":"session.status","properties":{"sessionID":"session-owned","status":{"type":"idle"}}}),
        json!({"type":"message.updated","properties":{"info":{"id":"user-1","sessionID":"session-owned","role":"user"}}}),
        json!({"type":"message.part.updated","properties":{"part":{"id":"user-part","sessionID":"session-owned","messageID":"user-1","type":"text","text":user_text}}}),
        json!({"type":"message.updated","properties":{"info":{"id":"assistant-1","sessionID":"session-owned","role":"assistant","parentID":"user-1"}}}),
        json!({"type":"session.status","properties":{"sessionID":"session-owned","status":{"type":"busy"}}}),
        json!({"type":"message.part.updated","properties":{"part":{"id":"answer-part","sessionID":"session-owned","messageID":"assistant-1","type":"text","text":assistant_text}}}),
        json!({"type":"message.part.updated","properties":{"part":{"id":"tool-part","sessionID":"session-owned","messageID":"assistant-1","type":"tool","callID":"tool-1","tool":"read","state":{"status":"completed","input":{},"output":"image","metadata":{},"attachments":[{"type":"file","mime":"image/png","url":image_url}]}}}}),
        json!({"type":"session.status","properties":{"sessionID":"session-owned","status":{"type":"idle"}}}),
    ];
    let mut wire = String::new();
    for payload in &payloads {
        let line = format!("data: {payload}\r\n\r\n");
        if payload["type"] == "message.part.updated" {
            assert!(line.len() > 512 * 1024, "exercise the stock limit");
        }
        wire.push_str(&line);
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("scratch server");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("listener address")
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("SSE connection");
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await.expect("request headers"));
        }
        let headers = String::from_utf8(request).expect("HTTP headers");
        assert!(headers.starts_with("GET /event?"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: basic ")
        );
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.expect("SSE headers");
        socket
            .write_all(format!("{:x}\r\n", wire.len()).as_bytes())
            .await
            .expect("chunk framing");
        // Socket fragmentation must not turn a single valid event into a cap
        // failure or lose the following normal session-status event.
        for fragment in wire.as_bytes().chunks(16 * 1024) {
            if socket.write_all(fragment).await.is_err() {
                return;
            }
        }
        socket.write_all(b"\r\n").await.expect("chunk ending");
        let _ = stopped.await; // Keep the lifecycle stream alive through assertions.
    });
    lifecycle::attach(
        &protocol,
        &endpoint,
        "fixture",
        "private",
        "session-owned",
        temp.path(),
    )
    .await
    .expect("attach subscription");
    let mut receipt = false;
    let mut answer = String::new();
    let mut tool = false;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.recv().await.expect("lifecycle event");
            assert_ne!(
                event.message["method"], MANAGED_AGENT_DISCONNECTED_METHOD,
                "must survive large SSE events: {}",
                event.message
            );
            receipt |= event.requested_delegation_id.as_deref() == Some("large-native-input");
            match event.message["method"].as_str() {
                Some("item/agentMessage/delta") => answer.push_str(
                    event.message["params"]["delta"]
                        .as_str()
                        .expect("answer text"),
                ),
                Some("cccc/toolActivity") => {
                    tool |= receipt && event.message["params"]["title"] == "read"
                }
                Some("turn/completed") => {
                    assert_eq!(event.message["params"]["turn"]["status"], "completed");
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("following idle must settle the turn");
    assert!(
        receipt,
        "large user part must preserve its exact input receipt"
    );
    assert_eq!(
        answer, assistant_text,
        "large assistant part must not be dropped"
    );
    assert!(tool, "large tool snapshot must still project activity");
    protocol
        .request("fixture/ping", json!({}), Duration::from_secs(5))
        .await
        .expect("session remains usable");
    protocol.close().await;
    owner.stop().expect("stop scratch ACP");
    let _ = stop.send(());
    server.await.expect("stop scratch SSE server");
}
