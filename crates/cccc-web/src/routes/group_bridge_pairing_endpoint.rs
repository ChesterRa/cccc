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
    Ok(format!(
        "{}://{}{}",
        url.scheme(),
        bracket_ipv6(url.host_str().unwrap_or("")),
        url.port().map_or(String::new(), |port| format!(":{port}"))
    ))
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
mod tests {
    use super::*;

    fn home_with_settings(settings: &str) -> (tempfile::TempDir, HomeLayout) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(home.root().join("settings.yaml"), settings).expect("settings");
        (temp, home)
    }

    #[test]
    fn submitted_public_https_origin_wins_atomically() {
        let (_temp, home) = home_with_settings(
            "remote_access:\n  web_host: 0.0.0.0\n  web_port: 80\n  web_public_url: http://fallback.example\n",
        );

        assert_eq!(
            preferred_issuer_endpoint(
                &home,
                "https://cccc.tae.vera-mesh.com/pairing?source=ui#invite",
                Some(Ipv4Addr::new(172, 30, 92, 65)),
            )
            .expect("endpoint"),
            "https://cccc.tae.vera-mesh.com"
        );
    }

    #[test]
    fn submitted_nonstandard_port_is_preserved() {
        let (_temp, home) =
            home_with_settings("remote_access:\n  web_host: 0.0.0.0\n  web_port: 80\n");

        assert_eq!(
            preferred_issuer_endpoint(&home, "https://bridge.example:9443/ui", None)
                .expect("endpoint"),
            "https://bridge.example:9443"
        );
    }

    #[test]
    fn empty_submission_falls_back_to_public_url() {
        let (_temp, home) = home_with_settings(
            "remote_access:\n  web_host: 0.0.0.0\n  web_port: 80\n  web_public_url: https://fallback.example:9443/ui?x=1\n",
        );

        assert_eq!(
            preferred_issuer_endpoint(&home, "  ", Some(Ipv4Addr::new(172, 30, 92, 65)))
                .expect("endpoint"),
            "https://fallback.example:9443"
        );
    }

    #[test]
    fn localhost_submission_uses_lan_compatibility_boundary() {
        let (_temp, home) =
            home_with_settings("remote_access:\n  web_host: 0.0.0.0\n  web_port: 9000\n");

        assert_eq!(
            preferred_issuer_endpoint(
                &home,
                "https://localhost:5555",
                Some(Ipv4Addr::new(192, 168, 1, 20)),
            )
            .expect("endpoint"),
            "https://192.168.1.20:9000"
        );
    }

    #[test]
    fn localhost_submission_stays_when_binding_is_loopback() {
        let (_temp, home) = home_with_settings("remote_access: {}\n");

        assert_eq!(
            preferred_issuer_endpoint(
                &home,
                "http://localhost:5555",
                Some(Ipv4Addr::new(192, 168, 1, 20)),
            )
            .expect("endpoint"),
            "http://localhost:5555"
        );
    }

    #[test]
    fn ipv6_origin_remains_well_formed() {
        let (_temp, home) = home_with_settings("remote_access: {}\n");

        assert_eq!(
            preferred_issuer_endpoint(&home, "https://[2001:db8::1]:9443/path", None)
                .expect("endpoint"),
            "https://[2001:db8::1]:9443"
        );
    }

    #[test]
    fn invalid_or_missing_origins_are_rejected() {
        let (_temp, home) = home_with_settings("remote_access: {}\n");

        for endpoint in [
            "ftp://bridge.example",
            "https://user@bridge.example",
            "https://",
        ] {
            assert!(preferred_issuer_endpoint(&home, endpoint, None).is_err());
        }
        assert!(preferred_issuer_endpoint(&home, "", None).is_err());
    }

    #[test]
    fn requester_advertises_configured_public_web_endpoint() {
        let (_temp, home) =
            home_with_settings("remote_access:\n  web_public_url: https://requester.example\n");
        assert_eq!(requester_endpoint(&home), "https://requester.example");
    }
}
