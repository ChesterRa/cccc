//! Signed public HTTPS delivery. Callback URLs and errors never enter logs.
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue};
use sha2::Sha256;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use url::Url;

const MAX_RESPONSE: usize = 16 * 1024;

pub(super) fn key(secret: &str) -> Result<Vec<u8>, &'static str> {
    let value = secret
        .strip_prefix("whsec_")
        .ok_or("invalid_signing_secret")?;
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| "invalid_signing_secret")?;
    if !(24..=64).contains(&bytes.len()) {
        return Err("invalid_signing_secret");
    }
    Ok(bytes)
}

pub(super) fn url(value: &str) -> Result<Url, &'static str> {
    if value.len() > 4096 || value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("invalid_callback_url");
    }
    let result = Url::parse(value).map_err(|_| "invalid_callback_url")?;
    if result.scheme() != "https"
        || result.host_str().is_none()
        || !result.username().is_empty()
        || result.password().is_some()
        || result.port_or_known_default() != Some(443)
        || result.fragment().is_some()
    {
        return Err("invalid_callback_url");
    }
    match result.host() {
        Some(url::Host::Ipv4(ip)) if !public_address(IpAddr::V4(ip)) => {
            return Err("non_public_callback");
        }
        Some(url::Host::Ipv6(ip)) if !public_address(IpAddr::V6(ip)) => {
            return Err("non_public_callback");
        }
        _ => {}
    }
    Ok(result)
}

fn v4_prefix(ip: Ipv4Addr, network: [u8; 4], prefix: u32) -> bool {
    let mask = u32::MAX << (32 - prefix);
    u32::from(ip) & mask == u32::from(Ipv4Addr::from(network)) & mask
}

pub(super) fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let excluded = [
                ([0, 0, 0, 0], 8),
                ([10, 0, 0, 0], 8),
                ([100, 64, 0, 0], 10),
                ([127, 0, 0, 0], 8),
                ([169, 254, 0, 0], 16),
                ([172, 16, 0, 0], 12),
                ([192, 0, 0, 0], 24),
                ([192, 0, 2, 0], 24),
                ([192, 88, 99, 0], 24),
                ([192, 168, 0, 0], 16),
                ([198, 18, 0, 0], 15),
                ([198, 51, 100, 0], 24),
                ([203, 0, 113, 0], 24),
                ([224, 0, 0, 0], 4),
                ([240, 0, 0, 0], 4),
            ];
            !excluded
                .iter()
                .any(|(net, prefix)| v4_prefix(ip, *net, *prefix))
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_address(IpAddr::V4(mapped));
            }
            let segments = ip.segments();
            // Global unicast only, excluding documentation and transition ranges.
            segments[0] & 0xe000 == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
        }
    }
}

pub(super) fn headers(
    subscription_id: &str,
    secret: &str,
    event_id: &str,
    timestamp: i64,
    body: &[u8],
    previous: Option<&str>,
) -> Result<HeaderMap, &'static str> {
    let timestamp = timestamp.to_string();
    let mut signatures = Vec::new();
    for secret in std::iter::once(secret).chain(previous) {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&key(secret)?).map_err(|_| "invalid_signing_secret")?;
        mac.update(event_id.as_bytes());
        mac.update(b".");
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(body);
        signatures.push(format!(
            "v1,{}",
            STANDARD.encode(mac.finalize().into_bytes())
        ));
    }
    let mut result = HeaderMap::new();
    for (name, value) in [
        ("content-type", "application/json".to_owned()),
        ("webhook-id", event_id.to_owned()),
        ("webhook-timestamp", timestamp),
        ("webhook-signature", signatures.join(" ")),
        ("x-mcp-subscription-id", subscription_id.to_owned()),
    ] {
        result.insert(
            name,
            HeaderValue::from_str(&value).map_err(|_| "invalid_callback_header")?,
        );
    }
    Ok(result)
}

pub(super) fn matches_challenge(secret: &str, expected: &str, actual: &str) -> bool {
    let Ok(bytes) = key(secret) else {
        return false;
    };
    let Ok(mut expected_mac) = Hmac::<Sha256>::new_from_slice(&bytes) else {
        return false;
    };
    let Ok(mut actual_mac) = Hmac::<Sha256>::new_from_slice(&bytes) else {
        return false;
    };
    expected_mac.update(expected.as_bytes());
    actual_mac.update(actual.as_bytes());
    expected_mac
        .verify_slice(&actual_mac.finalize().into_bytes())
        .is_ok()
}

/// Domains a proxied callback may reach. Through a proxy the address is
/// resolved there, so the public-address check is replaced by this list:
/// only ChatGPT's own callback hosts.
const PROXIED_CALLBACK_DOMAINS: [&str; 2] = ["openai.com", "chatgpt.com"];

fn proxied_host_allowed(host: &str) -> bool {
    PROXIED_CALLBACK_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

/// The proxy for a callback host: HTTPS_PROXY, else ALL_PROXY (either case),
/// unless NO_PROXY covers the host. Where direct HTTPS to ChatGPT is blocked,
/// only the proxy makes callbacks dependable.
fn proxy_for(host: &str, env: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let var = |name: &str| {
        env(name)
            .or_else(|| env(&name.to_ascii_lowercase()))
            .filter(|value| !value.trim().is_empty())
    };
    let host = host.to_ascii_lowercase();
    let host = host.as_str();
    let bypassed = var("NO_PROXY").is_some_and(|list| {
        list.split(',').map(str::trim).any(|entry| {
            let entry = entry.trim_start_matches('.').to_ascii_lowercase();
            let entry = entry.as_str();
            entry == "*"
                || (!entry.is_empty()
                    && (host == entry
                        || host
                            .strip_suffix(entry)
                            .is_some_and(|rest| rest.ends_with('.'))))
        })
    });
    if bypassed {
        return None;
    }
    let proxy = var("HTTPS_PROXY").or_else(|| var("ALL_PROXY"))?;
    Some(remote_dns(proxy.trim()))
}

/// SOCKS5 and SOCKS4 resolve the target on this machine, which would skip the
/// address check with nothing in its place; their `h`/`a` forms hand the name
/// to the proxy. HTTP proxies already do, through CONNECT.
fn remote_dns(proxy: &str) -> String {
    let Ok(mut url) = Url::parse(proxy) else {
        return proxy.to_owned();
    };
    let scheme = match url.scheme() {
        "socks5" => "socks5h",
        "socks4" => "socks4a",
        _ => return proxy.to_owned(),
    };
    match url.set_scheme(scheme) {
        Ok(()) => url.to_string(),
        Err(()) => proxy.to_owned(),
    }
}

pub(super) async fn post(
    value: &str,
    headers: HeaderMap,
    body: Vec<u8>,
    read_response: bool,
) -> Result<(u16, Vec<u8>), &'static str> {
    post_with(value, headers, body, read_response, &|name| {
        std::env::var(name).ok()
    })
    .await
}

async fn post_with(
    value: &str,
    headers: HeaderMap,
    body: Vec<u8>,
    read_response: bool,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<(u16, Vec<u8>), &'static str> {
    let target = url(value)?;
    let host = match target.host().ok_or("invalid_callback_url")? {
        url::Host::Domain(host) => host.to_owned(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => ip.to_string(),
    };
    // An address literal never goes through a proxy: it keeps the checks below.
    let proxy = matches!(target.host(), Some(url::Host::Domain(_)))
        .then(|| proxy_for(&host, env))
        .flatten();
    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10));
    let (client, failure) = if let Some(proxy) = proxy {
        if !proxied_host_allowed(&host) {
            return Err("callback_host_not_allowed");
        }
        let proxy = reqwest::Proxy::all(&proxy).map_err(|_| "callback_proxy_invalid")?;
        let client = builder
            .proxy(proxy)
            .build()
            .map_err(|_| "callback_client_failed")?;
        (client, "callback_proxy_failed")
    } else {
        let addresses: Vec<SocketAddr> = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host((host.as_str(), 443)),
        )
        .await
        .map_err(|_| "callback_dns_timeout")?
        .map_err(|_| "callback_dns_failed")?
        .collect();
        if addresses.is_empty() || addresses.iter().any(|a| !public_address(a.ip())) {
            return Err("non_public_callback");
        }
        // Pin this connection to the addresses just checked. No inherited proxy.
        let client = builder
            .no_proxy()
            .resolve_to_addrs(&host, &addresses)
            .build()
            .map_err(|_| "callback_client_failed")?;
        (client, "callback_transport_failed")
    };
    let mut response = client
        .post(target)
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|_| failure)?;
    let status = response.status().as_u16();
    if (300..400).contains(&status) {
        return Err("callback_redirect_rejected");
    }
    if !read_response {
        return Ok((status, Vec::new()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE as u64)
    {
        return Err("callback_response_too_large");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "callback_transport_failed")?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE {
            return Err("callback_response_too_large");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((status, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_rejects_private_mapped_multicast_and_unsafe_urls() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.1.1",
            "192.168.1.1",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:224.0.0.1",
            "2001:db8::1",
            "2002:7f00:1::",
        ] {
            assert!(!public_address(ip.parse().expect("IP")), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_address(ip.parse().expect("IP")), "{ip}");
        }
        for value in [
            "http://example.com/callback",
            "https://user:password@example.com/",
            "https://example.com:8848/",
            "https://example.com/#fragment",
            "https://127.0.0.1/",
            "https://[::1]/",
        ] {
            assert!(url(value).is_err());
        }
        assert!(url("https://example.com/callback?id=synthetic").is_ok());
    }

    #[test]
    fn signature_covers_id_timestamp_and_exact_body_bytes() {
        // Published Standard Webhooks test_sign_function vector (synthetic).
        // https://github.com/standard-webhooks/standard-webhooks/blob/7537d2a2d3d52d8f2e0ecd12527af4a9307fd81b/libraries/python/tests/test_webhooks.py#L214
        // Keep the published fixture bytes instead of a credential-shaped literal.
        let vector_secret = format!(
            "whsec_{}",
            STANDARD.encode([
                49_u8, 242, 144, 246, 191, 6, 41, 138, 171, 79, 8, 212, 60, 63, 8, 44, 246, 72,
                163, 98, 218, 45, 164, 176
            ])
        );
        let vector = headers(
            "sub_fixture",
            &vector_secret,
            "msg_p5jXN8AQM9LWM0D4loKWxJek",
            1614265330,
            b"{\"test\": 2432232314}",
            None,
        )
        .expect("published vector");
        assert_eq!(
            vector["webhook-signature"],
            "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="
        );
        let secret = format!("whsec_{}", STANDARD.encode([7_u8; 32]));
        assert!(matches_challenge(
            &secret,
            "synthetic-challenge",
            "synthetic-challenge"
        ));
        assert!(!matches_challenge(
            &secret,
            "synthetic-challenge",
            "different"
        ));
        let one = headers("sub_test", &secret, "evt_test", 123, b"{\"ok\":true}", None)
            .expect("signature");
        let other = headers(
            "sub_test",
            &secret,
            "evt_test",
            123,
            b"{ \"ok\":true}",
            None,
        )
        .expect("signature");
        assert_ne!(one["webhook-signature"], other["webhook-signature"]);
        let rotated =
            headers("sub_test", &secret, "evt_test", 123, b"{}", Some(&secret)).expect("rotation");
        assert_eq!(
            rotated["webhook-signature"]
                .to_str()
                .expect("header")
                .split(' ')
                .count(),
            2
        );
        assert!(key("whsec_invalid!").is_err());
        assert!(key(&format!("whsec_{}", STANDARD.encode([0_u8; 8]))).is_err());
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn callback_proxy_follows_https_then_all_proxy_and_honours_no_proxy() {
        let host = "connectors.api.openai.com";
        assert_eq!(proxy_for(host, &env(&[])), None);
        assert_eq!(
            proxy_for(host, &env(&[("ALL_PROXY", "socks5://127.0.0.1:7890")])).as_deref(),
            Some("socks5h://127.0.0.1:7890")
        );
        assert_eq!(
            proxy_for(
                host,
                &env(&[
                    ("https_proxy", "http://127.0.0.1:7890"),
                    ("ALL_PROXY", "socks5://x:1")
                ])
            )
            .as_deref(),
            Some("http://127.0.0.1:7890")
        );
        for bypass in [".openai.com", "api.openai.com", "*"] {
            let pairs = [("HTTPS_PROXY", "http://p:1"), ("NO_PROXY", bypass)];
            assert_eq!(proxy_for(host, &env(&pairs)), None, "{bypass}");
        }
        // Every spelling reqwest accepts gets remote DNS.
        for (raw, wanted) in [
            ("SOCKS5://127.0.0.1:7890", "socks5h://127.0.0.1:7890"),
            ("  socks5://127.0.0.1:7890 ", "socks5h://127.0.0.1:7890"),
            ("socks4://127.0.0.1:1080", "socks4a://127.0.0.1:1080"),
            ("socks5h://127.0.0.1:7890", "socks5h://127.0.0.1:7890"),
        ] {
            let pairs = [("ALL_PROXY", raw)];
            assert_eq!(
                proxy_for(host, &env(&pairs)).as_deref(),
                Some(wanted),
                "{raw}"
            );
        }
        let pairs = [("HTTPS_PROXY", "http://p:1"), ("NO_PROXY", ".OPENAI.COM")];
        assert_eq!(proxy_for(host, &env(&pairs)), None);
        // A suffix that is not a whole label does not bypass.
        assert!(
            proxy_for(
                host,
                &env(&[("HTTPS_PROXY", "http://p:1"), ("NO_PROXY", "ai.com")])
            )
            .is_some()
        );
    }

    #[test]
    fn proxied_callbacks_reach_only_chatgpt_hosts() {
        for host in ["connectors.api.openai.com", "openai.com", "x.chatgpt.com"] {
            assert!(proxied_host_allowed(host), "{host}");
        }
        for host in [
            "example.com",
            "evilopenai.com",
            "openai.com.evil.test",
            "localhost",
        ] {
            assert!(!proxied_host_allowed(host), "{host}");
        }
    }

    #[tokio::test]
    async fn a_configured_proxy_carries_the_callback_and_names_its_own_failure() {
        // Nothing listens on port 1: the request must fail at the proxy, without
        // resolving the callback host locally (which is what times out today).
        let proxy = env(&[("HTTPS_PROXY", "http://127.0.0.1:1")]);
        let result = post_with(
            "https://connectors.api.openai.com/callback",
            HeaderMap::new(),
            Vec::new(),
            false,
            &proxy,
        )
        .await;
        assert_eq!(result, Err("callback_proxy_failed"));
        let other = post_with(
            "https://example.com/hook",
            HeaderMap::new(),
            Vec::new(),
            false,
            &proxy,
        )
        .await;
        assert_eq!(other, Err("callback_host_not_allowed"));
        // An address literal still gets the public-address check.
        let private = post_with(
            "https://10.0.0.1/hook",
            HeaderMap::new(),
            Vec::new(),
            false,
            &proxy,
        )
        .await;
        assert_eq!(private, Err("non_public_callback"));
    }

    #[tokio::test]
    async fn a_socks_proxy_receives_the_callback_host_name_not_a_local_address() {
        for scheme in ["socks5", "SOCKS5"] {
            socks_proxy_receives_the_host_name(scheme).await;
        }
    }

    async fn socks_proxy_receives_the_host_name(scheme: &str) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("proxy");
        let port = listener.local_addr().expect("address").port();
        // Minimal SOCKS5: accept no-auth, then report the CONNECT address type.
        // Bounded: if the request never reaches the proxy, fail instead of waiting.
        let proxy = tokio::spawn(tokio::time::timeout(Duration::from_secs(5), async move {
            let (mut socket, _) = listener.accept().await.expect("client");
            let mut greeting = [0_u8; 2];
            socket.read_exact(&mut greeting).await.expect("greeting");
            let mut methods = vec![0_u8; greeting[1] as usize];
            socket.read_exact(&mut methods).await.expect("methods");
            socket.write_all(&[5, 0]).await.expect("choose no auth");
            let mut request = [0_u8; 4];
            socket.read_exact(&mut request).await.expect("request");
            let target = if request[3] == 3 {
                let mut length = [0_u8; 1];
                socket.read_exact(&mut length).await.expect("length");
                let mut name = vec![0_u8; length[0] as usize];
                socket.read_exact(&mut name).await.expect("name");
                Some(String::from_utf8(name).expect("utf8"))
            } else {
                None
            };
            // Refuse the connection; only the requested address matters here.
            let _ = socket.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await;
            target
        }));
        let address = format!("{scheme}://127.0.0.1:{port}");
        let pairs = [("ALL_PROXY", address.as_str())];
        let result = post_with(
            "https://connectors.api.openai.com/callback",
            HeaderMap::new(),
            Vec::new(),
            false,
            &env(&pairs),
        )
        .await;
        let target = proxy
            .await
            .expect("proxy task")
            .unwrap_or_else(|_| panic!("{scheme}: the callback never reached the proxy"));
        assert_eq!(result, Err("callback_proxy_failed"));
        assert_eq!(
            target.as_deref(),
            Some("connectors.api.openai.com"),
            "{scheme}: the proxy, not this machine, must resolve the callback host"
        );
    }
}
