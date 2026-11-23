//! Four representative network boundaries; pure policies are in proxy_contract.
use super::{fixture::Fixture, proxy_fixture as wire, proxy_process as process};
use std::net::SocketAddr;

const TEST: &str = "session_node::relay_tests::proxy::environment_proxy_routes_real_relay_and_preserves_authentication";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn environment_proxy_routes_real_relay_and_preserves_authentication() {
    if let Ok(case) = std::env::var("CCCC_PROXY_CLIENT") {
        super::proxy_connector::run(&case).await;
        return;
    }
    let home = tempfile::tempdir().expect("worker Home");
    if std::env::var_os("CCCC_PROXY_WORKER").is_none() {
        let mut command = process::command(TEST, home.path());
        command.env("CCCC_PROXY_WORKER", "1");
        process::run(command, b"", &[]).await;
        return;
    }
    for case in ["http", "https", "bypass", "udp"] {
        let mut fixture = Fixture::new().await;
        let local = url::Url::parse(fixture.ticket.relay_url.as_deref().expect("local facade"))
            .expect("URL");
        let upstream: SocketAddr = format!(
            "{}:{}",
            local.host_str().expect("host"),
            local.port().expect("port")
        )
        .parse()
        .expect("local address");
        let tls = wire::tls(upstream).await;
        let (proxy, requests) = wire::proxy(tls.address).await;
        let proxy_tls = if case == "https" {
            Some(wire::tls(proxy.address).await)
        } else {
            None
        };
        fixture.ticket.relay_url = Some(format!("https://{}:{}/", wire::HOST, tls.address.port()));
        fixture.ticket.addresses.retain(|address| {
            address
                .parse::<SocketAddr>()
                .is_ok_and(|addr| addr.ip().is_loopback())
        });
        let mut command = process::command(TEST, home.path());
        command.env("CCCC_PROXY_CLIENT", case);
        let proxy_url = match &proxy_tls {
            Some(server) => format!("https://{}:{}", wire::HOST, server.address.port()),
            None => format!("http://{}", proxy.address),
        };
        command.env("HTTPS_PROXY", proxy_url);
        if case == "http" {
            command.env("ALL_PROXY", format!("socks5://{}", proxy.address));
        }
        if case == "bypass" {
            command.env("NO_PROXY", ".proxy.invalid");
        }
        if case == "udp" {
            command.env(
                "HTTPS_PROXY",
                format!(
                    "http://proxy-fixture-user:proxy-fixture-password@{}",
                    proxy.address
                ),
            );
        }
        let mut private = vec![
            fixture.ticket.secret.as_str(),
            fixture
                .ticket
                .relay_token
                .as_deref()
                .expect("relay capability"),
        ];
        if case == "udp" {
            private.extend(["proxy-fixture-user", "proxy-fixture-password"]);
        }
        process::run(
            command,
            &serde_json::to_vec(&fixture.ticket).expect("synthetic ticket"),
            &private,
        )
        .await;
        {
            let requests = requests.lock().expect("requests");
            if matches!(case, "http" | "https") {
                assert!(
                    !requests.is_empty(),
                    "public relay must traverse CONNECT ({case})"
                );
                for head in requests.iter() {
                    assert!(head.starts_with(&format!(
                        "CONNECT {}:{} ",
                        wire::HOST,
                        tls.address.port()
                    )));
                    assert!(!head.contains(&fixture.ticket.secret));
                    assert!(
                        !head.contains(
                            fixture
                                .ticket
                                .relay_token
                                .as_deref()
                                .expect("relay capability")
                        )
                    );
                }
            } else {
                assert!(
                    requests.is_empty(),
                    "bypassed or disabled relay must not reach the proxy"
                );
            }
        }
        let accepted = tls.connections.load(std::sync::atomic::Ordering::SeqCst);
        if case == "udp" {
            assert_eq!(
                accepted, 0,
                "an unsupported proxy must not become bare public relay access"
            );
        } else {
            assert!(accepted > 0, "the real TLS relay must be reached");
        }
        if let Some(server) = proxy_tls {
            assert!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst) > 0,
                "the HTTPS proxy must complete a real TLS connection"
            );
        }
        fixture.close().await;
    }
}
