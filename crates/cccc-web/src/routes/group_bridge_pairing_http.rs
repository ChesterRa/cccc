use reqwest::{Method, Response};
use serde_json::{Value, json};
use std::error::Error as _;
use std::time::Duration;

const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REMOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
struct RemoteTimeouts {
    connect: Duration,
    request: Duration,
}

const PRODUCTION_TIMEOUTS: RemoteTimeouts = RemoteTimeouts {
    connect: REMOTE_CONNECT_TIMEOUT,
    request: REMOTE_REQUEST_TIMEOUT,
};

pub(super) async fn post_remote(endpoint: &str, path: &str, body: &Value) -> (Value, String) {
    send_remote(
        Method::POST,
        endpoint,
        path,
        Some(body),
        "remote pairing request",
        PRODUCTION_TIMEOUTS,
    )
    .await
}

pub(super) async fn get_remote(endpoint: &str, path: &str) -> (Value, String) {
    send_remote(
        Method::GET,
        endpoint,
        path,
        None,
        "remote pairing status",
        PRODUCTION_TIMEOUTS,
    )
    .await
}

async fn send_remote(
    method: Method,
    endpoint: &str,
    path: &str,
    body: Option<&Value>,
    operation: &str,
    timeouts: RemoteTimeouts,
) -> (Value, String) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeouts.connect)
        .timeout(timeouts.request)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return (
                json!({}),
                format_request_error(operation, &error, timeouts.request),
            );
        }
    };
    let mut request = client.request(method, format!("{endpoint}{path}"));
    if let Some(body) = body {
        request = request.json(body);
    }
    match request.send().await {
        Ok(response) => parse_remote(response, operation, timeouts.request).await,
        Err(error) => (
            json!({}),
            format_request_error(operation, &error, timeouts.request),
        ),
    }
}

async fn parse_remote(
    response: Response,
    operation: &str,
    request_timeout: Duration,
) -> (Value, String) {
    let status = response.status();
    match response.json::<Value>().await {
        Ok(value) if status.is_success() => {
            (value.get("result").cloned().unwrap_or(value), String::new())
        }
        Ok(value) => (json!({}), value.to_string()),
        Err(error) => (
            json!({}),
            format_request_error(operation, &error, request_timeout),
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    Timeout,
    Dns,
    Tls,
    Proxy,
    Connect,
    Response,
    Configuration,
    Request,
}

impl FailureKind {
    fn label(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Dns => "dns",
            Self::Tls => "tls",
            Self::Proxy => "proxy",
            Self::Connect => "connect",
            Self::Response => "response",
            Self::Configuration => "configuration",
            Self::Request => "request",
        }
    }
}

fn format_request_error(
    operation: &str,
    error: &reqwest::Error,
    request_timeout: Duration,
) -> String {
    let detail = error_detail(error);
    let kind = classify_failure(
        error.is_timeout(),
        error.is_connect(),
        error.is_decode() || error.is_body(),
        error.is_builder(),
        &detail,
    );
    let category = if kind == FailureKind::Timeout {
        format!("timeout after {}", duration_label(request_timeout))
    } else {
        kind.label().to_owned()
    };
    let summary = format!("{operation} failed ({category})");
    if detail.is_empty() {
        summary
    } else {
        format!("{summary}: {detail}")
    }
}

fn classify_failure(
    is_timeout: bool,
    is_connect: bool,
    is_response: bool,
    is_builder: bool,
    detail: &str,
) -> FailureKind {
    if is_timeout {
        return FailureKind::Timeout;
    }
    let detail = detail.to_ascii_lowercase();
    if contains_any(&detail, &["proxy", "tunnel connection"]) {
        FailureKind::Proxy
    } else if contains_any(
        &detail,
        &[
            "dns",
            "failed to lookup address",
            "name or service not known",
            "nodename nor servname",
            "no such host",
        ],
    ) {
        FailureKind::Dns
    } else if contains_any(
        &detail,
        &["tls", "ssl", "certificate", "handshake", "peer certificate"],
    ) {
        FailureKind::Tls
    } else if is_connect {
        FailureKind::Connect
    } else if is_response {
        FailureKind::Response
    } else if is_builder {
        FailureKind::Configuration
    } else {
        FailureKind::Request
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn error_detail(error: &reqwest::Error) -> String {
    let mut details = Vec::new();
    let mut source = error.source();
    while let Some(current) = source {
        let message = current
            .to_string()
            .replace(['\n', '\r'], " ")
            .trim()
            .to_owned();
        if !message.is_empty() && details.last() != Some(&message) {
            details.push(message);
        }
        if details.len() == 6 {
            break;
        }
        source = current.source();
    }
    details.join(": ").chars().take(320).collect()
}

fn duration_label(duration: Duration) -> String {
    if duration.subsec_nanos() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

#[cfg(test)]
#[path = "group_bridge_pairing_http_tests.rs"]
mod tests;
