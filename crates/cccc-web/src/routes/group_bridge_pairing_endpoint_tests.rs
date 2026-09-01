use super::*;

fn home_with_settings(settings: &str) -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    std::fs::write(home.root().join("settings.yaml"), settings).expect("settings");
    (temp, home)
}

fn endpoint(
    home: &HomeLayout,
    submitted: &str,
    lan_ip: Option<Ipv4Addr>,
) -> Result<String, ApiError> {
    preferred_issuer_endpoint(home, submitted, lan_ip, "127.0.0.1", 8848)
}

#[test]
fn submitted_public_https_origin_wins_atomically() {
    let (_temp, home) = home_with_settings(
        "remote_access:\n  web_host: 0.0.0.0\n  web_port: 80\n  web_public_url: http://fallback.example\n",
    );

    assert_eq!(
        endpoint(
            &home,
            "https://cccc.tae.vera-mesh.com/pairing?source=ui#invite",
            Some(Ipv4Addr::new(172, 30, 92, 65)),
        )
        .expect("endpoint"),
        "https://cccc.tae.vera-mesh.com"
    );
}

#[test]
fn submitted_nonstandard_port_is_preserved() {
    let (_temp, home) = home_with_settings("remote_access:\n  web_host: 0.0.0.0\n  web_port: 80\n");

    assert_eq!(
        endpoint(&home, "https://bridge.example:9443/ui", None).expect("endpoint"),
        "https://bridge.example:9443"
    );
}

#[test]
fn empty_submission_falls_back_to_public_url() {
    let (_temp, home) = home_with_settings(
        "remote_access:\n  web_host: 0.0.0.0\n  web_port: 80\n  web_public_url: https://fallback.example:9443/ui?x=1\n",
    );

    assert_eq!(
        endpoint(&home, "  ", Some(Ipv4Addr::new(172, 30, 92, 65))).expect("endpoint"),
        "https://fallback.example:9443"
    );
}

#[test]
fn localhost_submission_does_not_inherit_a_stale_saved_port() {
    let (_temp, home) =
        home_with_settings("remote_access:\n  web_host: 0.0.0.0\n  web_port: 9000\n");

    assert_eq!(
        preferred_issuer_endpoint(
            &home,
            "https://localhost:5555",
            Some(Ipv4Addr::new(192, 168, 1, 20)),
            "0.0.0.0",
            5555,
        )
        .expect("endpoint"),
        "https://192.168.1.20:5555"
    );
}

#[test]
fn localhost_submission_stays_when_binding_is_loopback() {
    let (_temp, home) = home_with_settings("remote_access: {}\n");

    assert_eq!(
        endpoint(
            &home,
            "http://localhost:5555",
            Some(Ipv4Addr::new(192, 168, 1, 20)),
        )
        .expect("endpoint"),
        "http://localhost:5555"
    );
}

#[test]
fn ipv6_origin_remains_well_formed() {
    let (_temp, home) = home_with_settings("remote_access: {}\n");

    assert_eq!(
        endpoint(&home, "https://[2001:db8::1]:9443/path", None).expect("endpoint"),
        "https://[2001:db8::1]:9443"
    );
}

#[test]
fn invalid_or_missing_origins_are_rejected() {
    let (_temp, home) = home_with_settings("remote_access: {}\n");

    for endpoint in [
        "ftp://bridge.example",
        "http://bridge.example",
        "https://user@bridge.example",
        "https://",
    ] {
        assert!(self::endpoint(&home, endpoint, None).is_err());
    }
    assert!(endpoint(&home, "", None).is_err());
}

#[test]
fn tailscale_cgnat_http_endpoint_is_allowed_as_a_private_overlay() {
    let (_temp, home) = home_with_settings("remote_access: {}\n");

    assert_eq!(
        endpoint(&home, "http://100.100.10.20:8848", None).expect("tailnet endpoint"),
        "http://100.100.10.20:8848"
    );
    assert!(endpoint(&home, "http://100.128.0.1:8848", None).is_err());
}

#[test]
fn requester_advertises_configured_public_web_endpoint() {
    let (_temp, home) =
        home_with_settings("remote_access:\n  web_public_url: https://requester.example\n");
    assert_eq!(requester_endpoint(&home), "https://requester.example");
}
