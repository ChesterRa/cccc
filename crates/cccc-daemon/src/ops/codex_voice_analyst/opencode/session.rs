use super::AcpClient;
use serde_json::{Value, json};
use std::io;
use std::path::Path;
use std::time::Duration;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

#[allow(clippy::too_many_arguments)]
pub(super) async fn initialize(
    protocol: &AcpClient,
    cwd: &Path,
    resume_session_id: Option<&str>,
    mcp_server: Value,
    allow_fresh_after_resume_failure: bool,
    model: Option<&str>,
    agent: Option<&str>,
) -> io::Result<(String, bool)> {
    let cwd = cwd.to_string_lossy().into_owned();
    let session_params = |session_id: Option<&str>| {
        let mut params = json!({"cwd":cwd,"mcpServers":[mcp_server.clone()]});
        if let Some(session_id) = session_id {
            params["sessionId"] = json!(session_id);
        }
        params
    };
    let (session_id, resumed) = if let Some(session_id) =
        resume_session_id.map(str::trim).filter(|id| !id.is_empty())
    {
        match protocol
            .request(
                "session/load",
                session_params(Some(session_id)),
                HANDSHAKE_TIMEOUT,
            )
            .await
        {
            Ok(_) => (session_id.to_owned(), true),
            Err(error) if allow_fresh_after_resume_failure => {
                tracing::warn!(%error, session_id, "OpenCode managed session resume failed; starting fresh");
                (new_session(protocol, session_params(None)).await?, false)
            }
            Err(error) => return Err(error),
        }
    } else {
        (new_session(protocol, session_params(None)).await?, false)
    };
    if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
        set_option(protocol, &session_id, "model", model).await?;
    }
    if let Some(agent) = agent.map(str::trim).filter(|value| !value.is_empty()) {
        set_option(protocol, &session_id, "mode", agent).await?;
    }
    Ok((session_id, resumed))
}

async fn new_session(protocol: &AcpClient, params: Value) -> io::Result<String> {
    let result = protocol
        .request("session/new", params, HANDSHAKE_TIMEOUT)
        .await?;
    result
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("OpenCode ACP returned an empty session id"))
}

async fn set_option(
    protocol: &AcpClient,
    session_id: &str,
    config_id: &str,
    value: &str,
) -> io::Result<()> {
    protocol
        .request(
            "session/set_config_option",
            json!({"sessionId":session_id,"configId":config_id,"value":value}),
            HANDSHAKE_TIMEOUT,
        )
        .await
        .map(|_| ())
}
