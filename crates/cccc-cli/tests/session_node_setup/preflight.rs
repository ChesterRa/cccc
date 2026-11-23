use super::fixture::Fixture;
use cccc_core::fs;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn existing_incompatible_daemon_is_not_replaced_or_followed_by_native_setup() {
    for (relay, runtime, protocol, expected) in [
        (false, "codex", 0, "does not support device"),
        (true, "codex", 1, "does not support relay"),
        (false, "pi", 1, "does not support pi device sessions"),
        (
            false,
            "opencode",
            1,
            "does not support opencode device sessions",
        ),
    ] {
        let mut f = Fixture::new();
        f.start_home(f.hub.clone()).await;
        f.node.initialize().expect("node Home");
        let mut ticket = cccc_session_node::decode_ticket(&f.ticket().await).expect("test ticket");
        if relay {
            ticket.v = 2;
            ticket.relay_url = Some("https://127.0.0.1:1/".into());
            ticket.relay_token = Some(format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ));
        }
        let encoded = cccc_session_node::encode_ticket(&ticket).expect("encoded test ticket");
        // Only this legacy IPC boundary is stubbed. The real CLI/daemon client,
        // capability validation and native setup ordering remain exercised.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("legacy IPC");
        let port = listener.local_addr().expect("address").port();
        fs::write_json(&f.node.daemon_dir().join("ccccd.addr.json"), &json!({"v":1,"transport":"tcp","host":"127.0.0.1","port":port,"pid":std::process::id(),"version":"fixture-old","ts":"fixture"})).expect("legacy address");
        f.spawn(tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await?;
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).await?;
                let request: cccc_contracts::DaemonRequest = serde_json::from_str(&line)?;
                let result = if request.op == "session_node_capabilities" {
                    json!({"protocol":protocol,"runtimes":["codex","claude"]})
                } else {
                    json!({})
                };
                let response = cccc_contracts::DaemonResponse::success(
                    result.as_object().expect("result").clone(),
                );
                stream
                    .get_mut()
                    .write_all(format!("{}\n", serde_json::to_string(&response)?).as_bytes())
                    .await?;
                if request.op == "shutdown" {
                    break;
                }
            }
            Ok(())
        }));
        let response = f
            .run(&["--runtime", runtime, "--ticket-stdin"], &encoded)
            .await;
        assert!(!response.status.success());
        let error = String::from_utf8_lossy(&response.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(
            f.calls().is_empty(),
            "preflight must precede provider setup"
        );
        assert!(
            !f.node.daemon_dir().join("ccccd.log").exists(),
            "existing daemon must not be replaced"
        );
        f.assert_private(&response, &encoded);
        f.close().await;
    }
}
