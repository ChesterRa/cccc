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

#[tokio::test]
async fn over_limit_event_disconnects_without_accepting_following_idle() {
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
        "sse-over-limit-fixture",
    )
    .expect("spawn fixture ACP");
    let protocol = AcpClient::new(
        stdin,
        stdout,
        "generation-over-limit".into(),
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
    protocol
        .register_native_input("over-limit-native-input", "fixture input")
        .await
        .expect("register exact input");
    let mut events = protocol.subscribe();
    let payloads = [
        json!({"type":"message.updated","properties":{"info":{"id":"user-1","sessionID":"session-owned","role":"user"}}}),
        json!({"type":"message.part.updated","properties":{"part":{"id":"user-part","sessionID":"session-owned","messageID":"user-1","type":"text","text":"fixture input"}}}),
        json!({"type":"message.updated","properties":{"info":{"id":"assistant-1","sessionID":"session-owned","role":"assistant","parentID":"user-1"}}}),
        json!({"type":"session.status","properties":{"sessionID":"session-owned","status":{"type":"busy"}}}),
        // Valid JSON that exceeds the documented encoded-line limit. A reader
        // that drops it and continues would incorrectly complete at the idle.
        json!({"type":"message.part.updated","properties":{"part":{"id":"answer-part","sessionID":"session-owned","messageID":"assistant-1","type":"text","text":"x".repeat(16 * 1024 * 1024)}}}),
        json!({"type":"session.status","properties":{"sessionID":"session-owned","status":{"type":"idle"}}}),
    ];
    let mut wire = String::new();
    for payload in &payloads {
        wire.push_str(&format!("data: {payload}\r\n\r\n"));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("scratch server");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("listener address")
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let mut server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await?);
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await?;
        socket
            .write_all(format!("{:x}\r\n", wire.len()).as_bytes())
            .await?;
        for fragment in wire.as_bytes().chunks(16 * 1024) {
            if socket.write_all(fragment).await.is_err() {
                return Ok::<(), std::io::Error>(());
            }
        }
        // Keep the HTTP stream open so EOF cannot stand in for the cap error.
        let _ = socket.write_all(b"\r\n").await;
        let _ = stopped.await;
        Ok(())
    });
    let observed = tokio::time::timeout(Duration::from_secs(10), async {
        lifecycle::attach(
            &protocol,
            &endpoint,
            "fixture",
            "private",
            "session-owned",
            temp.path(),
        )
        .await?;
        let mut receipt = false;
        let mut completed = false;
        loop {
            let event = events.recv().await.map_err(std::io::Error::other)?;
            receipt |= event.requested_delegation_id.as_deref() == Some("over-limit-native-input");
            completed |= event.message["method"] == "turn/completed"
                && event.message["params"]["turn"]["status"] == "completed";
            if event.message["method"] == MANAGED_AGENT_DISCONNECTED_METHOD {
                let reason = event.message["params"]["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                return Ok::<_, std::io::Error>((receipt, completed, reason));
            }
        }
    })
    .await;
    let ping = protocol
        .request("fixture/ping", json!({}), Duration::from_secs(5))
        .await;

    // Clean up even if observation times out or the bridge reports a failure.
    protocol.close().await;
    let process_stopped = owner.stop();
    let _ = stop.send(());
    let server_finished = match tokio::time::timeout(Duration::from_secs(5), &mut server).await {
        Ok(result) => result.is_ok(),
        Err(_) => {
            server.abort();
            let _ = server.await;
            false
        }
    };
    process_stopped.expect("stop scratch ACP");
    assert!(server_finished, "stop scratch SSE server");
    let (receipt, completed, reason) = observed
        .expect("over-limit event must disconnect promptly")
        .expect("observe lifecycle events");
    assert!(receipt, "exercise an admitted native turn before overflow");
    assert!(!completed, "the following idle must not settle lost output");
    assert!(reason.contains("exceeded the bounded buffer"), "{reason}");
    assert!(reason.contains("16777216 bytes per line"), "{reason}");
    assert_eq!(
        ping.expect_err("disconnected session must reject new requests")
            .kind(),
        std::io::ErrorKind::BrokenPipe
    );
}
