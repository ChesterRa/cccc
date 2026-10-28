//! Same-user native sessions, using ordinary Groups, Actors and delivery receipts.
use super::operation::{
    Operation,
    Policy::{GlobalWrite, Read, ResourceOwned, Write},
};
use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use cccc_contracts::local_session::{
    LocalSessionRecovery, LocalSessionRecoveryState, LocalSessionRuntime, LocalSessionShell,
};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, GroupState};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

mod codex;
mod conversations;
mod resume;
mod store;
mod transport;

#[cfg(all(test, target_os = "linux"))]
mod tests;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "local_session_list" => Operation::new(Read, list),
        "local_session_recovery" => Operation::new(Read, recovery),
        "local_session_refresh" => Operation::new(ResourceOwned, refresh),
        "local_session_receiver_attach" => Operation::new(GlobalWrite, attach),
        "local_session_receiver_heartbeat" => Operation::new(ResourceOwned, heartbeat),
        "local_session_receiver_next" => Operation::new(ResourceOwned, next),
        "local_session_receiver_ack" => Operation::new(ResourceOwned, acknowledge),
        "local_session_receiver_detach" => Operation::new(ResourceOwned, detach),
        "local_session_conversation" => Operation::new(GlobalWrite, conversations::create),
        "local_session_join" => Operation::new(Write, conversations::join),
        "local_session_send" => Operation::new(Write, conversations::send),
        "local_session_leave" => Operation::new(Write, conversations::leave),
        _ => return None,
    })
}

fn local(request: &DaemonRequest) -> Result<(), OpError> {
    if request.args.contains_key("_cccc_web_binding") {
        return Err(OpError::new(
            "permission_denied",
            "Local session access is unavailable to cloud connectors",
        ));
    }
    Ok(())
}

fn native_id(request: &DaemonRequest, name: &str) -> Result<String, OpError> {
    let value = required_arg(request, name)?;
    if value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b))
    {
        return Err(OpError::new(
            "invalid_args",
            format!("{name} must be a native session identifier"),
        ));
    }
    Ok(value)
}

fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    let include_offline = request
        .args
        .get("include_offline")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut result = object(
        json!({"sources":store::sources(home)?.into_iter().filter(|s| include_offline || s.connected).collect::<Vec<_>>()}),
    )?;
    if let Some(source_id) = string_arg(request, "source_id") {
        let groups = GroupStore::new(home.clone()).map_err(OpError::io)?;
        let mut memberships = Vec::new();
        for meta in groups.list().map_err(OpError::io)? {
            let group = match groups.load(&meta.group_id) {
                Ok(group) => group,
                Err(error) => {
                    tracing::warn!(group_id = %meta.group_id, %error, "Skipping unavailable Group during native membership discovery");
                    continue;
                }
            };
            for actor in group
                .actors
                .iter()
                .filter(|actor| actor.external_session_id == source_id)
            {
                memberships.push(json!({
                    "group_id":group.group_id,
                    "group_title":group.title,
                    "actor_id":actor.id,
                    "role":cccc_core::actors::effective_role(&group, &actor.id),
                    "enabled":actor.enabled,
                    "group_state":group.state,
                    "group_running":group.running,
                }));
            }
        }
        result.insert("self_memberships".into(), json!(memberships));
    }
    Ok(result)
}

fn recovery_actor(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<(Actor, LocalSessionRuntime), OpError> {
    local(request)?;
    if request.args.contains_key("source_id")
        || request.args.get("by").is_some_and(|by| by != "user")
    {
        return Err(OpError::new(
            "permission_denied",
            "Local recovery instructions are available only to the user",
        ));
    }
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(&group_id))
        .map_err(OpError::not_found)?;
    let actor = cccc_core::actors::find(&group, &actor_id)
        .filter(|actor| !actor.external_session_id.is_empty())
        .cloned()
        .ok_or_else(|| OpError::new("actor_not_found", "Native Actor not found"))?;
    let runtime = match actor.runtime {
        ActorRuntime::Codex => LocalSessionRuntime::Codex,
        ActorRuntime::Claude => LocalSessionRuntime::Claude,
        ActorRuntime::Pi => LocalSessionRuntime::Pi,
        ActorRuntime::Opencode => LocalSessionRuntime::Opencode,
        _ => {
            return Err(OpError::new(
                "invalid_actor",
                "Actor is not a native source",
            ));
        }
    };
    Ok((actor, runtime))
}

fn recovery(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (actor, runtime) = recovery_actor(home, request)?;
    recovery_for_actor(home, request, actor, runtime)
}

fn recovery_for_actor(
    home: &HomeLayout,
    request: &DaemonRequest,
    actor: Actor,
    runtime: LocalSessionRuntime,
) -> OpResult {
    use LocalSessionRecoveryState as State;
    use cccc_core::local_sessions::{self as native, NativeHost, RetireReason};

    let disabled = native::integration_disabled(home, runtime).map_err(|_| {
        OpError::new(
            "local_session_recovery_unavailable",
            "Native integration status is unreadable; repair it before recovering this session",
        )
    })?;
    let session = match native::read_record(home, &actor.external_session_id) {
        Ok(session) if session.runtime == runtime => Some(session),
        _ if disabled => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        _ => {
            return Err(OpError::new(
                "local_session_recovery_unavailable",
                "Native recovery metadata is unreadable; reload the original integration to register it again",
            ));
        }
    };
    let state = if disabled {
        // The durable fence is authoritative even when source cleanup failed
        // or the lifecycle record is absent/unreadable.
        State::IntegrationDisabled
    } else {
        session.as_ref().map_or(State::Unknown, |session| {
            match session.retired {
                Some(RetireReason::SessionEnd) => return State::SessionEnded,
                Some(RetireReason::IntegrationDisabled) => return State::IntegrationDisabled,
                None => {}
            }
            let host = NativeHost::capture(session.host.pid);
            if matches!(&host, Ok(current) if current != &session.host)
                || matches!(&host, Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return State::HostExited;
            }
            if host.is_err() {
                State::Unknown
            } else if connected(home, &actor.external_session_id) {
                State::Connected
            } else {
                State::Disconnected
            }
        })
    };
    let cwd = session
        .as_ref()
        .map(|session| session.cwd.clone())
        .filter(|cwd| !cwd.is_empty());
    let shell = if cfg!(windows) {
        LocalSessionShell::PowerShell
    } else {
        LocalSessionShell::Posix
    };
    let quote = |value: &str| match shell {
        LocalSessionShell::Posix => shell_words::quote(value).into_owned(),
        LocalSessionShell::PowerShell => native::powershell_quote(value),
    };
    let mut command = match shell {
        LocalSessionShell::Posix => cwd
            .as_ref()
            .map(|cwd| format!("cd -- {} &&\n", quote(cwd)))
            .unwrap_or_default(),
        LocalSessionShell::PowerShell => format!(
            "& {{\n{}",
            cwd.as_ref()
                .map(|cwd| format!(
                    "Set-Location -LiteralPath {} -ErrorAction Stop\n",
                    quote(cwd)
                ))
                .unwrap_or_default()
        ),
    };
    let mut environment = vec![("CCCC_HOME", home.root())];
    if let Some(provider_home) = session.as_ref().and_then(|session| match runtime {
        LocalSessionRuntime::Codex => session.codex_home.as_ref(),
        LocalSessionRuntime::Claude => session.claude_config_dir.as_ref(),
        LocalSessionRuntime::Pi => session.pi_agent_dir.as_ref(),
        LocalSessionRuntime::Opencode => session.opencode_config_dir.as_ref(),
    }) {
        environment.push((
            if runtime == LocalSessionRuntime::Codex {
                "CODEX_HOME"
            } else if runtime == LocalSessionRuntime::Claude {
                "CLAUDE_CONFIG_DIR"
            } else if runtime == LocalSessionRuntime::Opencode {
                "XDG_CONFIG_HOME"
            } else {
                "PI_CODING_AGENT_DIR"
            },
            if runtime == LocalSessionRuntime::Opencode {
                provider_home.parent().unwrap_or(provider_home)
            } else {
                provider_home.as_path()
            },
        ));
    }
    for (name, path) in environment {
        let value = quote(&path.to_string_lossy());
        command.push_str(&match shell {
            LocalSessionShell::Posix => format!("{name}={value} "),
            LocalSessionShell::PowerShell => format!("$env:{name} = {value}\n"),
        });
    }
    command.push_str(match runtime {
        LocalSessionRuntime::Codex => "codex resume",
        LocalSessionRuntime::Claude => "claude --resume",
        LocalSessionRuntime::Pi => "pi --session",
        LocalSessionRuntime::Opencode => "opencode --session",
    });
    let native_id = session
        .as_ref()
        .map(|session| session.native_session_id.as_str())
        .or_else(|| {
            (runtime == LocalSessionRuntime::Codex)
                .then(|| actor.external_session_id.strip_prefix("codex:"))
                .flatten()
        })
        .filter(|id| !id.is_empty());
    if let Some(id) = native_id {
        command.push(' ');
        command.push_str(&quote(id));
    }
    if shell == LocalSessionShell::PowerShell {
        command.push_str("\n}");
    }
    object(json!({
        "group_id":required_arg(request,"group_id")?,"actor_id":actor.id,"source_id":actor.external_session_id,
        "recovery":LocalSessionRecovery{state,cwd,shell,command}
    }))
}

fn refresh(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    use cccc_contracts::local_session::LocalSessionRefreshStatus as Status;
    let (actor, runtime) = recovery_actor(home, request)?;
    let mut status = Status::NotChecked;
    if runtime == LocalSessionRuntime::Codex {
        match cccc_core::local_sessions::read(home, &actor.external_session_id) {
            Ok(session) if session.runtime == runtime && session.host.alive() => {
                status = if codex::refresh(home, &session).is_ok() {
                    Status::Checked
                } else {
                    Status::Unavailable
                };
            }
            // Projection below handles ended, exited or missing metadata. A
            // qualification query must never revive those lifecycle records.
            _ => {}
        }
    }
    // Qualification runs outside the Group permit. Never project a replacement
    // member's private recovery details under the original selection.
    let (current, current_runtime) = recovery_actor(home, request)?;
    if current.external_session_id != actor.external_session_id
        || cccc_core::actors::generation_identity(&current)
            != cccc_core::actors::generation_identity(&actor)
        || current_runtime != runtime
    {
        return Err(OpError::new(
            "local_session_changed",
            "The native member changed; reopen it before refreshing",
        ));
    }
    let mut result = recovery_for_actor(home, request, current, current_runtime)?;
    result.insert("refresh_status".into(), json!(status));
    Ok(result)
}

fn attach(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    let receiver = store::attach(
        home,
        &required_arg(request, "source_id")?,
        &native_id(request, "connection_id")?,
    )?;
    if let Err(error) = resume::restore_memberships(home, &receiver) {
        // Recovery is an additional lifecycle action, not the receive lease.
        // An unavailable registry must not disconnect already joined Groups.
        cccc_runtime::diagnostic(format_args!(
            "Could not inspect native conversation recovery: source_id={} error_code={}",
            receiver.source_id, error.code
        ));
    }
    object(json!({"source":receiver.source()}))
}

fn heartbeat(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    let receiver = store::heartbeat(
        home,
        &required_arg(request, "source_id")?,
        &required_arg(request, "connection_id")?,
    )?;
    object(json!({"source":receiver.source()}))
}

fn next(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    let receiver = store::receiver(
        home,
        &required_arg(request, "source_id")?,
        &required_arg(request, "connection_id")?,
    )?;
    let handoff = transport::next(home, &receiver)?;
    object(json!({"source":receiver.source(),"handoff":handoff}))
}

fn acknowledge(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    let receiver = store::receiver(
        home,
        &required_arg(request, "source_id")?,
        &required_arg(request, "connection_id")?,
    )?;
    let reason = string_arg(request, "reason").unwrap_or_default();
    let outcome = match required_arg(request, "state")?.as_str() {
        "accepted" => transport::Outcome::Accepted,
        "failed" => transport::Outcome::Rejected(reason),
        "ambiguous" => transport::Outcome::Ambiguous(reason),
        _ => {
            return Err(OpError::new(
                "invalid_args",
                "state must be accepted, failed or ambiguous",
            ));
        }
    };
    transport::acknowledge(
        home,
        &receiver,
        &required_arg(request, "handoff_id")?,
        outcome,
    )?;
    object(json!({"recorded":true}))
}

pub(super) fn connected(home: &HomeLayout, source_id: &str) -> bool {
    store::connected(home, source_id)
}

fn detach(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    store::detach(
        home,
        &required_arg(request, "source_id")?,
        &required_arg(request, "connection_id")?,
    )?;
    object(json!({"detached":true}))
}

pub(super) fn session_marker(home: &HomeLayout, source_id: &str) -> Option<String> {
    cccc_core::local_sessions::read(home, source_id)
        .ok()
        .filter(cccc_core::local_sessions::NativeSession::host_alive)
        .map(|session| session.revision)
}

pub(super) enum PreflightError {
    Unavailable,
    Rejected(String),
}

/// Validate the fully rendered native payload without crossing a handoff or
/// recording a claim. Claude's receiver transport has no argv message limit.
pub(super) fn preflight(
    home: &HomeLayout,
    actor: &Actor,
    text: &str,
) -> Result<(), PreflightError> {
    if actor.runtime == ActorRuntime::Codex {
        codex::preflight(home, &actor.external_session_id, text)?;
    }
    Ok(())
}

pub(super) fn submit(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    events: &[cccc_contracts::Event],
    text: String,
    cancelled: &AtomicBool,
) -> transport::Outcome {
    if !actor.enabled || matches!(group.state, GroupState::Paused | GroupState::Stopped) {
        return transport::Outcome::Unavailable;
    }
    if actor.runtime == ActorRuntime::Codex {
        return codex::submit(home, &actor.external_session_id, &text, cancelled);
    }
    transport::submit(
        home,
        transport::Handoff {
            handoff_id: String::new(),
            source_id: actor.external_session_id.clone(),
            native_session_id: String::new(),
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            actor_generation: cccc_core::actors::generation_identity(actor),
            event_ids: events.iter().map(|e| e.id.clone()).collect(),
            text,
        },
        cancelled,
    )
}

pub(super) use transport::Outcome as DeliveryOutcome;

fn actor_runtime(runtime: LocalSessionRuntime) -> ActorRuntime {
    match runtime {
        LocalSessionRuntime::Codex => ActorRuntime::Codex,
        LocalSessionRuntime::Claude => ActorRuntime::Claude,
        LocalSessionRuntime::Pi => ActorRuntime::Pi,
        LocalSessionRuntime::Opencode => ActorRuntime::Opencode,
    }
}
