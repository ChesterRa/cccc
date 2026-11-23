use super::super::relay_http;
use super::fixture::Fixture;
use iroh::SecretKey;
use tokio::{net::TcpListener, sync::broadcast};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn public_facade_denies_unknown_device_keys_and_exhibit_access() {
    let f = Fixture::new().await;
    let endpoint = f.endpoint(SecretKey::generate(), Some(&f.ticket)).await;
    let denied = super::fixture::relay_client(
        &endpoint,
        f.ticket.relay_url.as_deref().expect("relay"),
        SecretKey::generate(),
    )
    .auth_token("a-browser-admin-token-is-not-a-device-grant")
    .connect()
    .await;
    assert!(
        denied
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string().contains("Session-node relay authority")),
        "Unexpected denial: {denied:?}"
    );
    let socket = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("exhibit facade");
    let url = format!("http://{}/", socket.local_addr().expect("address"));
    let (shutdown, _) = broadcast::channel(2);
    let routes = relay_http::routes(f.home.clone(), true, shutdown);
    let server = tokio::spawn(async move {
        axum::serve(socket, routes).await.expect("exhibit");
    });
    let denied = super::fixture::relay_client(&endpoint, &url, SecretKey::generate())
        .auth_token(format!(
            "{}{}:{}",
            super::super::relay_access::AUTH_PREFIX,
            f.ticket.invite_id,
            f.ticket.relay_token.as_deref().expect("relay capability")
        ))
        .connect()
        .await;
    assert!(
        denied
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string().contains("403")),
        "Unexpected exhibit outcome: {denied:?}"
    );
    server.abort();
    endpoint.close().await;
    f.close().await;
}
