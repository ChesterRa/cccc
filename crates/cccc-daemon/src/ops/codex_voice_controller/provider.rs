use super::projection::session_context_commands;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

const MAX_REALTIME_SDP_BYTES: usize = 256 * 1024;
pub(super) const REALTIME_INSTRUCTIONS: &str = r#"# Role
You are the concise, warm conversational surface of one CCCC assistant. Voice owns the live conversation. The connected Voice Analyst supplies repository inspection, current project and CCCC facts, tools, research, substantial reasoning, and durable coordination. Never mention a backend, intermediary, delegation, or separate assistant.

# Routing
- ANSWER DIRECTLY when the answer needs no unavailable fact, material verification, or action. This includes greetings, reactions, jokes, opinions, identity or role questions answerable from these instructions, clarifications, and self-contained discussion grounded in the live conversation.
- DO NOT delegate merely to produce a conversational reply, because the Voice Analyst could also answer, or for filler, a partial thought, or ambiguous low-content audio. Ask one short clarification when the user is clearly addressing you but the complete request is not yet clear.
- DELEGATE only a complete request that needs current CCCC, repository, build, test, or local-environment facts; web or another external source; a tool or operation; an action; or substantial reasoning that materially improves correctness.
- When Voice Analyst work is already active, immediately emit a new delegation for any complete correction, constraint, or follow-up that changes that work. Do not hold or discard it because the Analyst is busy; the connected Runtime decides whether the input steers the current turn or queues behind it.

# Results
Treat speakable Voice Analyst updates and results as authoritative. When one arrives, continue the live conversation immediately without waiting for another user message. Fold in only the new takeaway, status, or next step; do not read tool traces, tables, diffs, or long structured output aloud. Never claim work is complete before its result arrives.

# Speech
Speak in short natural sentences. Do not narrate routine routing or repeatedly promise to check. After routing work, wait for a substantive update. If the user interrupts, yield immediately and hear the complete correction."#;

#[derive(Debug, Clone)]
pub struct RealtimeCallConfig {
    pub auth_path: PathBuf,
    pub base_url: String,
    pub voice: String,
}

pub const DEFAULT_REALTIME_VOICE: &str = "cove";
pub const REALTIME_VOICES: &[&str] = &[
    "juniper", "maple", "spruce", "ember", "vale", "breeze", "arbor", "sol", "cove",
];

impl RealtimeCallConfig {
    pub fn from_environment() -> Result<Self> {
        Self::from_environment_with_voice(DEFAULT_REALTIME_VOICE)
    }

    pub fn from_environment_with_voice(voice: &str) -> Result<Self> {
        let voice = validate_realtime_voice(voice)?;
        let auth_path = configured_auth_path(
            std::env::var_os("CCCC_CODEX_AUTH_PATH").map(PathBuf::from),
            std::env::var_os("CODEX_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        )
        .map(Ok)
        .unwrap_or_else(|| {
            cccc_core::path_input::expand_user_path("~/.codex/auth.json")
                .context("resolve Codex authentication path")
        })?;
        Ok(Self {
            auth_path,
            base_url: std::env::var("CCCC_CODEX_VOICE_BASE_URL")
                .unwrap_or_else(|_| "https://chatgpt.com/backend-api/codex".into()),
            voice,
        })
    }
}

pub(super) fn configured_auth_path(
    explicit_auth_path: Option<PathBuf>,
    inherited_codex_home: Option<PathBuf>,
) -> Option<PathBuf> {
    explicit_auth_path.or_else(|| inherited_codex_home.map(|path| path.join("auth.json")))
}

pub fn validate_realtime_voice(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_lowercase();
    if REALTIME_VOICES.contains(&value.as_str()) {
        Ok(value)
    } else {
        bail!("unsupported Codex Realtime voice: {value}")
    }
}

/// Creates the provider side of a browser-owned WebRTC call without exposing
/// the Codex access token to the browser. The returned value is answer SDP.
pub async fn create_realtime_answer(config: &RealtimeCallConfig, offer: &str) -> Result<String> {
    let offer = validated_realtime_offer(offer)?;
    let auth: Value =
        serde_json::from_slice(&tokio::fs::read(&config.auth_path).await.with_context(|| {
            format!(
                "read Codex authentication from {}",
                config.auth_path.display()
            )
        })?)
        .context("parse Codex authentication")?;
    let token = auth["tokens"]["access_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .context("Codex ChatGPT access token is unavailable; run `codex login`")?;
    let account_id = auth["tokens"]["account_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .context("Codex ChatGPT account id is unavailable; run `codex login`")?;
    let endpoint = format!(
        "{}/realtime/calls?intent=quicksilver&architecture=avas",
        config.base_url.trim_end_matches('/')
    );
    let mut response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?
        .post(endpoint)
        .bearer_auth(token)
        .header("chatgpt-account-id", account_id)
        .header("originator", "cccc")
        .header("x-session-id", uuid::Uuid::new_v4().to_string())
        .header("user-agent", format!("cccc/{}", env!("CARGO_PKG_VERSION")))
        .header("openai-alpha", "quicksilver=v2")
        .json(&json!({
            "sdp":offer,
            "session":{
                "model":"gpt-live-1-codex",
                "instructions":REALTIME_INSTRUCTIONS,
                "audio":{"output":{"voice":config.voice}},
                "delegation":{"type":"client","ack_filler":true}
            }
        }))
        .send()
        .await
        .context("create Codex Voice call")?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REALTIME_SDP_BYTES as u64)
    {
        bail!("Codex Voice answer SDP is oversized");
    }
    let mut body = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(MAX_REALTIME_SDP_BYTES as u64) as usize,
    );
    while let Some(chunk) = response.chunk().await.context("read Codex Voice answer")? {
        if chunk.len() > MAX_REALTIME_SDP_BYTES.saturating_sub(body.len()) {
            bail!("Codex Voice answer SDP is oversized");
        }
        body.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(body).context("Codex Voice answer is not UTF-8")?;
    if status.as_u16() != 201 {
        bail!(
            "Codex Voice call failed with {status}: {}",
            body.chars().take(500).collect::<String>()
        );
    }
    Ok(body)
}

pub(super) fn validated_realtime_offer(offer: &str) -> Result<&str> {
    if offer.trim().is_empty() {
        bail!("Codex Voice WebRTC offer is empty");
    }
    if offer.len() > MAX_REALTIME_SDP_BYTES {
        bail!("Codex Voice WebRTC offer exceeds {MAX_REALTIME_SDP_BYTES} bytes");
    }
    // Preserve SDP byte-for-byte, including the trailing CRLF expected by some parsers.
    Ok(offer)
}

pub fn realtime_greeting_commands() -> Vec<Value> {
    session_context_commands(
        "The global voice session has started. Give the user one short, natural greeting, then wait for them to speak. Do not imply that a Working Group is already selected.",
    )
}

pub fn realtime_notice_commands(message: &str) -> Vec<Value> {
    session_context_commands(message.trim())
}
