use super::super::launch::ANALYST_INSTRUCTIONS;
use super::{AcpClient, SessionPurpose};
use serde_json::{Value, json};
use std::io;
use std::path::Path;
use std::time::Duration;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) async fn initialize(
    protocol: &AcpClient,
    cwd: &Path,
    purpose: SessionPurpose,
    configured_rules: &str,
    resume_session_id: Option<&str>,
    mcp_server: Value,
    allow_fresh_after_resume_failure: bool,
) -> io::Result<(String, bool)> {
    let rules = match purpose {
        SessionPurpose::VoiceAnalyst if configured_rules.trim().is_empty() => {
            ANALYST_INSTRUCTIONS.to_owned()
        }
        SessionPurpose::VoiceAnalyst => {
            format!("{ANALYST_INSTRUCTIONS}\n\n{configured_rules}")
        }
        SessionPurpose::Actor => configured_rules.to_owned(),
    };
    let cwd = cwd.to_string_lossy().into_owned();
    let session_params = |method: &str, session_id: Option<&str>| {
        let mut params = json!({
            "cwd":cwd,
            "mcpServers":[mcp_server.clone()],
            "_meta":{"yoloMode":true},
        });
        if !rules.trim().is_empty() {
            params["_meta"]["rules"] = json!(rules);
        }
        if method == "session/load"
            && let Some(session_id) = session_id
        {
            params["sessionId"] = json!(session_id);
        }
        params
    };
    if let Some(session_id) = resume_session_id.map(str::trim).filter(|id| !id.is_empty()) {
        match protocol
            .request(
                "session/load",
                session_params("session/load", Some(session_id)),
                HANDSHAKE_TIMEOUT,
            )
            .await
        {
            Ok(_) => return Ok((session_id.to_owned(), true)),
            Err(error) if allow_fresh_after_resume_failure => {
                tracing::warn!(%error, session_id, "Grok managed session resume failed; starting fresh");
            }
            Err(error) => return Err(error),
        }
    }
    let result = protocol
        .request(
            "session/new",
            session_params("session/new", None),
            HANDSHAKE_TIMEOUT,
        )
        .await?;
    let session_id = result
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("Grok ACP returned an empty session id"))?;
    Ok((session_id.to_owned(), false))
}
