//! Exercise the real per-URL Apple engine with in-memory settings only.
use super::*;

fn text(value: &str) -> CFType {
    CFString::new(value).as_CFType()
}
fn number(value: i64) -> CFType {
    CFNumber::from(value).as_CFType()
}
fn settings(values: Vec<(&str, CFType)>) -> Settings {
    let pairs = values
        .into_iter()
        .map(|(key, value)| (CFString::new(key), value))
        .collect::<Vec<_>>();
    Settings::from_CFType_pairs(&pairs)
}
fn manual(extra: Vec<(&str, CFType)>) -> Settings {
    let mut values = vec![
        ("HTTPSEnable", number(1)),
        ("HTTPSProxy", text("proxy.invalid")),
        ("HTTPSPort", number(8080)),
    ];
    values.extend(extra);
    settings(values)
}

#[test]
fn static_tunnel_respects_system_exceptions_and_simple_hosts() {
    let configured = manual(vec![
        ("SOCKSEnable", number(1)),
        ("SOCKSProxy", text("socks.invalid")),
        ("SOCKSPort", number(1080)),
    ]);
    let Proxy::Http { host, port } = resolve("https://relay.example/", &configured) else {
        panic!("expected the static HTTPS-target tunnel");
    };
    assert_eq!((host.as_str(), port), ("proxy.invalid", 8080));
    let exceptions = CFArray::from_CFTypes(&[CFString::new("*.internal.example")]).as_CFType();
    let bypass = manual(vec![
        ("ExceptionsList", exceptions),
        ("ExcludeSimpleHostnames", number(1)),
    ]);
    assert!(matches!(
        resolve("https://relay.internal.example/", &bypass),
        Proxy::Direct
    ));
    assert!(matches!(resolve("https://relay/", &bypass), Proxy::Direct));
    assert!(matches!(
        resolve("https://relay.example/", &settings(vec![])),
        Proxy::Direct
    ));
}

#[test]
fn automatic_or_visible_credentials_never_become_direct_fallbacks() {
    for automatic in ["ProxyAutoConfigEnable", "ProxyAutoDiscoveryEnable"] {
        let automatic_only = settings(vec![(automatic, number(1))]);
        assert!(matches!(
            resolve("https://relay.example/", &automatic_only),
            Proxy::Unavailable
        ));
        let mixed = manual(vec![(automatic, number(1))]);
        assert!(matches!(
            resolve("https://relay.example/", &mixed),
            Proxy::Unavailable
        ));
    }
    let credentials = manual(vec![("HTTPSUser", text("private-proxy-user"))]);
    assert!(matches!(
        resolve("https://relay.example/", &credentials),
        Proxy::Unavailable
    ));
    let socks = settings(vec![
        ("SOCKSEnable", number(1)),
        ("SOCKSProxy", text("socks.invalid")),
        ("SOCKSPort", number(1080)),
    ]);
    assert!(matches!(
        resolve("https://relay.example/", &socks),
        Proxy::Unavailable
    ));
}
