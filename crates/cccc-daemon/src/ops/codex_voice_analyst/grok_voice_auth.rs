//! A native authentication consumer, not an Analyst session. No session/new or prompt.
use super::{
    acp::{AcpClient, PermissionPolicy, PromptCompletion},
    process::ChildOwner,
};
use crate::ops::codex_voice_controller::RealtimeCallError;
use anyhow::{Context, Result, ensure};
use cccc_runtime::OwnedProcessTree;
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

const AUTH_SCOPE: &str = "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828";
const MAX_AUTH_BYTES: u64 = 1024 * 1024;

async fn login(path: &Path) -> Result<Value> {
    let metadata = tokio::fs::metadata(path)
        .await
        .context(RealtimeCallError::GrokCredentials)?;
    ensure!(
        metadata.is_file() && metadata.len() <= MAX_AUTH_BYTES,
        RealtimeCallError::GrokCredentials
    );
    let bytes = tokio::fs::read(path)
        .await
        .context(RealtimeCallError::GrokCredentials)?;
    let store: Value =
        serde_json::from_slice(&bytes).context(RealtimeCallError::GrokCredentials)?;
    let entry = store
        .get(AUTH_SCOPE)
        .filter(|entry| valid_login(entry))
        .context(RealtimeCallError::GrokCredentials)?;
    Ok(entry.clone())
}

fn valid_login(entry: &Value) -> bool {
    entry["auth_mode"] == "oidc"
        && entry["key"].as_str().is_some_and(|key| !key.is_empty())
        && entry["oidc_issuer"]
            .as_str()
            .is_none_or(|issuer| issuer == "https://auth.x.ai")
        && entry["oidc_client_id"]
            .as_str()
            .is_none_or(|id| id == "b1a00492-073a-47ea-816f-4c329264a828")
}

/// Metadata only. A stored login can be refreshed by the native owner on Start.
pub(crate) async fn grok_voice_credentials_available(path: &Path) -> bool {
    login(path).await.is_ok()
}

pub(crate) async fn grok_voice_bearer(executable: &Path, auth_path: &Path) -> Result<String> {
    let before = login(auth_path).await?;
    let temp = tempfile::Builder::new()
        .prefix("cccc-grok-auth-")
        .tempdir()
        .context(RealtimeCallError::GrokAuthentication)?;
    let root = temp.path();
    tokio::fs::write(root.join("config.toml"), "[cli]\nuse_leader = false\n").await?;
    let auth_path = tokio::fs::canonicalize(auth_path)
        .await
        .context(RealtimeCallError::GrokCredentials)?;
    let mut command = Command::new(executable);
    command
        .args(["agent", "--no-leader", "stdio"])
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("GROK_HOME", root)
        .env("GROK_AUTH_PATH", &auth_path)
        .env("CCCC_HOME", root.join("cccc"))
        .env("CODEX_HOME", root.join("codex"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("GROK_DISABLE_API_KEY_AUTH", "1")
        .env("GROK_NO_AUTO_UPDATE", "1")
        .env("GROK_AGENT_DASHBOARD", "0")
        .env("GROK_TELEMETRY_ENABLED", "false")
        .env("GROK_OTEL_ENABLED", "false")
        .env("RUST_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Native TLS/proxy/OS routing is retained; API keys, external auth commands,
    // provider overrides and the user's project/config environment are not inherited.
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "REQUESTS_CA_BUNDLE",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let (mut child, tree) =
        OwnedProcessTree::spawn(&mut command).context(RealtimeCallError::GrokRuntime)?;
    let stdin = child
        .stdin
        .take()
        .context(RealtimeCallError::GrokAuthentication)?;
    let stdout = child
        .stdout
        .take()
        .context(RealtimeCallError::GrokAuthentication)?;
    let owner = ChildOwner::new(child, tree);
    let private_values = ["key", "refresh_token"]
        .into_iter()
        .filter_map(|key| before[key].as_str().map(str::to_owned))
        .collect();
    let protocol = AcpClient::new_with_private_values(
        stdin,
        stdout,
        uuid::Uuid::new_v4().to_string(),
        "grok-voice-auth",
        PermissionPolicy::Reject,
        PromptCompletion::Response,
        private_values,
    )
    .context(RealtimeCallError::GrokAuthentication)?;
    let result = async {
        protocol
            .request(
                "initialize",
                json!({"protocolVersion":1,"clientCapabilities":{},
            "clientInfo":{"name":"cccc-voice","version":env!("CARGO_PKG_VERSION")}}),
                Duration::from_secs(15),
            )
            .await?;
        let wire = protocol
            .request(
                "_x.ai/auth/getBearerToken",
                json!({}),
                Duration::from_secs(15),
            )
            .await?;
        let token = wire
            .get("result")
            .unwrap_or(&wire)
            .get("token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .context(RealtimeCallError::GrokCredentials)?
            .to_owned();
        let after = login(&auth_path).await?;
        ensure!(
            after["key"].as_str() == Some(&token),
            RealtimeCallError::GrokCredentials
        );
        if let Some(user) = before["user_id"].as_str().filter(|value| !value.is_empty()) {
            ensure!(
                after["user_id"].as_str() == Some(user),
                RealtimeCallError::GrokCredentials
            );
        }
        Ok::<_, anyhow::Error>(token)
    };
    // close/Drop terminate the protocol and its owned process tree on every path,
    // including when the caller cancels the future. The provider owns auth.json's lock.
    let result = tokio::time::timeout(Duration::from_secs(25), result).await;
    protocol.close().await;
    owner
        .stop()
        .context(RealtimeCallError::GrokAuthentication)?;
    result
        .context(RealtimeCallError::GrokAuthentication)?
        .map_err(|_| RealtimeCallError::GrokAuthentication.into())
}

#[cfg(test)]
mod tests;
