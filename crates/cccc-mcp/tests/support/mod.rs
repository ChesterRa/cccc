//! Helpers shared by the T05-added MCP interface tests.

use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, Value};
use std::time::Duration;

pub fn payload(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}

/// Spawn a daemon for `home` and wait until it answers ping.
pub async fn start_daemon(
    home: &HomeLayout,
) -> (tokio::task::JoinHandle<anyhow::Result<()>>, DaemonClient) {
    let daemon_home = home.clone();
    let task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Map::new(),
            })
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    (task, client)
}

pub async fn stop_daemon(client: &DaemonClient, task: tokio::task::JoinHandle<anyhow::Result<()>>) {
    let _ = client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
}
