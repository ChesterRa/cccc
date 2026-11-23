//! Internet Options manual lists use semicolons or whitespace between entries.
use crate::relay_proxy::Selection;
use url::{Host, Url};

#[cfg(windows)]
pub(crate) fn select(target: &Url) -> Selection {
    use cccc_system_proxy::windows::{Configuration, current_user};
    match current_user() {
        Configuration::Direct => Selection::Direct,
        Configuration::Manual { server, bypass } => manual(target, &server, &bypass),
        Configuration::Unavailable => Selection::Unavailable,
    }
}

fn entries(list: &str) -> impl Iterator<Item = &str> {
    list.split(|c: char| c == ';' || c.is_ascii_whitespace())
        .filter(|entry| !entry.is_empty())
}

fn manual(target: &Url, server: &str, bypass: &str) -> Selection {
    if bypasses(target, bypass) {
        return Selection::Direct;
    }
    let mut generic = None;
    let mut socks = false;
    for entry in entries(server) {
        if let Some((scheme, endpoint)) = entry.split_once('=')
            && scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"+.-".contains(&c))
            && scheme
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
        {
            if scheme.eq_ignore_ascii_case(target.scheme()) {
                return proxy(endpoint);
            }
            socks |= scheme.eq_ignore_ascii_case("socks");
        } else {
            generic.get_or_insert(entry);
        }
    }
    match generic {
        Some(endpoint) => proxy(endpoint),
        None if socks => Selection::Unavailable,
        None => Selection::Direct,
    }
}

fn proxy(entry: &str) -> Selection {
    let authority = entry.split_once("://").map_or(entry, |(_, rest)| rest);
    if authority
        .trim_end_matches('/')
        .contains(['@', '/', '\\', '?', '#'])
    {
        return Selection::Unavailable;
    }
    // In "https=host:port", HTTPS names the destination scheme. A bare proxy uses
    // plain HTTP CONNECT; only an explicit https:// proxy URL asks for TLS to the proxy.
    let url = if entry.contains("://") {
        Url::parse(entry)
    } else {
        Url::parse(&format!("http://{entry}"))
    };
    let Ok(url) = url else {
        return Selection::Unavailable;
    };
    if !matches!(url.scheme(), "http" | "https") || url.path() != "/" {
        return Selection::Unavailable;
    }
    let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
        return Selection::Unavailable;
    };
    match super::http_connect_proxy(host, port) {
        Selection::Proxy(_) => Selection::Proxy(url),
        _ => Selection::Unavailable,
    }
}

fn bypasses(target: &Url, bypass: &str) -> bool {
    let Some(host) = target.host_str() else {
        return false;
    };
    let authority = target
        .port_or_known_default()
        .map(|port| format!("{host}:{port}"));
    entries(bypass).any(|rule| {
        if rule.eq_ignore_ascii_case("<local>") {
            return matches!(target.host(), Some(Host::Domain(name)) if !name.contains('.'));
        }
        let pattern = match rule.split_once("://") {
            Some((scheme, pattern)) if scheme.eq_ignore_ascii_case(target.scheme()) => pattern,
            Some(_) => return false,
            None => rule,
        };
        wildcard(pattern, host)
            || (pattern.contains(':')
                && (wildcard(pattern, host.trim_start_matches('[').trim_end_matches(']'))
                    || authority.as_deref().is_some_and(|a| wildcard(pattern, a))))
    })
}

/// Match an entire host or authority, with case-insensitive ASCII and Windows * / ? wildcards.
fn wildcard(pattern: &str, host: &str) -> bool {
    let (pattern, host) = (pattern.as_bytes(), host.as_bytes());
    let (mut p, mut h) = (0, 0);
    let mut star = None;
    while h < host.len() {
        if pattern.get(p) == Some(&b'*') {
            p += 1;
            star = Some((p, h));
        } else if pattern
            .get(p)
            .is_some_and(|c| *c == b'?' || c.eq_ignore_ascii_case(&host[h]))
        {
            p += 1;
            h += 1;
        } else if let Some((resume, previous)) = star {
            h = previous + 1;
            p = resume;
            star = Some((resume, h));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_list_selects_a_destination_without_leaking_proxy_credentials() {
        let target = Url::parse("https://relay.example:8443/").expect("relay URL");
        for (setting, expected) in [
            ("proxy.example:8080", "http://proxy.example:8080/"),
            (
                "HTTP=other:80;  HtTpS=chosen:3128 socks=other:1080",
                "http://chosen:3128/",
            ),
            ("https=https://chosen:80", "https://chosen:80/"),
            ("https=[2001:db8::1]:8080", "http://[2001:db8::1]:8080/"),
        ] {
            assert!(
                matches!(manual(&target, setting, ""), Selection::Proxy(url) if url.as_str()==expected)
            );
        }
        assert!(matches!(
            manual(&target, "http=other:80", ""),
            Selection::Direct
        ));
        for unsupported in [
            "socks=127.0.0.1:1080",
            "https=socks5://other:1080",
            "https=fixture-user:fixture-password@proxy:80",
            "https=proxy:0",
            "https=",
        ] {
            assert!(matches!(
                manual(&target, unsupported, ""),
                Selection::Unavailable
            ));
        }
    }

    #[test]
    fn bypass_uses_windows_host_patterns_and_the_effective_port() {
        for (url, bypass, expected) in [
            ("https://build/", "<LOCAL>", true),
            ("https://host.example/", "<local>", false),
            ("https://[2001:db8::1]/", "<local>", false),
            (
                "https://node.corp.example:8443/",
                "exact.example; *.CORP.EXAMPLE:8443",
                true,
            ),
            ("https://node.corp.example/", "*.corp.example:8443", false),
            ("https://x-internal-01.example/", "x-*-0?.example", true),
            ("https://node.example/", "https://NODE.example:443", true),
            ("https://other.example/", "exact.example", false),
            ("https://192.0.2.13/", "192.0.2.*", true),
        ] {
            let target = Url::parse(url).expect("relay URL");
            assert_eq!(bypasses(&target, bypass), expected);
        }
        let target = Url::parse("https://internal.example/").expect("relay URL");
        assert!(matches!(
            manual(&target, "socks=other:1080", "internal.example"),
            Selection::Direct
        ));
    }
}
