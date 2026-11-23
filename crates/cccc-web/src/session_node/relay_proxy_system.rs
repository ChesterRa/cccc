//! Platform-only lookup; environment precedence stays in relay_proxy.
use super::relay_proxy::Selection;
use url::Url;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(windows, test))]
mod windows;

pub(super) fn select(target: &Url) -> Selection {
    #[cfg(target_os = "macos")]
    {
        macos::select(target)
    }
    #[cfg(windows)]
    {
        windows::select(target)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = target;
        Selection::Direct
    }
}

#[cfg(any(target_os = "macos", windows, test))]
pub(super) fn http_connect_proxy(host: &str, port: u16) -> Selection {
    if port == 0 || host.is_empty() || host.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Selection::Unavailable;
    }
    // set_host alone truncates scheme/port suffixes. Parse one standalone host
    // strictly before composing it with the separately supplied port.
    let host = match host.parse::<std::net::Ipv6Addr>() {
        Ok(ip) => url::Host::Ipv6(ip),
        Err(_) => match url::Host::parse(host) {
            Ok(host) => host,
            Err(_) => return Selection::Unavailable,
        },
    };
    let Ok(mut proxy) = Url::parse("http://proxy.invalid/") else {
        return Selection::Unavailable;
    };
    if proxy.set_host(Some(&host.to_string())).is_err() || proxy.set_port(Some(port)).is_err() {
        return Selection::Unavailable;
    }
    Selection::Proxy(proxy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_host_is_a_single_field_not_a_url_or_authority() {
        for host in [
            "http://proxy.invalid",
            "proxy.invalid:garbage",
            "user@proxy.invalid",
            "proxy.invalid/path",
            "proxy.invalid?query",
            "proxy.invalid#fragment",
        ] {
            assert!(matches!(
                http_connect_proxy(host, 8080),
                Selection::Unavailable
            ));
        }
        assert!(
            matches!(http_connect_proxy("2001:db8::1", 8080), Selection::Proxy(url) if url.as_str() == "http://[2001:db8::1]:8080/")
        );
    }
}
