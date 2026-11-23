//! Pure selection differences and one routing contract, with isolated environments.
use super::proxy_process as process;
use crate::{
    network,
    relay_proxy::{self, Selection},
};
use cccc_contracts::session_node::SessionNodeEndpoint;
use iroh::SecretKey;

const TEST: &str = "relay_tests::proxy_contract::target_selection_and_route_contracts";
const TARGET: &str = "https://relay.proxy.invalid/";
const PROXY: &str = "http://proxy.invalid:8001/";

#[tokio::test]
async fn target_selection_and_route_contracts() {
    if let Ok(case) = std::env::var("CCCC_PROXY_POLICY_CASE") {
        check(&case);
        return;
    }
    let home = tempfile::tempdir().expect("isolated contract Home");
    for (case, variable, value) in [
        ("all", "ALL_PROXY", PROXY),
        ("http_only", "HTTP_PROXY", PROXY),
        ("wildcard_ip", "HTTPS_PROXY", PROXY),
        ("loopback", "HTTP_PROXY", PROXY),
        ("socks", "HTTPS_PROXY", "socks5://proxy.invalid:8001/"),
        ("invalid", "HTTPS_PROXY", "not a valid proxy URL"),
        ("route", "HTTPS_PROXY", "socks5://proxy.invalid:8001/"),
    ] {
        let mut command = process::command(TEST, home.path());
        command
            .env("CCCC_PROXY_POLICY_CASE", case)
            .env(variable, value);
        if case == "wildcard_ip" {
            command.env("NO_PROXY", "*");
        }
        let private: &[&str] = if case == "route" {
            &["route-fixture-relay-token"]
        } else {
            &[]
        };
        process::run(command, b"", private).await;
    }
}

fn check(case: &str) {
    let target = match case {
        "wildcard_ip" => "https://203.0.113.4/",
        "loopback" => "http://127.0.0.2:8000/",
        _ => TARGET,
    };
    if case == "route" {
        check_route();
        return;
    }
    let selected =
        relay_proxy::select_with_system(&url::Url::parse(target).expect("target"), |_| {
            Selection::Direct
        });
    match case {
        "all" => {
            let Selection::Proxy(actual) = selected else {
                panic!("ALL_PROXY must be selected");
            };
            assert_eq!(actual, url::Url::parse(PROXY).expect("expected proxy"));
        }
        "http_only" | "wildcard_ip" | "loopback" => assert!(matches!(selected, Selection::Direct)),
        "socks" | "invalid" => assert!(matches!(selected, Selection::Unavailable)),
        _ => panic!("unknown policy case"),
    }
}

fn check_route() {
    let mut route = SessionNodeEndpoint {
        endpoint_id: SecretKey::generate().public().to_string(),
        addresses: vec!["127.0.0.1:41000".into()],
        relay_url: Some(TARGET.into()),
    };
    let (builder, destination) = network::connect_builder(
        SecretKey::generate(),
        &route,
        Some("route-fixture-relay-token"),
    )
    .expect("direct route must survive");
    drop(builder); // No bind, DNS or network services in the route contract.
    assert_eq!(
        destination.ip_addrs().copied().collect::<Vec<_>>(),
        ["127.0.0.1:41000".parse().expect("IP")]
    );
    assert!(destination.relay_urls().next().is_none());
    route.addresses.clear();
    let error = network::connect_builder(
        SecretKey::generate(),
        &route,
        Some("route-fixture-relay-token"),
    )
    .map(|_| ())
    .expect_err("relay-only route must fail clearly");
    assert!(error.to_string().contains("no direct IP addresses"));
    route.relay_url = None;
    let original = network::destination(&route)
        .expect_err("missing route")
        .to_string();
    let actual = network::connect_builder(SecretKey::generate(), &route, None)
        .map(|_| ())
        .expect_err("missing route")
        .to_string();
    assert_eq!(
        actual, original,
        "a corrupt descriptor is not a proxy failure"
    );
}
