//! Manual browser fixture. Uses the production router and isolated daemons, with
//! only the generated loopback certificate added to this fixture's trust roots.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    homes: Vec<std::path::PathBuf>,
    ca_certificate: std::path::PathBuf,
    ready_file: std::path::PathBuf,
}

#[tokio::test]
#[ignore = "requires the isolated connect-workbench.mjs browser harness"]
async fn connect_browser_fixture_when_enabled() {
    let path = std::env::var_os("CCCC_CONNECT_BROWSER_FIXTURE")
        .expect("an isolated browser fixture config is required");
    let config: Config = serde_json::from_slice(&std::fs::read(path).expect("fixture config"))
        .expect("fixture config JSON");
    assert_eq!(
        config.homes.len(),
        3,
        "fixture requires three isolated Homes"
    );
    let certificate = reqwest::Certificate::from_pem(
        &std::fs::read(&config.ca_certificate).expect("fixture certificate"),
    )
    .expect("PEM certificate");
    let client = connect_frames::http_client()
        .no_proxy()
        .add_root_certificate(certificate)
        .build()
        .expect("fixture TLS client");
    let mut addresses = Vec::new();
    let mut tasks = tokio::task::JoinSet::new();
    for path in config.homes {
        let home = HomeLayout::from_path(path).expect("fixture Home");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener");
        let address = listener.local_addr().expect("listener address");
        let (shutdown, _) = broadcast::channel(1);
        let (_, _, _, mut state) = app_with_shutdown(
            home,
            shutdown,
            WebMode::Normal,
            None,
            LiveBinding::from_env(),
            new_web_runtime_id(),
        );
        state.connect_http = Ok(client.clone());
        tasks.spawn(async move {
            axum::serve(listener, router_for_state(state))
                .into_future()
                .await
                .expect("fixture Web server");
        });
        addresses.push(format!("http://{address}"));
    }
    std::fs::write(
        config.ready_file,
        serde_json::to_vec(&addresses).expect("addresses JSON"),
    )
    .expect("ready file");
    tasks
        .join_next()
        .await
        .expect("fixture servers stay running")
        .expect("server task");
}
