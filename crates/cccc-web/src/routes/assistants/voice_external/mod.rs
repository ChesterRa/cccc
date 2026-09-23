//! Cloud ASR adapters use the existing browser PCM/lease/transcript contracts.
mod active;
mod bailian;
mod checkpoint_schedule;
mod config;
mod connection;
mod connection_error;
mod disconnect;
mod lease;
mod persistence;
mod routes;
mod session;
mod transcript;
mod volcengine;

use super::voice_asr::VoiceError as AsrError;
use cccc_core::HomeLayout;
pub(super) use routes::routes;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Provider {
    Bailian,
    Volcengine,
}
impl Provider {
    fn parse(value: &str) -> Result<Self, AsrError> {
        match value {
            "bailian" => Ok(Self::Bailian),
            "volcengine" => Ok(Self::Volcengine),
            _ => Err(AsrError::new(
                "external_asr_unknown_provider",
                "Select Bailian or Volcengine ASR",
            )),
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Bailian => "bailian",
            Self::Volcengine => "volcengine",
        }
    }
}

pub(super) fn selected(assistant: &Value) -> Result<Provider, AsrError> {
    Provider::parse(
        assistant["config"]["external_asr_provider"]
            .as_str()
            .unwrap_or("bailian"),
    )
}

pub(super) fn health(home: &HomeLayout, assistant: &Value) -> Value {
    let result = selected(assistant)
        .and_then(|provider| config::load(home, provider).map(|cfg| (provider, cfg)));
    match result {
        Ok((provider, cfg)) => json!({"ready":cfg.configured(provider),"alive":true,
            "status":if cfg.configured(provider){"configured"}else{"not_configured"},
            "provider":provider.id(),"model_id":cfg.model_id(provider),"backend":"external_provider_asr"}),
        Err(error) => {
            json!({"ready":false,"alive":false,"status":"unavailable","error":{"code":error.code,"message":error.message}})
        }
    }
}

pub(super) use session::serve;

#[cfg(test)]
mod tests;

/// Provider error codes are bounded identifiers (`InvalidParameter`, `45000000`),
/// safe to surface. Anything else in a provider failure (free-text messages,
/// request ids) stays out of user-facing errors and logs.
pub(super) fn bounded_provider_code(raw: &str) -> String {
    let code = raw.trim();
    if code.is_empty()
        || code.len() > 64
        || !code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        "unknown".into()
    } else {
        code.to_owned()
    }
}
