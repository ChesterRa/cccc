//! Destination-scoped HTTP(S) proxy selection; never pass credentials to iroh logs.
use hyper_util::client::proxy::matcher::Matcher;
use url::Url;

pub(crate) enum Selection {
    Direct,
    Proxy(Url),
    Unavailable,
}

fn variable(upper: &str, lower: &str) -> Option<String> {
    std::env::var(upper).or_else(|_| std::env::var(lower)).ok()
}

pub(crate) fn select(relay: &Url) -> Selection {
    select_with_system(relay, super::relay_proxy_system::select)
}

pub(crate) fn select_with_system(relay: &Url, system: impl FnOnce(&Url) -> Selection) -> Selection {
    let bypass = variable("NO_PROXY", "no_proxy").unwrap_or_default();
    // hyper-util 0.1.20's wildcard domain matcher does not cover IP literals.
    if cccc_core::local_network::is_loopback_url(relay)
        || bypass.split(',').any(|entry| entry.trim() == "*")
    {
        return Selection::Direct;
    }
    let Ok(target) = relay.as_str().parse() else {
        return Selection::Unavailable;
    };
    // This placeholder is used only for NO_PROXY matching and is never dialed.
    if Matcher::builder()
        .all("http://unused.invalid")
        .no(bypass)
        .build()
        .intercept(&target)
        .is_none()
    {
        return Selection::Direct;
    }
    let protocol = if relay.scheme() == "https" {
        variable("HTTPS_PROXY", "https_proxy")
    } else {
        variable("HTTP_PROXY", "http_proxy")
    };
    let all = variable("ALL_PROXY", "all_proxy");
    if protocol.is_none() && all.is_none() {
        return system(relay);
    }
    let Some(proxy) = Matcher::from_env().intercept(&target) else {
        // An explicit empty setting suppresses system fallback. Invalid or
        // unsupported nonempty settings must never turn into public bare access.
        return if protocol
            .iter()
            .chain(all.iter())
            .any(|value| !value.trim().is_empty())
        {
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
