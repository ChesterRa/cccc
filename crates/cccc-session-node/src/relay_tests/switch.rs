use super::fixture::Fixture;
use cccc_core::{session_nodes, settings};
use serde_json::Value;

async fn relay_running(f: &Fixture) -> bool {
    let hubs = crate::hub::hubs().lock().await;
    let hub = hubs.get(f.home.root()).expect("fixture hub");
    hub.relay.lock().await.is_some()
}

fn set_remote_access(f: &Fixture, enabled: bool) {
    settings::update(&f.home, |settings| {
        settings
            .remote_access
            .insert("enabled".into(), Value::Bool(enabled));
        Ok(())
    })
    .expect("remote access setting");
}

/// The loopback relay follows Remote Access changes made after the gateway opened, and stays
/// down while the administrator's switch is off.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_relay_follows_remote_access_and_the_switch() {
    let f = Fixture::new().await;
    assert!(relay_running(&f).await);

    set_remote_access(&f, false);
    crate::hub::converge_relay(&f.home).await;
    assert!(!relay_running(&f).await);
    assert!(
        !session_nodes::relay_status(&f.home)
            .expect("status")
            .effective
    );
    assert!(session_nodes::relay_enabled(&f.home).expect("switch"));

    set_remote_access(&f, true);
    // Until the gateway converges, the withdrawn relay must not count as effective.
    assert!(
        !session_nodes::relay_status(&f.home)
            .expect("status")
            .effective
    );
    crate::hub::converge_relay(&f.home).await;
    assert!(relay_running(&f).await);
    assert!(
        session_nodes::relay_status(&f.home)
            .expect("status")
            .effective
    );

    crate::hub::set_relay(&f.home, false)
        .await
        .expect("switch off");
    assert!(!relay_running(&f).await);
    crate::hub::converge_relay(&f.home).await;
    assert!(!relay_running(&f).await);
    let ticket = crate::hub::create_invitation(&f.home, 0)
        .await
        .expect("ticket");
    assert_eq!(ticket.relay_url, None);
    f.close().await;
}
