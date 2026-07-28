use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::Map;
use std::time::Duration;

#[tokio::test]
async fn cancelled_daemon_cleans_runtime_files_and_releases_home_lock() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let paths = cccc_daemon::DaemonPaths::new(home.clone());

    let daemon = tokio::spawn(cccc_daemon::run(home.clone()));
    wait_until(|| paths.address.exists()).await;
    daemon.abort();
    assert!(
        daemon
            .await
            .expect_err("daemon should be cancelled")
            .is_cancelled()
    );

    assert!(!paths.address.exists());
    assert!(!paths.pid.exists());
    assert!(!paths.socket.exists());

    let restarted = tokio::spawn(cccc_daemon::run(home.clone()));
    wait_until(|| paths.address.exists()).await;
    let response = DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await
        .expect("request shutdown");
    assert!(response.ok);
    tokio::time::timeout(Duration::from_secs(5), restarted)
        .await
        .expect("daemon shutdown timeout")
        .expect("daemon task")
        .expect("daemon result");
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition was not met before timeout"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
// Included by the crate-level integration test harness.
