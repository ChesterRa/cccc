//! Shared precedence with a controlled OS boundary, independent of this machine.
use super::super::{
    relay_proxy::{Selection, select_with_system},
    relay_proxy_system::http_connect_proxy,
};
use super::proxy_process as process;
use url::Url;

const TEST: &str =
    "session_node::relay_tests::proxy_system::environment_precedes_destination_system_policy";

#[tokio::test]
async fn environment_precedes_destination_system_policy() {
    if let Ok(case) = std::env::var("CCCC_SYSTEM_PROXY_CASE") {
        check(&case);
        return;
    }
    let home = tempfile::tempdir().expect("isolated Home");
    for case in [
        "system",
        "all",
        "empty",
        "no_proxy",
        "loopback",
        "unavailable",
    ] {
        let mut command = process::command(TEST, home.path());
        command.env("CCCC_SYSTEM_PROXY_CASE", case);
        if case == "all" {
            command.env("ALL_PROXY", "http://explicit.proxy.invalid:8001");
        }
        if case == "empty" {
            command.env("HTTPS_PROXY", "");
            // Windows names are case-insensitive, so only Unix can express
            // distinct upper/lower values for this precedence check.
            #[cfg(not(windows))]
            command.env("https_proxy", "http://ignored.proxy.invalid:8001");
        }
        if case == "no_proxy" {
            command.env("NO_PROXY", ".example");
        }
        process::run(command, b"", &[]).await;
    }
}

fn check(case: &str) {
    let target = Url::parse(if case == "loopback" {
        "http://127.0.0.1:8080/"
    } else {
        "https://relay.example/"
    })
    .expect("target");
    let selected = select_with_system(&target, |actual| {
        assert!(
            matches!(case, "system" | "unavailable"),
            "explicit policy must skip OS lookup"
        );
        assert_eq!(actual, &target);
        if case == "unavailable" {
            Selection::Unavailable
        } else {
            http_connect_proxy("system.proxy.invalid", 8888)
        }
    });
    match case {
        "system" | "all" => {
            let Selection::Proxy(proxy) = selected else {
                panic!("destination requires its selected proxy");
            };
            let expected = if case == "all" {
                "http://explicit.proxy.invalid:8001/"
            } else {
                "http://system.proxy.invalid:8888/"
            };
            assert_eq!(proxy.as_str(), expected);
        }
        "empty" | "no_proxy" | "loopback" => assert!(matches!(selected, Selection::Direct)),
        "unavailable" => assert!(matches!(selected, Selection::Unavailable)),
        _ => panic!("unknown case"),
    }
}
