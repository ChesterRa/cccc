use rmcp::{
    RoleClient, ServiceError,
    model::{CallToolRequestParams, CallToolResponse},
    service::{ClientInitializeError, RunningService},
};
use serde_json::{Map, Value};

#[derive(Debug)]
pub(super) struct Failure {
    pub message: String,
    pub remote: bool,
    pub status: Option<u16>,
}

impl Failure {
    pub(super) fn transport(stage: &str, error: impl std::fmt::Display) -> Self {
        Self {
            message: format!("MCP {stage} failed: {error}"),
            remote: false,
            status: None,
        }
    }

    pub(super) fn initialize(error: ClientInitializeError) -> Self {
        let status = match &error {
            ClientInitializeError::TransportError { error, .. } => {
                http_status(error.error.as_ref())
            }
            _ => None,
        };
        let reason = match &error {
            ClientInitializeError::TransportError { error, .. } => {
                error_reason(error.error.as_ref())
            }
            ClientInitializeError::JsonRpcError(error) => error.message.to_string(),
            ClientInitializeError::ExpectedInitResponse(_)
            | ClientInitializeError::ExpectedInitResult(_) => {
                "server closed the connection or returned an invalid initialization response".into()
            }
            _ => error_reason(&error),
        };
        let mut failure = Self::transport("initialize", reason);
        failure.status = status;
        failure
    }

    fn rpc(stage: &str, error: ServiceError) -> Self {
        let remote = matches!(&error, ServiceError::McpError(_));
        let status = match &error {
            ServiceError::TransportSend(error) => http_status(error.error.as_ref()),
            _ => None,
        };
        let reason = match &error {
            ServiceError::TransportSend(error) => error_reason(error.error.as_ref()),
            ServiceError::McpError(error) => error.message.to_string(),
            _ => error_reason(&error),
        };
        Self {
            message: format!("MCP {stage} failed: {reason}"),
            remote,
            status,
        }
    }
}

fn http_status(mut error: &(dyn std::error::Error + 'static)) -> Option<u16> {
    use rmcp::transport::streamable_http_client::StreamableHttpError;
    loop {
        if let Some(http) = error.downcast_ref::<StreamableHttpError<reqwest_mcp::Error>>() {
            match http {
                StreamableHttpError::AuthRequired(_) => return Some(401),
                StreamableHttpError::InsufficientScope(_) => return Some(403),
                StreamableHttpError::Client(error) => {
                    return error.status().map(|status| status.as_u16());
                }
                StreamableHttpError::HttpStatus(status) => return Some(status.status),
                // rmcp 3.9's adapter loses the status on POST errors without a
                // challenge. Interpret only its pinned, typed transport prefix.
                StreamableHttpError::UnexpectedServerResponse(message)
                    if message.starts_with("HTTP 401 Unauthorized:") =>
                {
                    return Some(401);
                }
                StreamableHttpError::UnexpectedServerResponse(message)
                    if message.starts_with("HTTP 403 Forbidden:") =>
                {
                    return Some(403);
                }
                _ => (),
            }
        }
        error = error.source()?;
    }
}

fn error_reason(error: &(dyn std::error::Error + 'static)) -> String {
    let mut current = error;
    while let Some(source) = current.source() {
        current = source;
    }
    current.to_string()
}

pub(super) enum Request {
    List,
    Call {
        name: String,
        arguments: Map<String, Value>,
    },
}

pub(super) async fn request(
    service: &RunningService<RoleClient, ()>,
    request: Request,
    deadline: tokio::time::Instant,
) -> Result<Value, Failure> {
    match request {
        Request::List => match tokio::time::timeout_at(deadline, service.list_all_tools()).await {
            Ok(Ok(tools)) => serde_json::to_value(tools)
                .map_err(|error| Failure::transport("tools/list result", error)),
            Ok(Err(error)) => Err(Failure::rpc("tools/list", error)),
            Err(error) => Err(Failure::transport("tools/list timeout", error)),
        },
        Request::Call { name, arguments } => match tokio::time::timeout_at(
            deadline,
            service.call_tool_once(CallToolRequestParams::new(name).with_arguments(arguments)),
        )
        .await
        {
            Ok(Ok(CallToolResponse::Complete(result))) => serde_json::to_value(result)
                .map_err(|error| Failure::transport("tools/call result", error)),
            Ok(Ok(_)) => Err(Failure::transport(
                "tools/call",
                "interactive input and asynchronous task results are not supported; the tool was not called again",
            )),
            Ok(Err(error)) => Err(Failure::rpc("tools/call", error)),
            Err(error) => Err(Failure::transport("tools/call timeout", error)),
        },
    }
}

/// Replace every secret in one pass, earliest and then longest match first, so a short
/// value inside a longer secret cannot split it and leave pieces of the longer one.
pub(super) fn redact_diagnostic(message: &str, secrets: &[String]) -> String {
    let mut output = String::with_capacity(message.len());
    let mut rest = message;
    while !rest.is_empty() {
        let found = secrets
            .iter()
            .filter(|secret| !secret.is_empty())
            .filter_map(|secret| rest.find(secret).map(|at| (at, secret.len())))
            .min_by_key(|(at, len)| (*at, std::cmp::Reverse(*len)));
        let Some((at, len)) = found else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..at]);
        output.push_str("<redacted>");
        rest = &rest[at + len..];
    }
    output
}
