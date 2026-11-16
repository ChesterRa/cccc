//! Real CLI/configuration path; native executable and unavailable key are fixture boundaries.
#![cfg(unix)]
#[path = "session_node_setup/fixture.rs"]
mod fixture;
#[path = "session_node_setup/preflight.rs"]
mod preflight;
use cccc_contracts::local_session::LocalSessionRuntime;
use cccc_core::{local_sessions, session_nodes};
use fixture::Fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_command_configures_each_supported_runtime_and_pairs_without_admitting_a_group() {
    for runtime in [Some("codex"), Some("claude"), None] {
        let mut f = Fixture::new();
        f.start().await;
        let ticket = f.ticket().await;
        let args = runtime.map_or_else(
            || vec!["--ticket-stdin"],
            |runtime| vec!["--runtime", runtime, "--ticket-stdin"],
        );
        let mut child = f.command(&args).spawn().expect("one-command CLI");
        use tokio::io::AsyncWriteExt;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(format!("{ticket}\n").as_bytes())
            .await
            .expect("private ticket pipe");
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                assert!(
                    child.try_wait().expect("CLI status").is_none(),
                    "connector must remain in foreground"
                );
                if session_nodes::saved_client(&f.node)
                    .expect("client")
                    .is_some_and(|client| client.connected)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("real isolated pairing completes");
        assert_eq!(session_nodes::peers(&f.hub).expect("peers").len(), 1);
        for home in [&f.hub, &f.node] {
            assert!(
                cccc_core::GroupStore::new(home.clone())
                    .expect("store")
                    .list()
                    .expect("groups")
                    .is_empty()
            );
        }
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(child.id().expect("own PID") as i32),
            nix::sys::signal::Signal::SIGTERM,
        )
        .expect("stop own bridge");
        let response =
            tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
                .await
                .expect("bridge stops")
                .expect("bridge output");
        assert!(
            response.status.success(),
            "{}",
            String::from_utf8_lossy(&response.stderr)
        );
        if let Some(runtime) = runtime {
            assert!(f.calls().contains(&format!("{runtime}:mcp")));
        } else {
            assert!(
                f.calls().is_empty(),
                "legacy connect must not enable any runtime"
            );
        }
        f.assert_private(&response, &ticket);
        f.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_runtime_keeps_successful_setup_after_connection_failure() {
    for runtime in ["codex", "claude"] {
        let mut f = Fixture::new();
        f.start().await;
        let ticket = f.ticket().await;
        let old = f.saved(false);
        session_nodes::set_client(&f.node, &old).expect("old offline route");
        session_nodes::identity(&f.node, "client", || "invalid-private-key".into())
            .expect("controlled connect failure after configuration");
        let response = f
            .run(&["--runtime", runtime, "--ticket-stdin"], &ticket)
            .await;
        assert!(!response.status.success());
        let output = String::from_utf8_lossy(&response.stdout);
        assert!(output.contains("\"enabled\": true"), "{output}");
        let error = String::from_utf8_lossy(&response.stderr);
        assert!(error.contains("remains enabled"), "{error}");
        let current = session_nodes::saved_client(&f.node)
            .expect("saved")
            .expect("old route retained");
        assert_eq!(current.endpoint.endpoint_id, old.endpoint.endpoint_id);
        assert_eq!(current.grant_id, old.grant_id);
        let kind = if runtime == "codex" {
            LocalSessionRuntime::Codex
        } else {
            LocalSessionRuntime::Claude
        };
        assert!(!local_sessions::integration_disabled(&f.node, kind).expect("enabled state"));
        let calls = f.calls();
        assert!(calls.contains(&format!("{runtime}:mcp")), "{calls}");
        assert!(!calls.contains(if runtime == "codex" {
            "claude:"
        } else {
            "codex:"
        }));
        assert!(!calls.contains("trust"));
        f.assert_private(&response, &ticket);
        f.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_enable_never_starts_pairing_or_changes_reconnect_state() {
    let mut f = Fixture::new();
    f.start().await;
    let ticket = f.ticket().await;
    std::fs::write(f.root().join("fail-native"), b"fail configuration").expect("fault");
    let old = f.saved(false);
    session_nodes::set_client(&f.node, &old).expect("old route");
    let before = f.client_bytes();
    let response = f
        .run(&["--runtime", "codex", "--ticket-stdin"], &ticket)
        .await;
    assert!(!response.status.success());
    assert_eq!(f.client_bytes(), before);
    assert!(session_nodes::peers(&f.hub).expect("hub").is_empty());
    assert!(!f.provider("codex").join("hooks.json").exists());
    assert!(String::from_utf8_lossy(&response.stderr).contains("configuration failed"));
    f.assert_private(&response, &ticket);
    f.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_connector_is_rejected_before_native_setup() {
    let mut f = Fixture::new();
    f.start().await;
    let ticket = f.ticket().await;
    session_nodes::set_client(&f.node, &f.saved(true)).expect("live connector fixture");
    let before = f.client_bytes();
    let response = f
        .run(&["--runtime", "codex", "--ticket-stdin"], &ticket)
        .await;
    assert!(!response.status.success());
    assert!(String::from_utf8_lossy(&response.stderr).contains("already runs"));
    assert!(f.calls().is_empty());
    assert_eq!(f.client_bytes(), before);
    f.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_or_missing_ticket_and_unsupported_runtime_have_no_setup_side_effects() {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let malformed = format!(
        "cccc-session-node:{}",
        URL_SAFE_NO_PAD.encode(br#"{"v":"SENSITIVE_TICKET_DETAIL"}"#)
    );
    for (args, input) in [
        (
            vec!["--runtime", "codex", "--ticket-stdin"],
            "private-invalid-ticket",
        ),
        (vec!["--runtime", "claude", "--ticket-stdin"], ""),
        (
            vec!["--runtime", "codex", "--ticket-stdin"],
            malformed.as_str(),
        ),
        (vec!["--runtime", "codex"], ""),
        (vec!["--runtime", "pi", "--ticket-stdin"], ""),
        (vec!["--runtime", "opencode", "--ticket-stdin"], ""),
    ] {
        let f = Fixture::new();
        let response = f.run(&args, input).await;
        assert!(!response.status.success());
        assert!(f.calls().is_empty());
        assert!(
            !f.node.root().exists(),
            "invalid input must not start a daemon or initialize Home"
        );
        assert!(!String::from_utf8_lossy(&response.stderr).contains("private-invalid-ticket"));
        assert!(!String::from_utf8_lossy(&response.stderr).contains("SENSITIVE_TICKET_DETAIL"));
    }
}
