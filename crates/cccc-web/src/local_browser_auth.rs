use axum::extract::{ConnectInfo, Request};
use axum::http::{Method, header};
use std::net::{IpAddr, SocketAddr};

use crate::AppState;

pub(crate) fn allowed(state: &AppState, request: &Request) -> bool {
    let peer_is_loopback = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|peer| peer.0.ip().is_loopback());
    allowed_with_proxy(
        request,
        crate::request_origin::proxy_headers_trusted(state),
        peer_is_loopback,
    )
}

fn allowed_with_proxy(request: &Request, trust_proxy: bool, peer_is_loopback: bool) -> bool {
    if !peer_is_loopback || forwarded_client_is_remote(request.headers()) {
        return false;
    }
    let headers = request.headers();
    let Some(served_origin) = crate::request_origin::served_origin_with_proxy(headers, trust_proxy)
    else {
        return false;
    };
    if !crate::request_origin::origin_is_loopback(&served_origin) {
        return false;
    }
    let websocket = headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if matches!(request.method(), &Method::GET | &Method::HEAD) && !websocket {
        return true;
    }
    crate::request_origin::source_origin(headers).is_some_and(|source| {
        source == served_origin && crate::request_origin::origin_is_loopback(&source)
    })
}

fn forwarded_client_is_remote(headers: &axum::http::HeaderMap) -> bool {
    let mut values = ["cf-connecting-ip", "x-real-ip", "x-forwarded-for"]
        .into_iter()
        .filter_map(|name| headers.get(name))
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(value) = crate::request_origin::forwarded_parameter(headers, "for") {
        values.push(value);
    }
    values
        .iter()
        .any(|value| !forwarded_address_is_loopback(value))
}

fn forwarded_address_is_loopback(value: &str) -> bool {
    let value = value.trim().trim_matches('"');
    value
        .parse::<IpAddr>()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|address| address.ip()))
        .or_else(|| {
            value
                .strip_prefix('[')
                .and_then(|value| value.split_once(']'))
                .and_then(|(address, _)| address.parse::<IpAddr>().ok())
        })
        .is_some_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    #[test]
    fn local_access_requires_both_a_loopback_peer_and_served_origin() {
        let local = Request::builder()
            .uri("/api/v1/groups")
            .header(header::HOST, "127.0.0.1:8848")
            .body(Body::empty())
            .expect("request");
        assert!(allowed_with_proxy(&local, false, true));
        assert!(!allowed_with_proxy(&local, false, false));

        let public = Request::builder()
            .uri("/api/v1/groups")
            .header(header::HOST, "cccc.example")
            .body(Body::empty())
            .expect("request");
        assert!(!allowed_with_proxy(&public, false, true));
    }

    #[test]
    fn local_writes_and_websockets_require_a_same_origin_loopback_source() {
        for upgrade in [None, Some("websocket")] {
            let mut request = Request::builder()
                .method(if upgrade.is_some() {
                    Method::GET
                } else {
                    Method::POST
                })
                .uri("/api/v1/groups")
                .header(header::HOST, "localhost:8848");
            if let Some(upgrade) = upgrade {
                request = request.header(header::UPGRADE, upgrade);
            }
            let missing_origin = request.body(Body::empty()).expect("request");
            assert!(!allowed_with_proxy(&missing_origin, false, true));
        }

        let local_write = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/groups")
            .header(header::HOST, "localhost:8848")
            .header(header::ORIGIN, "http://localhost:8848")
            .body(Body::empty())
            .expect("request");
        assert!(allowed_with_proxy(&local_write, false, true));
    }

    #[test]
    fn proxy_client_headers_must_all_describe_loopback_sources() {
        for (name, value) in [
            ("x-forwarded-for", "203.0.113.10"),
            (
                "forwarded",
                "for=203.0.113.10;proto=http;host=127.0.0.1:8848",
            ),
        ] {
            let request = Request::builder()
                .uri("/api/v1/groups")
                .header(header::HOST, "127.0.0.1:8848")
                .header(name, value)
                .body(Body::empty())
                .expect("request");
            assert!(!allowed_with_proxy(&request, true, true), "{name}");
        }
    }
}
