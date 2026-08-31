use std::net::{IpAddr, Ipv4Addr};

use cccc_core::HomeLayout;
use serde_json::Value;

use crate::api::ApiError;

pub(super) fn preferred_issuer_endpoint(
    home: &HomeLayout,
    submitted: &str,
    lan_ip: Option<Ipv4Addr>,
) -> Result<String, ApiError> {
    let submitted = submitted.trim();
    if submitted.is_empty() {
        let public_url = requester_endpoint(home);
        if public_url.is_empty() {
            return Err(ApiError::bad("issuer_endpoint is required"));
        }
        return normalize_endpoint(&public_url);
    }

    let submitted = normalize_endpoint(submitted)?;
    if !is_loopback_endpoint(&submitted)? {
        return Ok(submitted);
    }

    local_advertised_endpoint(home, &submitted, lan_ip)
}

pub(super) fn requester_endpoint(home: &HomeLayout) -> String {
    cccc_core::settings::load(home)
        .ok()
        .and_then(|settings| {
            settings
                .remote_access
                .get("web_public_url")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| nonempty_env("CCCC_WEB_PUBLIC_URL"))
        .unwrap_or_default()
}

fn local_advertised_endpoint(
    home: &HomeLayout,
    submitted: &str,
    lan_ip: Option<Ipv4Addr>,
) -> Result<String, ApiError> {
    let settings =
        cccc_core::settings::load(home).map_err(|error| ApiError::bad(error.to_string()))?;
    let config = &settings.remote_access;
    let bind_host = config
        .get("web_host")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| nonempty_env("CCCC_WEB_HOST"))
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let advertised_host = match bind_host.as_str() {
        "0.0.0.0" | "::" | "[::]" => lan_ip.map(|ip| ip.to_string()),
        "127.0.0.1" | "localhost" | "::1" | "[::1]" => None,
        _ => Some(bind_host),
    };
    let Some(advertised_host) = advertised_host else {
        return Ok(submitted.to_owned());
    };

    let submitted_url = reqwest::Url::parse(submitted)
        .map_err(|_| ApiError::bad("issuer_endpoint must be an http(s) URL"))?;
    let port = config
        .get("web_port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
        .or_else(|| nonempty_env("CCCC_WEB_PORT").and_then(|value| value.parse::<u16>().ok()))
        .unwrap_or(8848);
    normalize_endpoint(&format!(
        "{}://{}:{port}",
        submitted_url.scheme(),
        bracket_ipv6(&advertised_host)
    ))
}

pub(super) fn normalize_endpoint(raw: &str) -> Result<String, ApiError> {
    let url = reqwest::Url::parse(raw)
        .map_err(|_| ApiError::bad("issuer_endpoint must be an http(s) URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ApiError::bad("invalid issuer_endpoint"));
    }
    if url.scheme() == "http"
        && !insecure_http_allowed(url.host_str().unwrap_or(""))
        && !environment_flag("CCCC_GROUP_BRIDGE_ALLOW_INSECURE_HTTP")
    {
        return Err(ApiError::bad(
            "public Group Bridge endpoints require https; plain http is limited to loopback/private addresses",
        ));
    }
    Ok(format!(
        "{}://{}{}",
        url.scheme(),
        bracket_ipv6(url.host_str().unwrap_or("")),
        url.port().map_or(String::new(), |port| format!(":{port}"))
    ))
}

fn insecure_http_allowed(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|address| match address {
        IpAddr::V4(address) => {
            address.is_loopback() || address.is_private() || is_shared_overlay_ipv4(address)
        }
        IpAddr::V6(address) => address.is_loopback() || address.is_unique_local(),
    })
}

fn is_shared_overlay_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, _, _] = address.octets();
    first == 100 && (64..=127).contains(&second)
}

fn environment_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn is_loopback_endpoint(endpoint: &str) -> Result<bool, ApiError> {
    let url = reqwest::Url::parse(endpoint)
        .map_err(|_| ApiError::bad("issuer_endpoint must be an http(s) URL"))?;
    let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
    Ok(host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback()))
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn bracket_ipv6(host: &str) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

#[cfg(test)]
#[path = "group_bridge_pairing_endpoint_tests.rs"]
mod tests;
