use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::utc_now;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActorRole {
    Foreman,
    Peer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActorSubmit {
    #[default]
    Enter,
    Newline,
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RunnerKind {
    #[default]
    Pty,
    Headless,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStateSource {
    #[default]
    Terminal,
    #[serde(rename = "managed_session", alias = "app_server")]
    ManagedSession,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActorRuntime {
    Amp,
    Antigravity,
    Auggie,
    Claude,
    Cline,
    #[default]
    Codex,
    Deepseek,
    Copilot,
    Cursor,
    Devin,
    Kiro,
    Kilo,
    Droid,
    Grok,
    Hermes,
    Kimi,
    Opencode,
    WebModel,
    Custom,
}

impl ActorRuntime {
    /// The runner is an implementation detail derived from the Runtime, not a
    /// user-selectable execution mode. CLI runtimes always expose their native
    /// terminal; runtimes with their own non-terminal surface keep the
    /// structured runner internally.
    #[must_use]
    pub const fn runner(self) -> RunnerKind {
        match self {
            Self::Deepseek | Self::WebModel => RunnerKind::Headless,
            _ => RunnerKind::Pty,
        }
    }

    /// Runtime state authority is derived from the Runtime adapter. It is not
    /// a user-selectable execution mode.
    #[must_use]
    pub const fn state_source(self) -> RuntimeStateSource {
        match self {
            Self::Claude | Self::Codex | Self::Grok | Self::Opencode | Self::Kilo => {
                RuntimeStateSource::ManagedSession
            }
            _ => RuntimeStateSource::Terminal,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GroupState {
    #[default]
    Active,
    Idle,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Actor {
    #[serde(default = "version")]
    pub v: u8,
    pub id: String,
    #[serde(default)]
    pub role: Option<ActorRole>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub default_scope_key: String,
    #[serde(default)]
    pub submit: ActorSubmit,
    #[serde(default)]
    pub capability_autoload: Vec<String>,
    #[serde(default)]
    pub capability_hidden: Vec<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub runner: RunnerKind,
    #[serde(default)]
    pub runtime: ActorRuntime,
    #[serde(default)]
    pub runtime_state_source: RuntimeStateSource,
    #[serde(default)]
    pub internal_kind: Option<String>,
    #[serde(default)]
    pub avatar_asset_path: String,
    #[serde(default)]
    pub profile_id: String,
    #[serde(default = "global_scope")]
    pub profile_scope: String,
    #[serde(default)]
    pub profile_owner: String,
    #[serde(default)]
    pub profile_revision_applied: u64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl Actor {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let now = utc_now();
        Self {
            v: 1,
            id: id.into(),
            role: None,
            title: String::new(),
            command: Vec::new(),
            env: BTreeMap::new(),
            default_scope_key: String::new(),
            submit: ActorSubmit::default(),
            capability_autoload: Vec::new(),
            capability_hidden: Vec::new(),
            enabled: true,
            runner: RunnerKind::default(),
            runtime: ActorRuntime::default(),
            runtime_state_source: RuntimeStateSource::default(),
            internal_kind: None,
            avatar_asset_path: String::new(),
            profile_id: String::new(),
            profile_scope: global_scope(),
            profile_owner: String::new(),
            profile_revision_applied: 0,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    pub fn normalize_runtime_constraints(&mut self) {
        self.runner = self.runtime.runner();
        self.runtime_state_source = self.runtime.state_source();
        if self.runtime == ActorRuntime::WebModel {
            self.command.clear();
        }
    }
}

const fn version() -> u8 {
    1
}
const fn enabled() -> bool {
    true
}
fn global_scope() -> String {
    "global".into()
}

#[cfg(test)]
mod tests {
    use super::{Actor, ActorRuntime, RunnerKind, RuntimeStateSource};

    #[test]
    fn cline_runtime_round_trips_through_the_shared_contract() {
        let runtime: ActorRuntime = serde_json::from_str(r#""cline""#).expect("deserialize");
        assert_eq!(runtime, ActorRuntime::Cline);
        assert_eq!(
            serde_json::to_string(&runtime).expect("serialize"),
            r#""cline""#
        );
    }

    #[test]
    fn deepseek_runtime_round_trips_through_the_shared_contract() {
        let runtime: ActorRuntime = serde_json::from_str(r#""deepseek""#).expect("deserialize");
        assert_eq!(runtime, ActorRuntime::Deepseek);
        assert_eq!(
            serde_json::to_string(&runtime).expect("serialize"),
            r#""deepseek""#
        );
    }

    #[test]
    fn execution_surface_is_derived_from_runtime_at_the_contract_boundary() {
        let mut actor = Actor::new("deepseek");
        actor.runtime = ActorRuntime::Deepseek;
        actor.runner = RunnerKind::Pty;
        actor.runtime_state_source = RuntimeStateSource::ManagedSession;
        actor.normalize_runtime_constraints();
        assert_eq!(actor.runner, RunnerKind::Headless);
        assert_eq!(actor.runtime_state_source, RuntimeStateSource::Terminal);

        actor.runtime = ActorRuntime::Codex;
        actor.runner = RunnerKind::Headless;
        actor.runtime_state_source = RuntimeStateSource::Terminal;
        actor.normalize_runtime_constraints();
        assert_eq!(actor.runner, RunnerKind::Pty);
        assert_eq!(
            actor.runtime_state_source,
            RuntimeStateSource::ManagedSession
        );

        actor.runtime = ActorRuntime::Custom;
        actor.runner = RunnerKind::Headless;
        actor.runtime_state_source = RuntimeStateSource::ManagedSession;
        actor.normalize_runtime_constraints();
        assert_eq!(actor.runner, RunnerKind::Pty);
        assert_eq!(actor.runtime_state_source, RuntimeStateSource::Terminal);
    }

    #[test]
    fn legacy_app_server_state_upgrades_to_managed_session() {
        let state: RuntimeStateSource =
            serde_json::from_str(r#""app_server""#).expect("legacy state");
        assert_eq!(state, RuntimeStateSource::ManagedSession);
        assert_eq!(
            serde_json::to_string(&state).expect("managed state"),
            r#""managed_session""#
        );
    }
}
