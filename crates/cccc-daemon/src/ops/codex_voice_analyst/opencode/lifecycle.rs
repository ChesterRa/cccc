use super::{AcpClient, STARTUP_TIMEOUT, process};
use base64::Engine;
use serde_json::Value;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

const SSE_BUFFER_LIMIT: usize = 512 * 1024;

pub(super) fn reserve_loopback_port() -> io::Result<u16> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
    listener.local_addr().map(|address| address.port())
}

pub(super) async fn wait_for_authenticated_backend(
    endpoint: &str,
    username: &str,
    password: &str,
    process: Arc<process::ChildOwner>,
) -> io::Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| io::Error::other(format!("build OpenCode health client: {error}")))?;
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    let health = format!("{endpoint}/global/health");
    while tokio::time::Instant::now() < deadline {
        if !process.running() {
            return Err(io::Error::other(
                "OpenCode ACP process exited during startup",
            ));
        }
        if let Ok(unauthenticated) = client.get(&health).send().await
            && unauthenticated.status() != reqwest::StatusCode::UNAUTHORIZED
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "OpenCode loopback backend did not enforce its CCCC-owned credentials",
            ));
        }
        match client
            .get(&health)
            .header(
                reqwest::header::AUTHORIZATION,
                basic_authorization(username, password),
            )
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                let value = response.json::<Value>().await.map_err(|error| {
                    io::Error::other(format!("invalid OpenCode health response: {error}"))
                })?;
                if value.get("healthy") == Some(&Value::Bool(true)) {
                    return Ok(());
                }
                return Err(io::Error::other(
                    "OpenCode health response did not identify a healthy backend",
                ));
            }
            Ok(_) | Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "OpenCode authenticated loopback backend did not become ready",
    ))
}

pub(super) async fn attach(
    protocol: &AcpClient,
    endpoint: &str,
    username: &str,
    password: &str,
    session_id: &str,
) -> io::Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|error| io::Error::other(format!("build OpenCode event client: {error}")))?;
    let response = client
        .get(format!("{endpoint}/global/event"))
        .header(
            reqwest::header::AUTHORIZATION,
            basic_authorization(username, password),
        )
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .send()
        .await
        .map_err(|error| io::Error::other(format!("connect OpenCode event stream: {error}")))?;
    if !response.status().is_success()
        || !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
    {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            "OpenCode lifecycle endpoint did not return an authenticated event stream",
        ));
    }
    let control = protocol.lifecycle_control();
    let session_id = session_id.to_owned();
    let task = tokio::spawn(async move {
        let mut response = response;
        let mut buffer = Vec::new();
        let mut current_user_message_id = None;
        let outcome: io::Result<()> =
            async {
                while let Some(chunk) = response.chunk().await.map_err(|error| {
                    io::Error::other(format!("read OpenCode lifecycle: {error}"))
                })? {
                    if buffer.len().saturating_add(chunk.len()) > SSE_BUFFER_LIMIT {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "OpenCode lifecycle event exceeded the bounded buffer",
                        ));
                    }
                    buffer.extend_from_slice(&chunk);
                    while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                        let mut line = buffer.drain(..=end).collect::<Vec<_>>();
                        while line
                            .last()
                            .is_some_and(|byte| matches!(byte, b'\n' | b'\r'))
                        {
                            line.pop();
                        }
                        let Ok(line) = std::str::from_utf8(&line) else {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "OpenCode lifecycle event was not UTF-8",
                            ));
                        };
                        let Some(data) = line.strip_prefix("data:") else {
                            continue;
                        };
                        let value: Value = serde_json::from_str(data.trim()).map_err(|error| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("invalid OpenCode lifecycle JSON: {error}"),
                            )
                        })?;
                        let payload = value.get("payload").unwrap_or(&value);
                        if let Some(text) =
                            observed_user_text(payload, &session_id, &mut current_user_message_id)
                        {
                            control.user_text(&session_id, &text).await?;
                        }
                        if payload.get("type").and_then(Value::as_str) == Some("session.status")
                            && payload
                                .pointer("/properties/sessionID")
                                .and_then(Value::as_str)
                                == Some(session_id.as_str())
                        {
                            match payload
                                .pointer("/properties/status/type")
                                .and_then(Value::as_str)
                            {
                                Some("busy") => control.status(&session_id, true).await?,
                                Some("idle") => {
                                    control.status(&session_id, false).await?;
                                    current_user_message_id = None;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "OpenCode lifecycle event stream ended",
                ))
            }
            .await;
        if let Err(error) = outcome {
            control.disconnected(error.to_string()).await;
        }
    });
    protocol.register_auxiliary_task(task);
    Ok(())
}

fn observed_user_text(
    payload: &Value,
    session_id: &str,
    current_user_message_id: &mut Option<String>,
) -> Option<String> {
    match payload.get("type").and_then(Value::as_str) {
        Some("message.updated") => {
            let info = payload.pointer("/properties/info")?;
            if info.get("role").and_then(Value::as_str) != Some("user")
                || info.get("sessionID").and_then(Value::as_str) != Some(session_id)
            {
                return None;
            }
            *current_user_message_id = info.get("id").and_then(Value::as_str).map(str::to_owned);
            None
        }
        Some("message.part.updated") => {
            let part = payload.pointer("/properties/part")?;
            if part.get("sessionID").and_then(Value::as_str) != Some(session_id)
                || part.get("type").and_then(Value::as_str) != Some("text")
                || part.get("messageID").and_then(Value::as_str)
                    != current_user_message_id.as_deref()
            {
                return None;
            }
            part.get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        }
        _ => None,
    }
}

fn basic_authorization(username: &str, password: &str) -> String {
    let credentials =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    format!("Basic {credentials}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_never_contains_plaintext_credentials() {
        let header = basic_authorization("opencode", "secret");
        assert!(header.starts_with("Basic "));
        assert!(!header.contains("secret"));
    }

    #[test]
    fn only_the_matching_backend_user_message_yields_prompt_text() {
        let mut user_message_id = None;
        assert_eq!(
            observed_user_text(
                &serde_json::json!({
                    "type":"message.updated",
                    "properties":{"info":{
                        "id":"message-user",
                        "sessionID":"session-owned",
                        "role":"user",
                    }}
                }),
                "session-owned",
                &mut user_message_id,
            ),
            None
        );
        assert_eq!(user_message_id.as_deref(), Some("message-user"));
        assert_eq!(
            observed_user_text(
                &serde_json::json!({
                    "type":"message.part.updated",
                    "properties":{"part":{
                        "sessionID":"session-owned",
                        "messageID":"message-user",
                        "type":"text",
                        "text":"owned prompt",
                    }}
                }),
                "session-owned",
                &mut user_message_id,
            )
            .as_deref(),
            Some("owned prompt")
        );
        assert_eq!(
            observed_user_text(
                &serde_json::json!({
                    "type":"message.part.updated",
                    "properties":{"part":{
                        "sessionID":"session-owned",
                        "messageID":"message-assistant",
                        "type":"text",
                        "text":"not a receipt",
                    }}
                }),
                "session-owned",
                &mut user_message_id,
            ),
            None
        );
    }
}
