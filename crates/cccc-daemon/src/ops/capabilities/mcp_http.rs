use rmcp::ServiceExt;
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};

pub(super) use super::mcp_client::Failure;
use super::mcp_client::{self, Request, redact_diagnostic};
use serde_json::{Map, Value};
use std::time::Duration;

use crate::ops::local_headless::block_on_managed;

#[cfg(test)]
mod tests;

pub(super) fn list_tools(url: &str, timeout: Duration) -> Result<Vec<Value>, Failure> {
    list_tools_with_token(url, None, timeout)
}

pub(super) fn list_tools_with_token(
    url: &str,
    token: Option<&str>,
    timeout: Duration,
) -> Result<Vec<Value>, Failure> {
    run(url, token, Request::List, timeout).and_then(|value| {
        serde_json::from_value(value)
            .map_err(|error| Failure::transport("tools/list result", error))
    })
}

pub(super) fn call_tool(
    url: &str,
    token: Option<&str>,
    name: &str,
    arguments: Map<String, Value>,
    timeout: Duration,
) -> Result<Value, Failure> {
    run(
        url,
        token,
        Request::Call {
            name: name.into(),
            arguments,
        },
        timeout,
    )
}

fn run(
    url: &str,
    token: Option<&str>,
    request: Request,
    timeout: Duration,
) -> Result<Value, Failure> {
    block_on_managed(exchange(url, token, request, timeout)).map_err(|mut error| {
        error.message = redact_transport_error(&error.message, url, token);
        error
    })
}

/// Transport errors can contain the endpoint, its URL credentials and the token.
fn redact_transport_error(message: &str, url: &str, token: Option<&str>) -> String {
    let message = message.replace(url, "<MCP endpoint>");
    let mut secrets = Vec::new();
    if let Ok(parsed) = url::Url::parse(url) {
        secrets.push(parsed.username().to_owned());
        secrets.extend(parsed.password().map(str::to_owned));
        secrets.extend(parsed.query_pairs().map(|(_, value)| value.into_owned()));
    }
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        secrets.push(token.to_owned());
        if let Ok(escaped) = serde_json::to_string(token) {
            secrets.push(escaped[1..escaped.len() - 1].to_owned());
        }
    }
    redact_diagnostic(&message, &secrets)
}

async fn exchange(
    url: &str,
    token: Option<&str>,
    request: Request,
    timeout: Duration,
) -> Result<Value, Failure> {
    // Preserve the existing HTTP trust store rather than adopting reqwest 0.13's
    // platform-root default as a side effect of changing the MCP transport.
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::aws_lc_rs::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .map_err(|error| Failure::transport("TLS configuration", error))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut builder = reqwest_mcp::Client::builder()
        .timeout(timeout)
        .redirect(reqwest_mcp::redirect::Policy::none())
        .tls_backend_preconfigured(tls);
    if url::Url::parse(url).is_ok_and(|url| cccc_core::local_network::is_loopback_url(&url)) {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|error| Failure::transport("client", error.without_url()))?;
    // A tool may have side effects. Report an expired session to the caller;
    // never replay its POST by silently starting another session.
    let mut config = StreamableHttpClientTransportConfig::with_uri(url.to_owned())
        .reinit_on_expired_session(false)
        .max_concurrent_requests(1);
    if let Some(token) = token {
        config = config.auth_header(token.to_owned());
    }
    let deadline = tokio::time::Instant::now() + timeout;
    let transport = StreamableHttpClientTransport::with_client(client, config);
    let service = tokio::time::timeout_at(deadline, ().serve(transport))
        .await
        .map_err(|error| Failure::transport("initialize timeout", error))?
        .map_err(Failure::initialize)?;
    let result = mcp_client::request(&service, request, deadline).await;
    // Cleanup must not erase a known tool result and invite a duplicate call.
    if !matches!(
        tokio::time::timeout(Duration::from_secs(6), service.cancel()).await,
        Ok(Ok(_))
    ) {
        tracing::warn!("MCP HTTP client cleanup did not complete");
    }
    result
}
