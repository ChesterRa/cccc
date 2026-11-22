use cccc_contracts::event::DeliveryEvidence;
use cccc_core::HomeLayout;
use std::io;

/// Use only for a finite synchronous projection, never a wait/follow loop or
/// delivery decision. Nested projections share the same observation.
pub(crate) fn read_scope<T>(read: impl FnOnce() -> T) -> T {
    #[cfg(unix)]
    return super::projection::read_scope(read);
    #[cfg(not(unix))]
    read()
}

pub(crate) fn enabled(home: &HomeLayout) -> bool {
    #[cfg(unix)]
    if super::platform_supported() {
        return super::controller::enabled(home);
    }
    {
        let _ = home;
        false
    }
}

pub(crate) fn require_available(
    home: &HomeLayout,
    group: Option<&str>,
    actor: Option<&str>,
) -> io::Result<()> {
    #[cfg(unix)]
    if enabled(home) {
        return super::controller::require_available(home, group, actor);
    }
    let _ = (home, group, actor);
    Ok(())
}

pub(crate) fn initialize(home: &HomeLayout) -> io::Result<Vec<super::AgentIdentity>> {
    #[cfg(unix)]
    if super::platform_supported() {
        return super::controller::initialize(home);
    }
    {
        let _ = home;
        Ok(Vec::new())
    }
}

#[cfg(unix)]
pub(crate) fn snapshot(
    group: &str,
    actor: &str,
) -> io::Result<Option<super::protocol::ActorSnapshot>> {
    if super::is_owner() {
        return Ok(None);
    }
    super::controller::snapshot(group, actor)
}

#[cfg(unix)]
pub(crate) fn launch_managed(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &cccc_contracts::Actor,
) -> io::Result<()> {
    let host_environment = if actor.runtime == cccc_contracts::ActorRuntime::Claude {
        crate::ops::codex_voice_analyst::claude_host_environment(cccc_runtime::unicode_environment())
    } else {
        cccc_runtime::unicode_environment().collect()
    };
    super::controller::start(
        home,
        actor_ref(group, actor),
        super::protocol::PreparedLaunch::Managed {
            group: group.clone(),
            actor: actor.clone(),
            host_environment,
        },
    )?;
    Ok(())
}

#[cfg(unix)]
pub(crate) fn launch_pty(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    configured: &cccc_contracts::Actor,
    mut spec: cccc_runtime::LaunchSpec,
    history: cccc_runtime::HistoryConfig,
    launch: cccc_runtime::ActorLaunch,
) -> io::Result<cccc_runtime::SessionStatus> {
    let mut environment =
        cccc_runtime::unicode_environment().collect::<std::collections::BTreeMap<_, _>>();
    environment.extend(spec.env);
    spec.env = environment;
    let actor = super::protocol::ActorRef {
        group_id: spec.group_id.clone(),
        actor_id: spec.actor_id.clone(),
        actor_created_at: configured.created_at.clone(),
        generation: None,
    };
    let preamble = crate::ops::actor_delivery_preamble::render(home, group, configured);
    super::controller::start(
        home,
        actor,
        super::protocol::PreparedLaunch::Pty {
            spec,
            history,
            launch,
            preamble,
        },
    )?
    .terminal
    .ok_or_else(|| io::Error::other("PTY launch returned no terminal status"))
}

#[cfg(unix)]
pub(crate) fn launch_deepseek(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &cccc_contracts::Actor,
    cwd: &std::path::Path,
    private_values: Vec<String>,
) -> io::Result<()> {
    super::controller::start(
        home,
        actor_ref(group, actor),
        super::protocol::PreparedLaunch::DeepSeek {
            group: group.clone(),
            actor: actor.clone(),
            cwd: cwd.to_path_buf(),
            private_values,
        },
    )?;
    Ok(())
}

/// Returns None only when this process has no remote registration for the key.
/// A failed query is an error, including on explicit Stop.
#[cfg(unix)]
pub(crate) fn stop_actor(
    group: &str,
    actor: &str,
) -> io::Result<Option<Option<cccc_runtime::SessionStatus>>> {
    let Some(snapshot) = snapshot(group, actor)? else {
        return Ok(None);
    };
    let status = snapshot.terminal;
    super::controller::perform(
        &snapshot.actor,
        super::protocol::Mutation::Stop {
            actor: snapshot.actor.clone(),
        },
    )?;
    Ok(Some(status.map(|mut status| {
        status.running = false;
        status
    })))
}

#[cfg(unix)]
pub(crate) fn control(
    group: &str,
    actor: &str,
    generation: Option<&str>,
    operation: Control,
) -> io::Result<Option<serde_json::Value>> {
    let Some(snapshot) = snapshot(group, actor)? else {
        return Ok(None);
    };
    let mut identity = snapshot.actor;
    if let Some(generation) = generation {
        identity.generation = Some(generation.into());
    }
    use super::protocol::Mutation;
    let mutation = match operation {
        Control::Permission { request_id, allow } => Mutation::Permission {
            actor: identity.clone(),
            request_id,
            allow,
        },
        Control::Interaction { request_id, reply } => Mutation::Interaction {
            actor: identity.clone(),
            request_id,
            reply,
        },
        Control::CancelTurn => Mutation::CancelTurn {
            actor: identity.clone(),
        },
        Control::EnsureViewer => Mutation::EnsureViewer {
            actor: identity.clone(),
        },
        Control::DetachViewer => Mutation::DetachViewer {
            actor: identity.clone(),
        },
        Control::PauseConfirmation => Mutation::PauseConfirmation {
            actor: identity.clone(),
        },
        Control::RetryConfirmation => Mutation::RetryConfirmation {
            actor: identity.clone(),
        },
    };
    super::controller::perform(&identity, mutation).map(Some)
}

#[cfg(unix)]
pub(crate) enum Control {
    Permission {
        request_id: String,
        allow: bool,
    },
    Interaction {
        request_id: String,
        reply: serde_json::Value,
    },
    CancelTurn,
    EnsureViewer,
    DetachViewer,
    PauseConfirmation,
    RetryConfirmation,
}

#[cfg(unix)]
fn actor_ref(
    group: &cccc_core::GroupDoc,
    actor: &cccc_contracts::Actor,
) -> super::protocol::ActorRef {
    super::protocol::ActorRef {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        actor_created_at: actor.created_at.clone(),
        generation: None,
    }
}

pub(crate) fn finish(
    home: &HomeLayout,
    mode: super::ShutdownMode,
    request_id: String,
) -> io::Result<super::ShutdownReport> {
    #[cfg(unix)]
    if enabled(home) {
        return super::controller::finish(home, mode, request_id);
    }
    Ok(super::ShutdownReport {
        v: 1,
        daemon_pid: std::process::id(),
        request_id,
        mode,
        confirmed_stopped: Vec::new(),
        retained: Vec::new(),
        unconfirmed: Vec::new(),
    })
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &cccc_contracts::Actor,
    events: &[cccc_contracts::Event],
    text: &str,
    cancelled: &std::sync::atomic::AtomicBool,
    started: &std::sync::atomic::AtomicBool,
) -> io::Result<serde_json::Value> {
    super::controller::submit(home, group, actor, events, text, cancelled, started)
}

pub(crate) fn acknowledge_event(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
) -> io::Result<()> {
    #[cfg(unix)]
    if enabled(home) {
        return super::controller::acknowledge_event(home, group, actor, created, event);
    }
    let _ = (home, group, actor, created, event);
    Ok(())
}

pub(crate) enum RetainedDelivery {
    Pending,
    Accepted(Option<DeliveryEvidence>),
    Unsubmitted,
    Failed(String),
    Withdrawn,
    Unknown,
}

pub(crate) fn delivery_fact(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
    transport: &str,
) -> io::Result<RetainedDelivery> {
    let fact = observe_delivery(home, group, actor, created, event)?;
    #[cfg(unix)]
    if matches!(fact, RetainedDelivery::Pending) {
        super::controller::track_recovery(home, group, actor, created, event, transport)?;
    }
    let _ = transport;
    Ok(fact)
}

/// Observe an existing attempt without registering another settlement worker.
/// Live delivery workers own their own claim; startup recovery is separate.
pub(crate) fn observe_delivery(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
) -> io::Result<RetainedDelivery> {
    #[cfg(unix)]
    if enabled(home) {
        use super::protocol::OperationState;
        let mut facts = super::controller::event_facts(home, group, actor, created, event)?;
        if facts.len() != 1 {
            return Ok(RetainedDelivery::Unknown);
        }
        let (_, fact) = facts.pop().expect("one execution fact");
        return Ok(match fact.state {
            OperationState::Pending => RetainedDelivery::Pending,
            OperationState::NotAdmitted => RetainedDelivery::Unsubmitted,
            OperationState::Finished { result: Ok(result) } => match result
                .get("backend")
                .and_then(serde_json::Value::as_str)
            {
                Some("managed") => {
                    match crate::ops::local_headless::BatchSubmission::from_owner_result(&result) {
                        Some(crate::ops::local_headless::BatchSubmission::Accepted(evidence)) => {
                            RetainedDelivery::Accepted(evidence)
                        }
                        Some(crate::ops::local_headless::BatchSubmission::Withheld) => {
                            RetainedDelivery::Withdrawn
                        }
                        Some(crate::ops::local_headless::BatchSubmission::Deferred)
                            if result.get("started").and_then(serde_json::Value::as_bool)
                                == Some(false) =>
                        {
                            RetainedDelivery::Unsubmitted
                        }
                        _ => RetainedDelivery::Unknown,
                    }
                }
                Some("deepseek") => match result.get("outcome").cloned().and_then(|value| {
                    serde_json::from_value::<crate::ops::deepseek_runtime::TurnDelivery>(value).ok()
                }) {
                    Some(crate::ops::deepseek_runtime::TurnDelivery::Completed) => {
                        RetainedDelivery::Accepted(DeliveryEvidence::from_json(
                            &result["delivery_evidence"],
                        ))
                    }
                    Some(crate::ops::deepseek_runtime::TurnDelivery::Failed(reason)) => {
                        RetainedDelivery::Failed(reason)
                    }
                    Some(crate::ops::deepseek_runtime::TurnDelivery::Incomplete)
                        if result.get("started").and_then(serde_json::Value::as_bool)
                            == Some(false) =>
                    {
                        RetainedDelivery::Unsubmitted
                    }
                    _ => RetainedDelivery::Unknown,
                },
                Some("pty") => match result.get("outcome").and_then(serde_json::Value::as_str) {
                    Some("accepted") => RetainedDelivery::Accepted(DeliveryEvidence::from_json(
                        &result["delivery_evidence"],
                    )),
                    Some("not_written") => RetainedDelivery::Unsubmitted,
                    _ => RetainedDelivery::Unknown,
                },
                _ => RetainedDelivery::Unknown,
            },
            _ => RetainedDelivery::Unknown,
        });
    }
    let _ = (home, group, actor, created, event);
    Ok(RetainedDelivery::Unknown)
}

pub(crate) fn recovery_groups() -> Vec<String> {
    #[cfg(unix)]
    return super::controller::recovery_groups();
    #[cfg(not(unix))]
    Vec::new()
}

pub(crate) fn recoveries(
    home: &HomeLayout,
    group: &str,
) -> io::Result<Vec<(String, String, String, String)>> {
    #[cfg(unix)]
    if enabled(home) {
        return super::controller::recoveries(home, group);
    }
    let _ = (home, group);
    Ok(Vec::new())
}

pub(crate) fn forget_recovery(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
) -> io::Result<()> {
    #[cfg(unix)]
    if enabled(home) {
        return super::controller::forget_recovery(home, group, actor, created, event);
    }
    let _ = (home, group, actor, created, event);
    Ok(())
}

pub(crate) fn exited_terminals() -> io::Result<Vec<cccc_runtime::SessionStatus>> {
    #[cfg(unix)]
    return super::controller::exited_terminals();
    #[cfg(not(unix))]
    Ok(Vec::new())
}

pub(crate) fn provider_exits(
    home: &HomeLayout,
    group: &str,
) -> io::Result<Vec<super::AgentIdentity>> {
    #[cfg(unix)]
    if enabled(home) {
        return super::controller::provider_exits(home, group);
    }
    let _ = (home, group);
    Ok(Vec::new())
}
