//! Destination-scoped HTTP(S) proxy selection; never pass credentials to iroh logs.
use hyper_util::client::proxy::matcher::Matcher;
use url::Url;

pub(super) enum Selection {
    Direct,
    Proxy(Url),
    Unavailable,
}

fn variable(upper: &str, lower: &str) -> String {
    std::env::var(upper)
        .or_else(|_| std::env::var(lower))
        .unwrap_or_default()
}

pub(super) fn select(relay: &Url) -> Selection {
    let bypass = variable("NO_PROXY", "no_proxy");
    // hyper-util 0.1.20's wildcard domain matcher does not cover IP literals.
    if cccc_core::local_network::is_loopback_url(relay)
        || bypass.split(',').any(|entry| entry.trim() == "*")
    {
        return Selection::Direct;
    }
    let Ok(target) = relay.as_str().parse() else {
        return Selection::Unavailable;
    };
    let Some(proxy) = Matcher::from_env().intercept(&target) else {
        let protocol = if relay.scheme() == "https" {
            variable("HTTPS_PROXY", "https_proxy")
        } else {
            variable("HTTP_PROXY", "http_proxy")
        };
        let configured =
            !protocol.trim().is_empty() || !variable("ALL_PROXY", "all_proxy").trim().is_empty();
        // Reuse the library's NO_PROXY decision even when its proxy parser fails.
        // This placeholder is used only for matching and is never dialed.
        let bypassed = Matcher::builder()
            .all("http://unused.invalid")
            .no(bypass)
            .build()
            .intercept(&target)
            .is_none();
        return if configured && !bypassed {
            Selection::Unavailable
        } else {
            Selection::Direct
        };
    };
    // The matcher removes userinfo from uri(); ignoring its separate auth field
    // would silently turn an authenticated proxy into an anonymous one.
    if proxy.basic_auth().is_some()
        || proxy.raw_auth().is_some()
        || !matches!(proxy.uri().scheme_str(), Some("http" | "https"))
    {
        return Selection::Unavailable;
    }
    match Url::parse(&proxy.uri().to_string()) {
        Ok(url) => Selection::Proxy(url),
        Err(_) => Selection::Unavailable,
    }
}
