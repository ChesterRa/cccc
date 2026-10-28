//! Fixtures shared by the interface's unit tests.

use super::app::Context;
use crate::commands::sessions::setup::SetupInspection;
use crate::host_state::{DaemonState, HostPhase, HostState, WebState};
use cccc_contracts::local_session::{LocalSessionRuntime, LocalSessionSource};

pub(super) fn source(
    runtime: LocalSessionRuntime,
    cwd: &str,
    title: &str,
    connected: bool,
) -> LocalSessionSource {
    LocalSessionSource {
        source_id: runtime.source_id(&format!("{cwd}{title}")),
        runtime,
        native_session_id: format!("{cwd}{title}"),
        cwd: cwd.into(),
        title: title.into(),
        origin: None,
        connected,
    }
}

/// A found app that this CCCC has not set up yet, as `/setup` checks report it.
pub(super) fn found(
    runtime: LocalSessionRuntime,
    version: &str,
    provider_dir: &str,
) -> SetupInspection {
    SetupInspection {
        runtime: runtime.as_str().into(),
        executable: Some(format!("/usr/bin/{}", runtime.as_str()).into()),
        version: Some(version.into()),
        provider_dir: Some(provider_dir.into()),
        configured: Some(false),
        disabled: false,
        reconfigure_disconnects: matches!(
            runtime,
            LocalSessionRuntime::Pi | LocalSessionRuntime::Opencode
        ),
        issue: None,
        codex_auto_start: None,
    }
}

pub(super) fn missing(runtime: LocalSessionRuntime) -> SetupInspection {
    SetupInspection {
        executable: None,
        version: None,
        provider_dir: None,
        configured: None,
        issue: Some(format!(
            "{} was not found in this process's PATH",
            runtime.as_str()
        )),
        ..found(runtime, "", "")
    }
}

/// The four checks in display order: Codex and Claude Code found and not
/// yet set up, Pi set up by this CCCC, OpenCode missing.
pub(super) fn checks() -> Vec<SetupInspection> {
    vec![
        found(LocalSessionRuntime::Codex, "0.164.0", "/home/me/.codex"),
        found(LocalSessionRuntime::Claude, "2.1.311", "/home/me/.claude"),
        SetupInspection {
            configured: Some(true),
            ..found(LocalSessionRuntime::Pi, "1.4.0", "/home/me/.pi/agent")
        },
        missing(LocalSessionRuntime::Opencode),
    ]
}

pub(super) fn context() -> Context {
    Context {
        version: "0.4.45",
        home: "~/.cccc".into(),
        user_home: Some("/home/me".into()),
        color: true,
    }
}

pub(super) fn running() -> HostState {
    HostState {
        daemon: DaemonState::StartedHere { pid: 42 },
        web: WebState::Listening {
            url: "http://127.0.0.1:8848".into(),
        },
        phase: HostPhase::Running,
    }
}
