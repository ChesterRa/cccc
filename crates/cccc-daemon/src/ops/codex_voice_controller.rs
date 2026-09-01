#[cfg(test)]
use super::codex_voice_analyst::AnalystEvent;
use super::codex_voice_analyst::{AnalystSession, LaunchConfig, TurnReceipt};
use super::codex_voice_lifecycle::{AnalystLifecycle, AnalystLifecycleEvent};
use cccc_core::HomeLayout;
use std::sync::Arc;
use tokio::sync::broadcast;

mod analyst;
mod call;
mod delegation;
mod lease;
mod projection;
mod provider;
#[cfg(test)]
mod tests;

pub use delegation::{ProviderDelegation, parse_provider_delegation};
use lease::CallLease;
use projection::CallState;
pub use projection::FinalProjection;
pub use provider::{
    DEFAULT_REALTIME_VOICE, REALTIME_VOICES, RealtimeCallConfig, create_realtime_answer,
    realtime_greeting_commands, realtime_notice_commands, validate_realtime_voice,
};

/// One repository-bound, resumable Codex analysis runtime behind the global Voice surface.
///
/// This runtime deliberately outlives individual microphone calls. The Web
/// owner keeps one instance warm and may wrap it in multiple short-lived
/// [`CodexVoiceCall`] values over time.
pub struct CodexVoiceAnalyst {
    session: Arc<AnalystSession>,
    lifecycle: Arc<AnalystLifecycle>,
}

/// Internal owner for one live Codex Voice audio call.
///
/// Media transport remains outside this type. It owns only the existing global
/// microphone lease, call-generation fencing, and at-most-once local context
/// projection. Stopping it never stops the shared warm Analyst.
pub struct CodexVoiceCall {
    generation: String,
    analyst: Arc<CodexVoiceAnalyst>,
    lease: CallLease,
    state: tokio::sync::Mutex<CallState>,
}
