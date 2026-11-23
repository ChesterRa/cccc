use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::time::Duration;

async fn call(client: &DaemonClient, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
    client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        })
        .await
        .expect("daemon exchange")
}

enum DeviceState {
    NeverEnabled,
    FileMissing,
    Corrupt,
}

/// Device-session state never stops the daemon or its local session directory: a gateway
/// restore that cannot run fails on its own, management reports the state, nothing listens
/// before a ticket, and shutdown completes.
async fn daemon_lifecycle_with(device: DeviceState) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let state = home.root().join("state/session_nodes/state.json");
    match device {
        DeviceState::NeverEnabled => {}
        DeviceState::FileMissing => {
            std::fs::create_dir_all(state.parent().expect("state directory")).expect("directory")
        }
        DeviceState::Corrupt => {
            std::fs::create_dir_all(state.parent().expect("state directory")).expect("directory");
            std::fs::write(&state, b"{ not json").expect("corrupt state");
        }
    }
    cccc_daemon::session_node_host::register(std::sync::Arc::new(crate::Host));
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone()).without_voice_origin();
    super::fixture::until(async || {
        client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Default::default(),
            })
            .await
            .is_ok()
    })
    .await;

    let local = call(&client, "local_session_list", json!({})).await;
    assert!(local.ok, "{:?}", local.error);
    let status = call(&client, "session_node_status", json!({"by":"user"})).await;
    if matches!(device, DeviceState::Corrupt) {
        assert_eq!(
            status.error.expect("unreadable state").code,
            "session_node_state_unreadable"
        );
        let invite = call(&client, "session_node_invite", json!({"by":"user"})).await;
        assert!(!invite.ok, "A corrupt state must not issue tickets");
    } else {
        assert!(status.ok, "{:?}", status.error);
        assert_eq!(
            status.result.get("endpoint"),
            Some(&Value::Null),
            "{:?}",
            status.result
        );
        assert!(
            !crate::hub::hubs().lock().await.contains_key(home.root()),
            "No gateway listens before the first ticket"
        );
    }

    let _ = call(&client, "shutdown", json!({})).await;
    tokio::time::timeout(Duration::from_secs(10), daemon)
        .await
        .expect("daemon stops")
        .expect("daemon task")
        .expect("daemon shutdown");
    match device {
        DeviceState::Corrupt => {
            assert_eq!(std::fs::read(&state).expect("state kept"), b"{ not json")
        }
        _ => assert!(
            !state.exists(),
            "Starting and stopping must not create device state"
        ),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn daemon_lifecycle_ignores_device_sessions_that_were_never_enabled() {
    daemon_lifecycle_with(DeviceState::NeverEnabled).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn daemon_lifecycle_tolerates_a_missing_device_state_file() {
    daemon_lifecycle_with(DeviceState::FileMissing).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unreadable_device_state_leaves_daemon_lifecycle_intact() {
    daemon_lifecycle_with(DeviceState::Corrupt).await;
}
