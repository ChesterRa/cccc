use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use std::time::Duration;
use tokio::task::JoinHandle;

pub async fn start(home: HomeLayout, daemons: &mut Vec<JoinHandle<anyhow::Result<()>>>) {
    home.initialize().expect("initialize");
    let daemon_home = home.clone();
    daemons.push(tokio::spawn(
        async move { cccc_daemon::run(daemon_home).await },
    ));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if cccc_client::DaemonClient::new(home.clone())
                .call(&DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("isolated daemon ready");
}
