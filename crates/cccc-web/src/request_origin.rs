use axum::http::{HeaderMap, header};

use crate::AppState;

fn first_list_value(value: &str) -> Option<String> {
    value
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(crate) fn forwarded_parameter(headers: &HeaderMap, name: &str) -> Option<String> {
    let first = headers
        .get("forwarded")
        .and_then(|value| value.to_str().ok())?
        .split(',')
        .next()?;
    first.split(';').find_map(|part| {
        let (key, raw_value) = part.split_once('=')?;
        if !key.trim().eq_ignore_ascii_case(name) {
            return None;
        }
        let value = raw_value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value)
            .trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn forwarded_host(headers: &HeaderMap, trust_proxy: bool) -> Option<String> {
    if !trust_proxy {
        return None;
    }
    headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .and_then(first_list_value)
        .or_else(|| forwarded_parameter(headers, "host"))
}

fn forwarded_scheme(headers: &HeaderMap, trust_proxy: bool) -> Option<String> {
    if !trust_proxy {
        return None;
    }
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(first_list_value)
        .or_else(|| forwarded_parameter(headers, "proto"))
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| matches!(value.as_str(), "http" | "https"))
}

pub(crate) fn proxy_headers_trusted(state: &AppState) -> bool {
    proxy_headers_trusted_for(
        state.restart.is_some(),
        environment_flag("CCCC_WEB_TRUST_PROXY_HEADERS"),
        Some(&state.live_binding.host),
    )
}

fn proxy_headers_trusted_for(
    supervised: bool,
    explicitly_trusted: bool,
    effective_host: Option<&str>,
) -> bool {
    explicitly_trusted || supervised && effective_host.is_some_and(is_loopback_host)
}

fn environment_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim();
    host.eq_ignore_ascii_case("localhost")
        || host
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .unwrap_or(host)
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub fn served_origin(state: &AppState, headers: &HeaderMap) -> Option<String> {
    served_origin_with_proxy(headers, proxy_headers_trusted(state))
}

pub(crate) fn served_origin_with_proxy(headers: &HeaderMap, trust_proxy: bool) -> Option<String> {
    let host = forwarded_host(headers, trust_proxy).or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })?;
    if host.is_empty() {
        return None;
    }
    let scheme = forwarded_scheme(headers, trust_proxy).unwrap_or_else(|| "http".into());
    cccc_core::web_login_grants::normalize_origin(&format!("{scheme}://{host}"))
}

pub(crate) fn origin_is_loopback(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    url.host_str().is_some_and(is_loopback_host)
}

pub fn source_origin(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return cccc_core::web_login_grants::normalize_origin(origin);
    }
    headers
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())
        .and_then(cccc_core::web_login_grants::normalize_origin)
}

pub(crate) fn origin_allowed_with_proxy(
    headers: &HeaderMap,
    origin: &str,
    trust_proxy: bool,
) -> bool {
    let Some(origin) = cccc_core::web_login_grants::normalize_origin(origin) else {
        return false;
    };
    if served_origin_with_proxy(headers, trust_proxy).as_deref() == Some(origin.as_str()) {
        return true;
    }
    configured_origins().any(|allowed| allowed == origin)
}

pub fn cookie_csrf_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    cookie_csrf_allowed_with_proxy(headers, proxy_headers_trusted(state))
}

pub(crate) fn cookie_csrf_allowed_with_proxy(headers: &HeaderMap, trust_proxy: bool) -> bool {
    source_origin(headers)
        .is_some_and(|origin| origin_allowed_with_proxy(headers, &origin, trust_proxy))
}

pub fn is_https(state: &AppState, headers: &HeaderMap) -> bool {
    served_origin(state, headers).is_some_and(|origin| origin.starts_with("https://"))
}

fn configured_origins() -> impl Iterator<Item = String> {
    std::env::var("CCCC_WEB_CORS_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|value| {
            let value = value.trim();
            (!value.is_empty() && value != "*")
                .then(|| cccc_core::web_login_grants::normalize_origin(value))
                .flatten()
        })
        .collect::<Vec<_>>()
        .into_iter()
}

#[cfg(test)]
#[path = "request_origin_tests.rs"]
mod tests;
