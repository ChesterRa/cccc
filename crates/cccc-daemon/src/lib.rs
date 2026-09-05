pub mod deepseek_setup;
mod dispatch;
mod dispatch_concurrency;
mod group_bridge_sessions;
mod ops;
mod paths;
mod process;
mod process_tree;
mod runtime_start_gate;
mod server;
mod server_actor_activity;
mod server_automation;
mod server_connection;
mod server_connections;
mod server_events_stream;
mod server_lifecycle;
mod server_terminal_attach;

pub use dispatch::dispatch as handle_request;
pub use paths::DaemonPaths;
pub use process::{DetachedDaemon, StartOutcome};
pub use server::run;
pub use server_lifecycle::stop_every_runtime;

/// Return the recorded Web binding only when its process and signed readiness
/// endpoint still match the persisted runtime identity.
pub fn live_web_binding(home: &cccc_core::HomeLayout) -> Option<(String, u16)> {
    crate::ops::validated_live_web_binding(home)
        .ok()
        .map(|binding| (binding.host, binding.port))
}

/// Opt-in, compatibility-pinned Codex Realtime Voice building blocks.
///
/// This API is intentionally experimental. It owns no ledger schema or roster
/// identity and may change when the upstream private Voice protocol changes.
pub mod experimental_codex_voice {
    pub use crate::ops::codex_voice_analyst::{LaunchConfig, TurnReceipt};
    pub use crate::ops::codex_voice_controller::{
        CodexVoiceAnalyst, CodexVoiceCall, DEFAULT_REALTIME_VOICE, FinalProjection,
        ProviderDelegation, REALTIME_VOICES, RealtimeCallConfig, create_realtime_answer,
        parse_provider_delegation, realtime_greeting_commands, realtime_notice_commands,
        validate_realtime_voice,
    };
    pub use crate::ops::codex_voice_lifecycle::{
        AnalystLifecycleEvent, AnalystTurnOrigin, TrackedWork, VoiceDelegationAdmission,
    };
}
