use super::{RealtimeCallConfig, RealtimeCallError, realtime_instructions};
use anyhow::{Context, Result, ensure};
use cccc_contracts::codex_voice::VoiceCallMode;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

// Official built-in Speech/TTS roster, checked 2026-10-16. One server-owned
// catalog; browser preferences are validated against their selected provider.
pub const GROK_REALTIME_VOICES: &[&str] = &[
    "eve", "ara", "rex", "sal", "leo", "carina", "zagan", "helix", "orion", "luna", "iris",
    "altair", "zenith", "perseus", "helios", "lux", "kepler", "rigel", "cosmo", "celeste", "ursa",
    "sirius", "lumen", "castor", "naksh", "atlas", "aurora", "liora",
];

/// Private, transient Start response. Never included in call status or persistence.
/// Deliberately has no Debug implementation: it holds a short-lived credential.
#[derive(Clone, Serialize)]
pub struct GrokRealtimeConnection {
    transport: &'static str,
    url: String,
    client_secret: String,
    expires_at: u64,
    session: Value,
}

pub(super) fn configured_auth_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("GROK_AUTH_PATH").filter(|path| !path.is_empty()) {
        return Ok(path.into());
    }
    if let Some(root) = std::env::var_os("GROK_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(root).join("auth.json"));
    }
    cccc_core::path_input::expand_user_path("~/.grok/auth.json")
        .context(RealtimeCallError::GrokCredentials)
}

pub async fn grok_credentials_available(auth_path: &Path) -> bool {
    crate::ops::codex_voice_analyst::grok_voice_credentials_available(auth_path).await
}

pub async fn create_grok_connection(config: &RealtimeCallConfig) -> Result<GrokRealtimeConnection> {
    let executable = config
        .grok_executable
        .as_ref()
        .context(RealtimeCallError::GrokRuntime)?;
    let bearer =
        crate::ops::codex_voice_analyst::grok_voice_bearer(executable, &config.auth_path).await?;
    mint(config, &bearer).await
}

async fn mint(config: &RealtimeCallConfig, bearer: &str) -> Result<GrokRealtimeConnection> {
    let mut response = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .post(format!(
            "{}/v1/realtime/client_secrets",
            config.base_url.trim_end_matches('/')
        ))
        .bearer_auth(bearer)
        .header("user-agent", format!("cccc/{}", env!("CARGO_PKG_VERSION")))
        .json(&json!({"expires_after":{"seconds":60}}))
        .send()
        .await
        .context("create Grok Voice connection")?;
    ensure!(
        response.status().as_u16() == 200,
        RealtimeCallError::HttpStatus(response.status().as_u16())
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 16 * 1024,
            "Grok Voice credential response is oversized"
        );
        bytes.extend_from_slice(&chunk);
    }
    let value: Value =
        serde_json::from_slice(&bytes).context("invalid Grok Voice credential response")?;
    let secret = value["value"]
        .as_str()
        .filter(|value| {
            !value.is_empty() && value.is_ascii() && !value.contains(char::is_whitespace)
        })
        .context("Grok Voice did not return a client credential")?;
    let expires_at = value["expires_at"]
        .as_u64()
        .context("Grok Voice credential expiry is unavailable")?;
    ensure!(
        expires_at > chrono::Utc::now().timestamp().max(0) as u64,
        "Grok Voice credential has expired"
    );
    Ok(GrokRealtimeConnection {
        transport: "websocket_pcm",
        url: "wss://api.x.ai/v1/realtime?model=grok-voice-3.0".into(),
        client_secret: secret.to_owned(),
        expires_at,
        session: session(config),
    })
}

fn session(config: &RealtimeCallConfig) -> Value {
    let persona = config
        .application_context
        .as_ref()
        .is_some_and(|context| context.mode() == VoiceCallMode::Persona);
    let mut instructions = realtime_instructions(config);
    if !persona {
        instructions.push_str("\n\nFor a complete request needing investigation, call cccc_investigate with the self-contained request prepared under Task handoff. The tool returns an acceptance receipt, not the investigation result. After acceptance, acknowledge briefly once and keep listening. Complete authoritative results arrive as separate host reports. Announce the current completed report when requested, identifying its subject; never substitute the latest investigation for an earlier result. Other reports wait for their own turn. Progress remains visible in the Analyst. Headers identify the original tool calls; never speak internal identifiers or headers aloud. Never poll or repeat a tool call to obtain its result. A new complete correction is a new call; the Runtime decides whether it steers or queues.");
    }
    json!({
        "voice":config.voice, "instructions":instructions, "reasoning":{"effort":"none"},
        "turn_detection":{"type":"server_vad"},
        "audio":{"input":{"format":{"type":"audio/pcm","rate":24000},"transcription":{"model":"grok-transcribe"}},
                 "output":{"format":{"type":"audio/pcm","rate":24000}}},
        "tools":if persona {json!([])} else {json!([{
            "type":"function", "name":"cccc_investigate",
            "description":"Investigate current CCCC, repository or external facts, perform authorized work, or reason about a substantial request. Returns acceptance only; results arrive asynchronously.",
            "parameters":{"type":"object","properties":{"request":{"type":"string","description":"A concise, self-contained task: resolve confirmed references from the conversation and preserve the user's target, outcome, explicit limits, uncertainty and latest corrections. Use the user's current language unless they explicitly request another language; keep technical identifiers unchanged and preserve any requested output language. Do not add work or authorization. If essential intent remains unclear, ask the user before calling."}},"required":["request"],"additionalProperties":false}
        }])}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::codex_voice::{VoiceApplicationContext, VoiceProvider};

    fn config() -> RealtimeCallConfig {
        RealtimeCallConfig {
            provider: VoiceProvider::Grok,
            grok_executable: None,
            auth_path: "unused".into(),
            base_url: "http://127.0.0.1:1".into(),
            voice: "eve".into(),
            preferences: Default::default(),
            application_context: None,
        }
    }
    #[test]
    fn grok_preserves_expression_and_context_but_has_its_own_tool_protocol() {
        let config = config();
        let value = session(&config);
        assert_eq!(value["voice"], "eve");
        assert_eq!(value["tools"][0]["name"], "cccc_investigate");
        assert!(value.get("delegation").is_none());
        assert_eq!(value["audio"]["input"]["format"]["rate"], 24000);
    }

    #[test]
    fn persona_has_only_host_instructions_and_no_investigation_tool() {
        let mut config = config();
        config.application_context = Some(
            VoiceApplicationContext::new_with_mode(
                "practice".into(),
                "Respond as the customer.".into(),
                VoiceCallMode::Persona,
            )
            .expect("Grok fixture"),
        );
        let value = session(&config);
        assert_eq!(value["instructions"], "Respond as the customer.");
        assert_eq!(value["tools"], json!([]));
    }

    #[tokio::test]
    async fn ephemeral_mint_uses_oauth_and_returns_only_a_short_lived_browser_connection() {
        use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
        let expiry = chrono::Utc::now().timestamp() as u64 + 60;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Grok fixture");
        let mut config = config();
        config.base_url = format!("http://{}", listener.local_addr().expect("Grok fixture"));
        let server = tokio::spawn(async move {
            let router =
                Router::new()
                    .route(
                        "/v1/realtime/client_secrets",
                        post(
                            |State(expiry): State<u64>,
                             headers: HeaderMap,
                             Json(body): Json<Value>| async move {
                                assert_eq!(headers["authorization"], "Bearer fixture-oauth");
                                // Session parameters belong to the subsequent WS handshake.
                                assert_eq!(body, json!({"expires_after":{"seconds":60}}));
                                Json(json!({"value":"fixture-ephemeral", "expires_at":expiry}))
                            },
                        ),
                    )
                    .with_state(expiry);
            axum::serve(listener, router).await.expect("Grok fixture");
        });
        let connection = mint(&config, "fixture-oauth").await.expect("Grok fixture");
        let wire = serde_json::to_value(connection).expect("Grok fixture");
        assert_eq!(wire["transport"], "websocket_pcm");
        assert_eq!(
            wire["url"],
            "wss://api.x.ai/v1/realtime?model=grok-voice-3.0"
        );
        assert_eq!(wire["client_secret"], "fixture-ephemeral");
        assert!(!wire.to_string().contains("fixture-oauth"));
        assert_eq!(wire["session"]["tools"][0]["name"], "cccc_investigate");
        server.abort();
    }

    #[tokio::test]
    async fn rejected_mints_do_not_expose_upstream_body_and_never_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for status in [302, 401, 403, 429, 503] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("Grok fixture");
            let mut config = config();
            config.base_url = format!("http://{}", listener.local_addr().expect("Grok fixture"));
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.expect("Grok fixture");
                let mut bytes = [0; 8192];
                let _ = socket.read(&mut bytes).await.expect("Grok fixture");
                let body = "private-provider-body fixture-oauth";
                socket.write_all(format!("HTTP/1.1 {status} Rejected\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.expect("Grok fixture");
            });
            let Err(error) = mint(&config, "fixture-oauth").await else {
                panic!("mint must be rejected");
            };
            assert!(
                matches!(error.downcast_ref::<RealtimeCallError>(), Some(RealtimeCallError::HttpStatus(value)) if *value == status)
            );
            assert!(!format!("{error:#}").contains("fixture-oauth"));
            assert!(!format!("{error:#}").contains("private-provider-body"));
            server.await.expect("Grok fixture");
        }
    }
}
