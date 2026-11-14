use super::*;

fn configured() -> Fixture {
    let f = Fixture::new();
    crate::settings::update(&f.home, |settings| {
        settings.remote_access.extend(
            serde_json::json!({
                "enabled":true,"provider":"manual","web_public_url":"https://relay.example.test/ui/"
            })
            .as_object()
            .expect("settings")
            .clone(),
        );
        Ok(())
    })
    .expect("public settings");
    register_relay(
        &f.home,
        "gateway",
        "127.0.0.1:3456".parse().expect("local relay"),
    )
    .expect("owned relay");
    f
}

#[test]
fn relay_tickets_extend_routes_without_changing_the_direct_ticket_or_application_protocol() {
    let direct = Fixture::new();
    let ticket = create_invite(&direct.home).expect("legacy ticket");
    assert_eq!(ticket.v, SESSION_NODE_PROTOCOL);
    assert!(ticket.relay_url.is_none());
    let legacy = serde_json::to_value(&ticket).expect("legacy wire shape");
    assert!(legacy.get("relay_url").is_none());
    let decoded: SessionNodeTicket =
        serde_json::from_value(legacy).expect("old ticket remains readable");
    assert!(
        pair(
            &direct.home,
            "gateway",
            "legacy-node",
            "Legacy",
            Some((&decoded.invite_id, &decoded.secret))
        )
        .is_ok()
    );
    let f = configured();
    let ticket = create_invite(&f.home).expect("relay ticket");
    assert_eq!(ticket.v, SESSION_NODE_RELAY_TICKET);
    assert_eq!(
        ticket.relay_url.as_deref(),
        Some("https://relay.example.test/")
    );
    assert_eq!(ticket.addresses, ["127.0.0.1:2345"]);
    assert_eq!(SESSION_NODE_PROTOCOL, 1);
    assert!(
        register_relay(
            &f.home,
            "wrong-owner",
            "127.0.0.1:4567".parse().expect("address")
        )
        .is_err()
    );
    assert!(register_relay(&f.home, "gateway", "0.0.0.0:4567".parse().expect("address")).is_err());
    assert_eq!(
        relay_address(&f.home).expect("relay"),
        Some("127.0.0.1:3456".parse().expect("owned address"))
    );
}

#[path = "relay_authority_tests.rs"]
mod authority;

#[test]
fn public_relay_addresses_require_a_secure_origin_and_transport_disable_keeps_device_grants() {
    for value in [
        "http://remote.example/",
        "https://user:password@remote.example/",
        "https://remote.example/?token=private",
        "https://remote.example/prefix",
        "https://remote.example/#fragment",
    ] {
        assert!(normalize_relay_url(value).is_err(), "{value}");
    }
    assert_eq!(
        normalize_relay_url("https://remote.example/ui/").expect("Web URL"),
        "https://remote.example/"
    );
    assert_eq!(
        normalize_relay_url("http://127.0.0.1:3456/").expect("isolated loopback relay"),
        "http://127.0.0.1:3456/"
    );
    let f = configured();
    let peer = f.pair("node");
    crate::settings::update(&f.home, |settings| {
        settings
            .remote_access
            .insert("enabled".into(), serde_json::json!(false));
        Ok(())
    })
    .expect("disable remote route");
    assert!(
        relay_public_url(&f.home)
            .expect("no public relay")
            .is_none()
    );
    assert!(authorize_relay(&f.home, "gateway", "node", None).is_err());
    assert!(relay_authorities(&f.home, "gateway").is_err());
    assert_eq!(
        pair(&f.home, "gateway", "node", "Laptop", None)
            .expect("direct grant remains valid")
            .grant_id,
        peer.grant_id
    );
}

#[test]
fn existing_web_addresses_without_relay_support_keep_direct_pairing_available() {
    let f = configured();
    let original = f.pair("existing-node");
    for (index, value) in [
        "http://192.168.1.20:8848/ui/",
        "https://web.example.test/cccc/ui/",
    ]
    .into_iter()
    .enumerate()
    {
        crate::settings::update(&f.home, |settings| {
            settings
                .remote_access
                .insert("web_public_url".into(), serde_json::json!(value));
            Ok(())
        })
        .expect("existing Web access settings");
        assert!(normalize_relay_url(value).is_err());
        assert!(relay_public_url(&f.home).expect("optional relay").is_none());
        let ticket = create_invite(&f.home).expect("direct invitation remains available");
        assert_eq!(ticket.v, SESSION_NODE_PROTOCOL);
        assert!(ticket.relay_url.is_none() && ticket.relay_token.is_none());
        pair(
            &f.home,
            "gateway",
            &format!("new-node-{index}"),
            "New direct device",
            Some((&ticket.invite_id, &ticket.secret)),
        )
        .expect("new direct pairing");
        assert!(authorize_relay(&f.home, "gateway", "existing-node", None).is_err());
        assert_eq!(
            pair(&f.home, "gateway", "existing-node", "Laptop", None)
                .expect("direct resume retains its grant")
                .grant_id,
            original.grant_id
        );
    }
}
