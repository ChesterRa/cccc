use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use reqwest::header::{AUTHORIZATION, COOKIE, HOST, ORIGIN, SET_COOKIE};

#[tokio::test]
async fn supervised_loopback_web_trusts_forwarded_https_for_auth_and_security() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let token = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve port");
    let port = listener.local_addr().expect("address").port();
    drop(listener);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_home = home.clone();
    let server = tokio::spawn(async move {
        cccc_web::serve_until_mode_supervised(
            server_home,
            "127.0.0.1",
            port,
            cccc_web::WebMode::Normal,
            async move {
                let _ = shutdown_rx.await;
            },
        )
        .await
    });
    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{port}");
    wait_until_ready(&client, &base_url).await;

    let session = client
        .get(format!("{base_url}/api/v1/web_access/session"))
        .header(AUTHORIZATION, format!("Bearer {}", token.token))
        .header(HOST, format!("127.0.0.1:{port}"))
        .header("x-forwarded-host", "reach.example")
        .header("x-forwarded-proto", "https")
        .send()
        .await
        .expect("session request");
    assert!(session.status().is_success());
    assert_eq!(
        session
            .headers()
            .get("strict-transport-security")
            .and_then(|value| value.to_str().ok()),
        Some("max-age=31536000")
    );
    assert!(
        session
            .headers()
            .get(SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("Secure"))
    );

    let logout = client
        .post(format!("{base_url}/api/v1/web_access/logout"))
        .header(COOKIE, format!("cccc_access_token={}", token.token))
        .header(HOST, format!("127.0.0.1:{port}"))
        .header(ORIGIN, "https://reach.example")
        .header("x-forwarded-host", "reach.example")
        .header("x-forwarded-proto", "https")
        .send()
        .await
        .expect("logout request");
    assert!(logout.status().is_success());

    shutdown_tx.send(()).expect("shutdown receiver");
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .expect("server shutdown timeout")
        .expect("server task")
        .expect("server result");
    assert!(matches!(outcome, cccc_web::ServeOutcome::Stopped(_)));
}

async fn wait_until_ready(client: &reqwest::Client, base_url: &str) {
    for _ in 0..100 {
        if client
            .get(format!("{base_url}/api/v1/ready"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("supervised Web server did not become ready");
}
