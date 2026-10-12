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

pub(super) async fn post(
    value: &str,
    headers: HeaderMap,
    body: Vec<u8>,
    read_response: bool,
) -> Result<(u16, Vec<u8>), &'static str> {
    let target = url(value)?;
    let host = match target.host().ok_or("invalid_callback_url")? {
        url::Host::Domain(host) => host.to_owned(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => ip.to_string(),
    };
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
    // Pin this connection to the addresses just checked. No inherited proxy or redirect.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .resolve_to_addrs(&host, &addresses)
        .build()
        .map_err(|_| "callback_client_failed")?;
    let mut response = client
        .post(target)
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|_| "callback_transport_failed")?;
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
}
