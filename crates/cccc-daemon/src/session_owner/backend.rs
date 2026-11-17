use super::protocol::{
    ActorRef, ActorSnapshot, Failure, Mutation, PreparedLaunch, TerminalAction, TerminalRead,
};
use crate::ops::{actor_runtime, deepseek_runtime, local_headless};
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

type Key = (String, String);

#[derive(Default)]
pub(super) struct Backend {
    pub(super) browsers: super::browser_backend::Browsers,
    mcp: Mutex<super::mcp::Registry>,
    actors: Mutex<BTreeMap<Key, ActorRef>>,
    cancelled: Mutex<BTreeMap<Key, Arc<AtomicBool>>>,
    starting: Mutex<BTreeSet<Key>>,
    started: Condvar,
    stopping: AtomicBool,
    stop_all_pending: AtomicBool,
    stop_pending: Mutex<BTreeSet<Key>>,
    environments: Mutex<BTreeMap<Key, BTreeMap<String, String>>>,
    preambles: Mutex<BTreeMap<Key, Arc<Mutex<Option<String>>>>>,
    provider_exits: Mutex<BTreeMap<Key, String>>,
    detached_work: Mutex<BTreeMap<Key, cccc_runtime::DetachedWork>>,
}

impl Backend {
    pub(super) fn with_owner(home: HomeLayout, owner_id: String, instance: String) -> Self {
        Self {
            browsers: super::browser_backend::Browsers::new(owner_id.clone(), instance.clone()),
            mcp: Mutex::new(super::mcp::Registry::new(home, owner_id, instance)),
            ..Self::default()
        }
    }

    pub(super) fn set_browser_launcher(
        &mut self,
        launcher: Option<Arc<dyn super::browser::BrowserLauncher>>,
    ) {
        self.browsers.set_launcher(launcher);
    }

    pub(super) fn mcp_binding_file(&self, group: &str, actor: &str) -> Option<std::path::PathBuf> {
        self.mcp.lock().ok()?.file(group, actor)
    }

    #[cfg(test)]
    pub(super) fn mcp_binding(
        &self,
        group: &str,
        actor: &str,
    ) -> Option<cccc_core::owned_mcp::Binding> {
        self.mcp.lock().ok()?.binding(group, actor)
    }

    pub(super) fn register_mcp(
        &self,
        native: cccc_core::local_sessions::NativeHost,
        report: cccc_core::owned_mcp::Registration,
    ) -> Result<(), Failure> {
        self.mcp
            .lock()
            .map_err(|_| poisoned())?
            .register(native, report)
    }
    pub(super) fn owner_snapshot(&self) -> Result<super::protocol::OwnerSnapshot, Failure> {
        Ok(super::protocol::OwnerSnapshot {
            actors: self.snapshots()?,
            browsers: self.browsers.snapshots()?,
        })
    }

    pub(super) fn control_acquired(&self, epoch: u64) -> Result<(), Failure> {
        self.browsers.control_acquired(epoch)
    }

    pub(super) fn snapshots(&self) -> Result<Vec<ActorSnapshot>, Failure> {
        let actors = self
            .actors
            .lock()
            .map_err(|_| poisoned())?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut snapshots = actors
            .iter()
            .map(|actor| snapshot(actor).unwrap_or_else(|error| unknown_snapshot(actor, error)))
            .collect::<Vec<_>>();
        {
            let mut mcp = self.mcp.lock().map_err(|_| poisoned())?;
            mcp.refresh();
            for snapshot in &mut snapshots {
                snapshot.mcp = mcp.snapshot(&snapshot.actor);
            }
        }
        {
            let work = self.detached_work.lock().map_err(|_| poisoned())?;
            let starting = self.starting.lock().map_err(|_| poisoned())?;
            for snapshot in &mut snapshots {
                if snapshot.owner.is_none()
                    && work.contains_key(&key(&snapshot.actor))
                    && !starting.contains(&key(&snapshot.actor))
                {
                    snapshot.observation_error = Some(Failure::new(
                        "detached_work_unconfirmed",
                        "a failed native launch may have left an independent job; missing session registration does not prove it stopped",
                    ));
                }
            }
        }
        {
            let exits = self.provider_exits.lock().map_err(|_| poisoned())?;
            for snapshot in &mut snapshots {
                snapshot.provider_exited = snapshot
                    .actor
                    .generation
                    .as_ref()
                    .is_some_and(|generation| exits.get(&key(&snapshot.actor)) == Some(generation));
            }
        }
        // Publish only a still-matching registration. Actual provider reads
        // happen before this short metadata lock, outside the admission mutex.
        let mut registered = self.actors.lock().map_err(|_| poisoned())?;
        for (before, snapshot) in actors.iter().zip(&snapshots) {
            if registered.get(&key(before)) == Some(before) {
                registered.insert(key(before), snapshot.actor.clone());
            }
        }
        Ok(snapshots)
    }

    pub(super) fn validate(&self, mutation: &Mutation) -> Result<(), Failure> {
        if matches!(
            mutation,
            Mutation::BrowserEnsure { .. }
                | Mutation::BrowserPagesSet { .. }
                | Mutation::BrowserClose { .. }
        ) {
            if matches!(mutation, Mutation::BrowserEnsure { .. })
                && self.stopping.load(Ordering::Acquire)
            {
                return Err(Failure::new(
                    "owner_stopping",
                    "the browser holder is stopping",
                ));
            }
            return self.browsers.validate(mutation);
        }
        if matches!(mutation, Mutation::StopAll) && self.stop_all_pending.load(Ordering::Acquire) {
            return Err(Failure::new(
                "already_stopping",
                "all member stops are already in progress",
            ));
        }
        if let Mutation::Stop { actor } = mutation
            && (self.stop_all_pending.load(Ordering::Acquire)
                || self
                    .stop_pending
                    .lock()
                    .map_err(|_| poisoned())?
                    .contains(&key(actor)))
        {
            return Err(Failure::new(
                "already_stopping",
                "this member stop is already in progress",
            ));
        }
        let wanted = match mutation {
            Mutation::Start { actor, launch } => {
                if self.stopping.load(Ordering::Acquire) {
                    return Err(Failure::new("owner_stopping", "session owner is stopping"));
                }
                let matches = match &**launch {
                    PreparedLaunch::Managed {
                        group,
                        actor: configured,
                        ..
                    }
                    | PreparedLaunch::DeepSeek {
                        group,
                        actor: configured,
                        ..
                    } => {
                        actor.group_id == group.group_id
                            && actor.actor_id == configured.id
                            && actor.actor_created_at == configured.created_at
                    }
                    PreparedLaunch::Pty { spec, .. } => {
                        actor.group_id == spec.group_id && actor.actor_id == spec.actor_id
                    }
                };
                if !matches {
                    return Err(Failure::new(
                        "actor_mismatch",
                        "launch identity does not match the actor",
                    ));
                }
                // Start may retry only after querying its prior operation. An
                // existing registration cannot be silently replaced by Start.
                if self
                    .actors
                    .lock()
                    .map_err(|_| poisoned())?
                    .contains_key(&key(actor))
                {
                    return Err(Failure::new(
                        "actor_owned",
                        "actor already has a registered owner",
                    ));
                }
                return Ok(());
            }
            Mutation::Submit {
                source,
                group,
                actor,
                events,
                text,
            } => {
                use sha2::{Digest, Sha256};
                if source.actor.group_id != group.group_id
                    || source.actor.actor_id != actor.id
                    || source.actor.actor_created_at != actor.created_at
                    || events.is_empty()
                    || events.len() > 64
                    || source.event_ids
                        != events
                            .iter()
                            .map(|event| event.id.clone())
                            .collect::<Vec<_>>()
                    || source.input_sha256 != format!("{:x}", Sha256::digest(text.as_bytes()))
                {
                    return Err(Failure::new(
                        "invalid_input",
                        "delivery identity, sources or input digest do not match",
                    ));
                }
                &source.actor
            }
            Mutation::Stop { actor }
            | Mutation::Permission { actor, .. }
            | Mutation::Interaction { actor, .. }
            | Mutation::CancelTurn { actor }
            | Mutation::EnsureViewer { actor }
            | Mutation::DetachViewer { actor }
            | Mutation::PauseConfirmation { actor }
            | Mutation::RetryConfirmation { actor }
            | Mutation::Terminal { actor, .. } => actor,
            Mutation::StopAll
            | Mutation::BrowserEnsure { .. }
            | Mutation::BrowserPagesSet { .. }
            | Mutation::BrowserClose { .. }
            | Mutation::Acknowledge { .. }
            | Mutation::Retire
            | Mutation::Recovery { .. } => return Ok(()),
        };
        let registered = self
            .actors
            .lock()
            .map_err(|_| poisoned())?
            .get(&key(wanted))
            .cloned()
            .ok_or_else(|| Failure::new("actor_not_owned", "actor is not registered here"))?;
        if self
            .starting
            .lock()
            .map_err(|_| poisoned())?
            .contains(&key(wanted))
        {
            return Err(Failure::new(
                "actor_starting",
                "actor launch is still in progress",
            ));
        }
        if registered.actor_created_at != wanted.actor_created_at {
            return Err(Failure::new(
                "actor_replaced",
                "actor creation identity changed",
            ));
        }
        if registered.generation != wanted.generation {
            return Err(Failure::new(
                "session_replaced",
                "session generation changed",
            ));
        }
        Ok(())
    }

    /// Called under the admission lock, before a task can be scheduled. Pending
    /// launches already own their member; StopAll closes their cancellation gate.
    pub(super) fn reserve(&self, mutation: &Mutation) -> Result<(), Failure> {
        match mutation {
            Mutation::BrowserEnsure { .. }
            | Mutation::BrowserPagesSet { .. }
            | Mutation::BrowserClose { .. } => {
                self.browsers.reserve(mutation)?;
            }
            Mutation::Start { actor, launch } => {
                if let PreparedLaunch::Managed {
                    group,
                    actor: configured,
                    ..
                } = &**launch
                {
                    self.mcp.lock().map_err(|_| poisoned())?.reserve(
                        actor,
                        group,
                        matches!(
                            configured.runtime,
                            cccc_contracts::ActorRuntime::Codex
                                | cccc_contracts::ActorRuntime::Claude
                        ),
                    )?;
                }
                let actor_key = key(actor);
                self.actors
                    .lock()
                    .map_err(|_| poisoned())?
                    .insert(actor_key.clone(), actor.clone());
                self.cancelled
                    .lock()
                    .map_err(|_| poisoned())?
                    .insert(actor_key.clone(), Arc::new(AtomicBool::new(false)));
                self.starting
                    .lock()
                    .map_err(|_| poisoned())?
                    .insert(actor_key);
                if let PreparedLaunch::Managed {
                    host_environment, ..
                } = &**launch
                {
                    self.environments
                        .lock()
                        .map_err(|_| poisoned())?
                        .insert(key(actor), host_environment.clone());
                }
                if let PreparedLaunch::Pty { preamble, .. } = &**launch {
                    self.preambles
                        .lock()
                        .map_err(|_| poisoned())?
                        .insert(key(actor), Arc::new(Mutex::new(Some(preamble.clone()))));
                }
            }
            Mutation::StopAll => {
                self.browsers.prevent_starts()?;
                self.stop_all_pending.store(true, Ordering::Release);
                self.stopping.store(true, Ordering::Release);
                for cancelled in self.cancelled.lock().map_err(|_| poisoned())?.values() {
                    cancelled.store(true, Ordering::Release);
                }
            }
            Mutation::Stop { actor } => {
                self.stop_pending
                    .lock()
                    .map_err(|_| poisoned())?
                    .insert(key(actor));
                if let Some(cancelled) = self
                    .cancelled
                    .lock()
                    .map_err(|_| poisoned())?
                    .get(&key(actor))
                {
                    cancelled.store(true, Ordering::Release);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn execute(&self, home: &HomeLayout, mutation: Mutation) -> Result<Value, Failure> {
        self.execute_in_epoch(home, mutation, 0)
    }

    pub(super) fn execute_in_epoch(
        &self,
        home: &HomeLayout,
        mutation: Mutation,
        epoch: u64,
    ) -> Result<Value, Failure> {
        let _stop_completion = match &mutation {
            Mutation::Stop { actor } => Some(StopCompletion {
                backend: self,
                key: Some(key(actor)),
            }),
            Mutation::StopAll => Some(StopCompletion {
                backend: self,
                key: None,
            }),
            _ => None,
        };
        let wanted = match &mutation {
            Mutation::Submit { source, .. } => Some(&source.actor),
            Mutation::Stop { actor }
            | Mutation::Permission { actor, .. }
            | Mutation::Interaction { actor, .. }
            | Mutation::CancelTurn { actor }
            | Mutation::EnsureViewer { actor }
            | Mutation::DetachViewer { actor }
            | Mutation::PauseConfirmation { actor }
            | Mutation::RetryConfirmation { actor }
            | Mutation::Terminal { actor, .. } => Some(actor),
            _ => None,
        };
        if let Some(wanted) = wanted {
            let registered = self
                .actors
                .lock()
                .map_err(|_| poisoned())?
                .get(&key(wanted))
                .cloned()
                .ok_or_else(|| Failure::new("actor_not_owned", "actor stopped before execution"))?;
            if registered.actor_created_at != wanted.actor_created_at
                || snapshot(&registered)?.actor.generation != wanted.generation
            {
                return Err(Failure::new(
                    "session_replaced",
                    "session changed before execution",
                ));
            }
        }
        match mutation {
            Mutation::BrowserEnsure { spec } => {
                serde_json::to_value(self.browsers.ensure(spec)?).map_err(serialization_failure)
            }
            Mutation::BrowserPagesSet {
                resource_id,
                sessions,
                ..
            } => serde_json::to_value(self.browsers.set_pages(&resource_id, sessions, epoch)?)
                .map_err(serialization_failure),
            Mutation::BrowserClose { resource_id, .. } => {
                self.browsers
                    .close(&resource_id, std::time::Duration::from_secs(15))?;
                Ok(json!({"closed":true}))
            }
            Mutation::Start { actor, launch } => {
                let actor_key = key(&actor);
                let _completion = StartCompletion {
                    backend: self,
                    key: actor_key,
                };
                if self.stopping.load(Ordering::Acquire) {
                    return Err(Failure::new(
                        "owner_stopping",
                        "launch was withdrawn before provider startup",
                    ));
                }
                let launched = match *launch {
                    PreparedLaunch::Managed { group, actor, .. } => {
                        local_headless::start(home, &group, &actor).map_err(io_failure)
                    }
                    PreparedLaunch::Pty {
                        spec,
                        history,
                        launch,
                        ..
                    } => cccc_runtime::start_actor_with_history(spec, history, launch)
                        .map(|_| ())
                        .map_err(runtime_failure),
                    PreparedLaunch::DeepSeek {
                        group,
                        actor,
                        cwd,
                        private_values,
                    } => deepseek_runtime::start_with_private_values(
                        home,
                        &group,
                        &actor,
                        &cwd,
                        private_values,
                    )
                    .map_err(io_failure),
                };
                if let Err(error) = launched {
                    // Reuse each backend's confirmed cleanup. A missing
                    // executable can then be retried, while failed cleanup
                    // leaves the holding registration available for Stop.
                    if let Err(cleanup) = self.stop(&actor) {
                        if let Some(lease) = self
                            .detached_work
                            .lock()
                            .map_err(|_| poisoned())?
                            .get(&key(&actor))
                        {
                            lease.mark_uncertain().map_err(io_failure)?;
                        }
                        return Err(Failure::new(
                            "launch_cleanup_unconfirmed",
                            format!("{error}; {cleanup}"),
                        ));
                    }
                    return Err(error);
                }
                let current = snapshot(&actor)?;
                self.actors
                    .lock()
                    .map_err(|_| poisoned())?
                    .insert(key(&actor), current.actor.clone());
                serde_json::to_value(current).map_err(serialization_failure)
            }
            Mutation::Stop { actor } => {
                self.stop(&actor)?;
                Ok(json!({"stopped":true}))
            }
            Mutation::StopAll => {
                let browser_before = self.browsers.snapshots();
                let actors = self
                    .actors
                    .lock()
                    .map_err(|_| poisoned())?
                    .values()
                    .cloned()
                    .collect::<Vec<_>>();
                let mut stopped = Vec::new();
                let mut unconfirmed = Vec::new();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                for actor in actors {
                    let mut starting = self.starting.lock().map_err(|_| poisoned())?;
                    while starting.contains(&key(&actor)) {
                        let remaining =
                            deadline.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        starting = self
                            .started
                            .wait_timeout(starting, remaining)
                            .map_err(|_| poisoned())?
                            .0;
                    }
                    let pending = starting.contains(&key(&actor));
                    drop(starting);
                    if pending {
                        unconfirmed.push(actor);
                        continue;
                    }
                    // A failed observation must not prevent stopping the rest.
                    // The known registration still identifies the resources
                    // that an explicit Stop is allowed to reclaim.
                    let actual = snapshot(&actor).map_or_else(|_| actor.clone(), |s| s.actor);
                    if self.stop(&actor).is_ok() {
                        stopped.push(actual);
                    } else {
                        unconfirmed.push(actual);
                    }
                }
                let browsers = self
                    .browsers
                    .stop_all(deadline.saturating_duration_since(std::time::Instant::now()));
                let browser_actor = |member: super::receipt::AgentIdentity| ActorRef {
                    group_id: member.group_id,
                    actor_id: member.actor_id,
                    actor_created_at: member.actor_created_at,
                    generation: member.generation,
                };
                let browser_error = match browsers {
                    Ok(report) => {
                        stopped.extend(report.stopped.into_iter().map(browser_actor));
                        unconfirmed.extend(report.unconfirmed.into_iter().map(browser_actor));
                        report.error
                    }
                    Err(error) => {
                        if let Ok(before) = browser_before {
                            unconfirmed.extend(
                                before
                                    .iter()
                                    .flat_map(|browser| browser.identities())
                                    .map(browser_actor),
                            );
                        }
                        Some(error)
                    }
                };
                Ok(
                    json!({"stopped":stopped,"unconfirmed":unconfirmed,"browsers_stopped":browser_error.is_none(),"browser_error":browser_error}),
                )
            }
            Mutation::Submit {
                source,
                group,
                actor,
                events,
                text,
            } => {
                let cancelled = self
                    .cancelled
                    .lock()
                    .map_err(|_| poisoned())?
                    .get(&key(&source.actor))
                    .cloned()
                    .ok_or_else(|| Failure::new("actor_not_owned", "actor stopped"))?;
                let started = AtomicBool::new(false);
                if deepseek_runtime::registered_generation(&group.group_id, &actor.id).is_some() {
                    if events.len() != 1 {
                        return Err(Failure::new(
                            "invalid_input",
                            "DeepSeek executes one event per operation",
                        ));
                    }
                    let event = events.first().ok_or_else(|| {
                        Failure::new("invalid_input", "DeepSeek needs one source event")
                    })?;
                    let result = deepseek_runtime::deliver_prepared(
                        home, &group, &actor, event, &text, &cancelled, &started,
                    );
                    Ok(
                        json!({"backend":"deepseek","outcome":result,"started":started.load(Ordering::Acquire)}),
                    )
                } else if local_headless::registered_running(&group.group_id, &actor.id).is_some() {
                    let result = local_headless::submit_prepared(
                        home, &group, &actor, &events, &text, &cancelled, &started,
                    );
                    Ok(
                        json!({"backend":"managed","outcome":result,"started":started.load(Ordering::Acquire)}),
                    )
                } else {
                    let expected = cccc_runtime::status(&group.group_id, &actor.id)
                        .map_err(runtime_failure)?;
                    let launched = expected.launch.map_or_else(
                        || (*actor).clone(),
                        |launch| actor_runtime::as_launched(&actor, launch),
                    );
                    let preamble = self
                        .preambles
                        .lock()
                        .map_err(|_| poisoned())?
                        .get(&key(&source.actor))
                        .cloned()
                        .ok_or_else(|| {
                            Failure::new("owner_error", "PTY startup context is missing")
                        })?;
                    let mut preamble = preamble.lock().map_err(|_| poisoned())?;
                    let mut payload = text;
                    if let Some(context) = preamble.as_ref() {
                        if launched.runtime != cccc_contracts::ActorRuntime::Custom {
                            if !crate::ops::actor_delivery_worker::wait_for_input_mode(
                                &group.group_id,
                                &actor.id,
                                &cancelled,
                            ) {
                                return Ok(json!({"backend":"pty","outcome":"not_written"}));
                            }
                            if launched.runtime == cccc_contracts::ActorRuntime::Antigravity
                                && !crate::ops::actor_delivery_worker::interruptible_sleep(
                                    std::time::Duration::from_millis(1500),
                                    &cancelled,
                                )
                            {
                                return Ok(json!({"backend":"pty","outcome":"not_written"}));
                            }
                        }
                        if launched.runtime == cccc_contracts::ActorRuntime::Custom {
                            match crate::ops::actor_delivery::submit_terminal_text_for_session(
                                &expected, &launched, context, &cancelled,
                            ) {
                                cccc_runtime::PtyHandoff::Accepted => {
                                    *preamble = None;
                                    if !crate::ops::actor_delivery_worker::interruptible_sleep(
                                        std::time::Duration::from_millis(500),
                                        &cancelled,
                                    ) {
                                        return Ok(
                                            json!({"backend":"pty","outcome":"not_written"}),
                                        );
                                    }
                                }
                                cccc_runtime::PtyHandoff::NotWritten(reason) => {
                                    return Ok(
                                        json!({"backend":"pty","outcome":"not_written","reason":reason.to_string()}),
                                    );
                                }
                                cccc_runtime::PtyHandoff::Uncertain(reason) => {
                                    return Ok(
                                        json!({"backend":"pty","outcome":"uncertain","reason":reason.to_string()}),
                                    );
                                }
                            }
                        } else {
                            payload = format!("{}\n\n{payload}", context.trim_end());
                        }
                    }
                    let result = crate::ops::actor_delivery::submit_terminal_text_for_session(
                        &expected, &launched, &payload, &cancelled,
                    );
                    if matches!(result, cccc_runtime::PtyHandoff::Accepted) {
                        *preamble = None;
                    }
                    let (outcome, reason) = match result {
                        cccc_runtime::PtyHandoff::Accepted => ("accepted", None),
                        cccc_runtime::PtyHandoff::NotWritten(reason) => {
                            ("not_written", Some(reason.to_string()))
                        }
                        cccc_runtime::PtyHandoff::Uncertain(reason) => {
                            ("uncertain", Some(reason.to_string()))
                        }
                    };
                    Ok(json!({"backend":"pty","outcome":outcome,"reason":reason}))
                }
            }
            Mutation::Permission {
                actor,
                request_id,
                allow,
            } => {
                local_headless::respond_permission(
                    &actor.group_id,
                    &actor.actor_id,
                    generation(&actor)?,
                    &request_id,
                    allow,
                )
                .map_err(io_failure)?;
                Ok(Value::Null)
            }
            Mutation::Interaction {
                actor,
                request_id,
                reply,
            } => {
                local_headless::respond_interaction(
                    &actor.group_id,
                    &actor.actor_id,
                    generation(&actor)?,
                    &request_id,
                    reply,
                )
                .map_err(io_failure)?;
                Ok(Value::Null)
            }
            Mutation::CancelTurn { actor } => {
                local_headless::cancel_turn(&actor.group_id, &actor.actor_id, generation(&actor)?)
                    .map(json)
                    .map_err(io_failure)
            }
            Mutation::EnsureViewer { actor } => {
                local_headless::ensure_viewer(&actor.group_id, &actor.actor_id)
                    .map_err(io_failure)?;
                Ok(Value::Null)
            }
            Mutation::DetachViewer { actor } => {
                local_headless::detach_after_viewer_exit(&actor.group_id, &actor.actor_id)
                    .map(json)
                    .map_err(io_failure)
            }
            Mutation::PauseConfirmation { actor } => {
                local_headless::pause_confirmation(&actor.group_id, &actor.actor_id)
                    .map_err(io_failure)?;
                Ok(Value::Null)
            }
            Mutation::RetryConfirmation { actor } => {
                local_headless::retry_confirmation(&actor.group_id, &actor.actor_id)
                    .map_err(io_failure)?;
                Ok(Value::Null)
            }
            Mutation::Terminal { actor, action } => {
                let group = &actor.group_id;
                let id = &actor.actor_id;
                match action {
                    TerminalAction::Write { data } => {
                        cccc_runtime::write(group, id, &data).map(|()| Value::Null)
                    }
                    TerminalAction::Resize { cols, rows } => {
                        cccc_runtime::resize(group, id, cols, rows).map(|()| Value::Null)
                    }
                    TerminalAction::ResizeAttachment {
                        attachment_id,
                        cols,
                        rows,
                    } => cccc_runtime::resize_from_attachment(group, id, attachment_id, cols, rows)
                        .map(json),
                    TerminalAction::Clear => cccc_runtime::clear(group, id).map(|()| Value::Null),
                }
                .map_err(runtime_failure)
            }
            Mutation::Acknowledge { .. } => Err(Failure::new(
                "invalid_operation",
                "acknowledgement is handled at admission",
            )),
            Mutation::Recovery { .. } | Mutation::Retire => Err(Failure::new(
                "invalid_operation",
                "retirement is handled at admission",
            )),
        }
    }

    fn stop(&self, actor: &ActorRef) -> Result<(), Failure> {
        if let Some(cancelled) = self
            .cancelled
            .lock()
            .map_err(|_| poisoned())?
            .get(&key(actor))
        {
            cancelled.store(true, Ordering::Release);
        }
        local_headless::stop(&actor.group_id, &actor.actor_id).map_err(io_failure)?;
        deepseek_runtime::stop_confirmed(&actor.group_id, &actor.actor_id).map_err(io_failure)?;
        match cccc_runtime::stop(&actor.group_id, &actor.actor_id) {
            Ok(_) | Err(cccc_runtime::RuntimeError::NotFound(_, _)) => {}
            Err(error) => return Err(runtime_failure(error)),
        }
        if self
            .detached_work
            .lock()
            .map_err(|_| poisoned())?
            .contains_key(&key(actor))
        {
            return Err(Failure::new(
                "detached_work_unconfirmed",
                "the native background job has not been confirmed absent; a missing session registration is not proof of removal",
            ));
        }
        self.forget_member(actor)
    }

    fn forget_member(&self, actor: &ActorRef) -> Result<(), Failure> {
        self.mcp.lock().map_err(|_| poisoned())?.forget(actor);
        self.actors
            .lock()
            .map_err(|_| poisoned())?
            .remove(&key(actor));
        self.cancelled
            .lock()
            .map_err(|_| poisoned())?
            .remove(&key(actor));
        self.environments
            .lock()
            .map_err(|_| poisoned())?
            .remove(&key(actor));
        self.preambles
            .lock()
            .map_err(|_| poisoned())?
            .remove(&key(actor));
        self.provider_exits
            .lock()
            .map_err(|_| poisoned())?
            .remove(&key(actor));
        Ok(())
    }

    /// Review a failed launch without terminating healthy members sharing this
    /// owner. This process independently proves there is no active daemon.
    pub(super) fn recover(
        &self,
        home: &HomeLayout,
        owner_id: &str,
        action: super::RecoveryAction,
    ) -> Result<Value, Failure> {
        use super::RecoveryAction;
        use cccc_runtime::process_recovery;
        let _home_lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
            .map_err(io_failure)?
            .ok_or_else(|| {
                Failure::new(
                    "daemon_running",
                    "first run `cccc daemon stop --keep-agents`, then review the session owner",
                )
            })?;
        let paths = super::transport::Paths::new(home, owner_id).map_err(io_failure)?;
        let ledger = paths.directory.join("owned-process-groups.json");
        let mut accepted = None;
        match action {
            RecoveryAction::List => {}
            RecoveryAction::Retry { id, version } => {
                self.recovery_member(&id)?;
                process_recovery::retry_active(&ledger, &id, &version)
                    .map_err(|error| Failure::new("recovery_changed", error.to_string()))?;
            }
            RecoveryAction::Accept { id, version } => {
                let member = self.recovery_member(&id)?;
                let mut work = self.detached_work.lock().map_err(|_| poisoned())?;
                if work
                    .get(&key(&member))
                    .and_then(|lease| lease.recovery_id())
                    .as_deref()
                    != Some(id.as_str())
                {
                    return Err(Failure::new(
                        "recovery_changed",
                        "launch evidence changed; list it again",
                    ));
                }
                process_recovery::accept_active(&ledger, &id, &version)
                    .map_err(|error| Failure::new("recovery_changed", error.to_string()))?;
                // Acceptance releases the registration, not the external job.
                // Do not call confirm_stopped or report a confirmed stop.
                work.remove(&key(&member));
                drop(work);
                self.forget_member(&member)?;
                accepted = Some(member);
            }
        }
        let snapshot = process_recovery::list_active(&ledger)
            .map_err(|error| Failure::new("recovery_unavailable", error.to_string()))?;
        let records = snapshot
            .records
            .into_iter()
            .map(|record| {
                let member = self.recovery_member(&record.id).ok();
                let mut value = json!(record);
                value["acceptance_supported"] = json!(member.is_some());
                if let Some(member) = member {
                    value["member"] = json!(member);
                }
                value
            })
            .collect::<Vec<_>>();
        Ok(
            json!({"owner":"session", "owner_id":owner_id, "records":records,
            "diagnostics":snapshot.diagnostics, "retired":false,
            "released_after_risk_acceptance":accepted}),
        )
    }

    fn recovery_member(&self, id: &str) -> Result<ActorRef, Failure> {
        let member_key = self.detached_work.lock().map_err(|_| poisoned())?.iter()
            .find_map(|(key, lease)| (lease.recovery_id().as_deref() == Some(id)).then(|| key.clone()))
            .ok_or_else(|| Failure::new("recovery_not_supported", "only an uncertain startup without a registered native session can be released here"))?;
        if self
            .starting
            .lock()
            .map_err(|_| poisoned())?
            .contains(&member_key)
        {
            return Err(Failure::new(
                "actor_starting",
                "wait for the native launch to finish before review",
            ));
        }
        let actor = self
            .actors
            .lock()
            .map_err(|_| poisoned())?
            .get(&member_key)
            .cloned()
            .ok_or_else(|| Failure::new("recovery_changed", "member registration changed"))?;
        let observed = snapshot(&actor)?;
        if observed.owner.is_some()
            || observed.terminal.is_some()
            || observed.workspace.is_some()
            || observed.confirmation_terminal
            || observed.waiting_for_workspace_trust
        {
            return Err(Failure::new(
                "native_session_registered",
                "a native session or terminal still owns this member; use its normal Stop and confirm removal",
            ));
        }
        Ok(actor)
    }

    pub(super) fn record_provider_exit(&self, group: &str, actor: &str, generation: &str) {
        // The reader calls this only after the same native Stop confirmed
        // removal; observer disappearance alone never reaches this path.
        if let Err(error) = self.clear_detached_work(&(group.into(), actor.into())) {
            tracing::warn!(kind = ?error.kind(), "confirmed provider exit still has pending ownership evidence");
        }
        if let Ok(mut exits) = self.provider_exits.lock() {
            exits.insert((group.into(), actor.into()), generation.into());
        }
    }

    pub(super) fn protect_detached_work(&self, group: &str, actor: &str) -> std::io::Result<()> {
        let mut work = self
            .detached_work
            .lock()
            .map_err(|_| std::io::Error::other("detached work registry lock poisoned"))?;
        let actor = (group.into(), actor.into());
        if let std::collections::btree_map::Entry::Vacant(entry) = work.entry(actor) {
            entry.insert(cccc_runtime::protect_detached_work()?);
        }
        Ok(())
    }

    pub(super) fn clear_detached_work(&self, actor: &Key) -> std::io::Result<()> {
        let mut work = self
            .detached_work
            .lock()
            .map_err(|_| std::io::Error::other("detached work registry lock poisoned"))?;
        if let Some(evidence) = work.get_mut(actor) {
            evidence.confirm_stopped()?;
        }
        work.remove(actor);
        Ok(())
    }

    pub(super) fn is_empty(&self) -> Result<bool, Failure> {
        Ok(self.actors.lock().map_err(|_| poisoned())?.is_empty() && self.browsers.is_empty()?)
    }

    pub(super) fn accepting_starts(&self) -> bool {
        !self.stopping.load(Ordering::Acquire)
    }

    pub(super) fn retire(&self) -> Result<(), Failure> {
        if !self.is_empty()? {
            return Err(Failure::new(
                "owner_not_empty",
                "owner still holds member resources",
            ));
        }
        self.stopping.store(true, Ordering::Release);
        Ok(())
    }

    pub(super) fn environment(&self, group: &str, actor: &str) -> Option<BTreeMap<String, String>> {
        self.environments
            .lock()
            .ok()?
            .get(&(group.into(), actor.into()))
            .cloned()
    }

    pub(super) fn read_terminal(
        &self,
        actor: &ActorRef,
        read: TerminalRead,
    ) -> Result<Value, Failure> {
        let registered = self
            .actors
            .lock()
            .map_err(|_| poisoned())?
            .get(&key(actor))
            .cloned()
            .ok_or_else(|| Failure::new("not_found", "member is no longer registered"))?;
        if registered.actor_created_at != actor.actor_created_at
            || snapshot(&registered)?.actor.generation != actor.generation
        {
            return Err(Failure::new(
                "session_replaced",
                "terminal instance changed",
            ));
        }
        let group = &actor.group_id;
        let id = &actor.actor_id;
        let value = match read {
            TerminalRead::History { before, limit } => {
                cccc_runtime::history(group, id, before, limit).map(json)
            }
            TerminalRead::Retained => cccc_runtime::retained_history(group, id).map(json),
            TerminalRead::RetainedTail { limit } => {
                cccc_runtime::retained_history_tail(group, id, limit).map(json)
            }
            TerminalRead::Since { after, limit } => {
                cccc_runtime::history_since(group, id, after, limit).map(json)
            }
            TerminalRead::Replay { after, end, limit } => {
                cccc_runtime::active_history_replay(group, id, after, end, limit).map(json)
            }
            TerminalRead::LastOutput => cccc_runtime::last_output(group, id).map(json),
            TerminalRead::BracketedPaste => {
                cccc_runtime::bracketed_paste_enabled(group, id).map(json)
            }
            TerminalRead::Writable { attachment_id } => {
                cccc_runtime::attachment_writable(group, id, attachment_id).map(json)
            }
        };
        value.map_err(runtime_failure)
    }
}

struct StartCompletion<'a> {
    backend: &'a Backend,
    key: Key,
}

struct StopCompletion<'a> {
    backend: &'a Backend,
    key: Option<Key>,
}
impl Drop for StopCompletion<'_> {
    fn drop(&mut self) {
        if let Some(key) = &self.key {
            if let Ok(mut pending) = self.backend.stop_pending.lock() {
                pending.remove(key);
            }
        } else {
            self.backend
                .stop_all_pending
                .store(false, Ordering::Release);
        }
    }
}
impl Drop for StartCompletion<'_> {
    fn drop(&mut self) {
        if let Ok(mut starting) = self.backend.starting.lock() {
            starting.remove(&self.key);
            self.backend.started.notify_all();
        }
    }
}

fn snapshot(actor: &ActorRef) -> Result<ActorSnapshot, Failure> {
    let group = &actor.group_id;
    let id = &actor.actor_id;
    let owner = actor_runtime::registered_owner(group, id).map_err(io_failure)?;
    let generation = owner.as_ref().and_then(|owner| match &owner.identity {
        actor_runtime::OwnerIdentity::Managed { generation } => generation.clone(),
        actor_runtime::OwnerIdentity::DeepSeek { generation } => Some(generation.clone()),
        actor_runtime::OwnerIdentity::Pty { started_at, .. } => Some(started_at.clone()),
    });
    let terminal = match cccc_runtime::status(group, id) {
        Ok(status) => Some(status),
        Err(cccc_runtime::RuntimeError::NotFound(_, _)) => None,
        Err(error) => return Err(runtime_failure(error)),
    };
    let workspace = local_headless::registered_workspace(group, id)
        .or_else(|| deepseek_runtime::registered_workspace(group, id))
        .map(|(_, workspace)| workspace);
    Ok(ActorSnapshot {
        actor: ActorRef {
            generation,
            ..actor.clone()
        },
        observation_error: None,
        mcp: None,
        owner,
        terminal_launch: terminal.as_ref().and_then(|terminal| terminal.launch),
        terminal,
        managed_status: local_headless::status(group, id),
        provider_exited: false,
        workspace,
        structured: local_headless::structured_state(group, id),
        ready_for_mail_notice: local_headless::ready_for_mail_notice(group, id),
        waiting_for_workspace_trust: local_headless::waiting_for_workspace_trust(group, id)
            .map_err(io_failure)?,
        confirmation_terminal: local_headless::confirmation_terminal_owns(group, id)
            .map_err(io_failure)?,
        confirmation_pending: local_headless::confirmation_pending_active(group, id)
            .map_err(io_failure)?,
        confirmation_paused: local_headless::confirmation_paused(group, id).map_err(io_failure)?,
    })
}

fn unknown_snapshot(actor: &ActorRef, error: Failure) -> ActorSnapshot {
    ActorSnapshot {
        actor: actor.clone(),
        observation_error: Some(error),
        mcp: None,
        owner: None,
        terminal: None,
        terminal_launch: None,
        managed_status: None,
        provider_exited: false,
        workspace: None,
        structured: None,
        ready_for_mail_notice: false,
        waiting_for_workspace_trust: false,
        confirmation_terminal: false,
        confirmation_pending: false,
        confirmation_paused: false,
    }
}

fn key(actor: &ActorRef) -> Key {
    (actor.group_id.clone(), actor.actor_id.clone())
}
fn generation(actor: &ActorRef) -> Result<&str, Failure> {
    actor
        .generation
        .as_deref()
        .ok_or_else(|| Failure::new("session_not_ready", "session has no generation"))
}
fn json(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("serializable operation result")
}
fn poisoned() -> Failure {
    Failure::new("owner_unavailable", "session owner registry lock poisoned")
}
fn io_failure(error: std::io::Error) -> Failure {
    Failure::new("runtime_error", error.to_string())
}
fn runtime_failure(error: cccc_runtime::RuntimeError) -> Failure {
    let code = match &error {
        cccc_runtime::RuntimeError::NotFound(_, _) => "not_found",
        cccc_runtime::RuntimeError::NotRunning(_, _) => "not_running",
        _ => "runtime_error",
    };
    Failure::new(code, error.to_string())
}
fn serialization_failure(error: serde_json::Error) -> Failure {
    Failure::new("owner_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controlled_mcp_provenance_follows_the_selected_launch_method() {
        use cccc_contracts::ActorRuntime;
        let temp = tempfile::tempdir().expect("isolated admission fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group: cccc_core::GroupDoc = serde_json::from_value(json!({
            "v":1, "group_id":"g_launch_provenance", "generation":"resource",
            "title":"launch provenance", "created_at":"created", "updated_at":"created"
        }))
        .expect("group");
        let actor = ActorRef {
            group_id: group.group_id.clone(),
            actor_id: "member".into(),
            actor_created_at: "created".into(),
            generation: None,
        };
        for (runtime, controlled) in [
            (ActorRuntime::Codex, true),
            (ActorRuntime::Claude, true),
            (ActorRuntime::Pi, false),
        ] {
            let backend = Backend::with_owner(home.clone(), "owner".into(), "instance".into());
            let mut configured = cccc_contracts::Actor::new(&actor.actor_id);
            configured.created_at.clone_from(&actor.actor_created_at);
            configured.runtime = runtime;
            backend
                .reserve(&Mutation::Start {
                    actor: actor.clone(),
                    launch: Box::new(PreparedLaunch::Managed {
                        group: group.clone(),
                        actor: configured,
                        host_environment: Default::default(),
                    }),
                })
                .expect("reserve selected managed path");
            let sample = backend
                .mcp
                .lock()
                .expect("registry")
                .snapshot(&actor)
                .expect("reserved MCP source");
            assert_eq!(sample.source_controlled, Some(controlled));
            assert!(!sample.initialized, "admission alone is not a live MCP");
        }
        // A traditional terminal can run the same provider without going
        // through either managed main-MCP injection path.
        let backend = Backend::with_owner(home, "owner".into(), "instance".into());
        backend
            .reserve(&Mutation::Start {
                actor: actor.clone(),
                launch: Box::new(PreparedLaunch::Pty {
                    spec: cccc_runtime::LaunchSpec {
                        group_id: actor.group_id.clone(),
                        actor_id: actor.actor_id.clone(),
                        runner: cccc_contracts::RunnerKind::Pty,
                        command: vec!["claude".into()],
                        cwd: temp.path().into(),
                        env: Default::default(),
                        cols: 80,
                        rows: 24,
                    },
                    history: cccc_runtime::HistoryConfig {
                        path: temp.path().join("history"),
                        max_bytes: 4096,
                        hot_bytes: 4096,
                        persist: false,
                    },
                    launch: cccc_runtime::ActorLaunch {
                        runtime: ActorRuntime::Claude,
                        submit: cccc_contracts::Actor::new("member").submit,
                    },
                    preamble: String::new(),
                }),
            })
            .expect("reserve traditional terminal, without executing it");
        assert!(
            backend
                .mcp
                .lock()
                .expect("registry")
                .snapshot(&actor)
                .is_none()
        );
        assert!(
            backend
                .mcp_binding_file(&actor.group_id, &actor.actor_id)
                .is_none()
        );
    }

    #[test]
    fn live_recovery_releases_only_the_reviewed_unknown_launch() {
        const CHILD: &str = "CCCC_TEST_LIVE_OWNER_RECOVERY_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().expect("resolve test executable"))
                .args(["--exact", "session_owner::backend::tests::live_recovery_releases_only_the_reviewed_unknown_launch", "--nocapture"])
                .env(CHILD, "1").output().expect("run isolated test child");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
        home.initialize().expect("initialize fixture state");
        let owner_id = uuid::Uuid::new_v4().simple().to_string();
        let paths =
            super::super::transport::Paths::new(&home, &owner_id).expect("resolve owner paths");
        paths.initialize().expect("initialize fixture state");
        let ledger = paths.directory.join("owned-process-groups.json");
        cccc_runtime::protect_owned_process_groups(&ledger)
            .expect("enable process ownership tracking");
        let backend = Backend::default();
        let missing = ActorRef {
            group_id: "g_live_review".into(),
            actor_id: "failed".into(),
            actor_created_at: "created".into(),
            generation: None,
        };
        let healthy = ActorRef {
            actor_id: "healthy".into(),
            ..missing.clone()
        };
        cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: healthy.group_id.clone(),
            actor_id: healthy.actor_id.clone(),
            runner: cccc_contracts::RunnerKind::Pty,
            command: vec!["sleep".into(), "30".into()],
            cwd: temp.path().into(),
            env: Default::default(),
            cols: 80,
            rows: 24,
        })
        .expect("start fixture terminal");
        let pid = cccc_runtime::status(&healthy.group_id, &healthy.actor_id)
            .expect("read terminal status")
            .pid;
        for member in [&missing, &healthy] {
            backend
                .actors
                .lock()
                .expect("fixture registry lock")
                .insert(key(member), member.clone());
            backend
                .protect_detached_work(&member.group_id, &member.actor_id)
                .expect("register detached work");
        }
        for lease in backend
            .detached_work
            .lock()
            .expect("fixture registry lock")
            .values()
        {
            lease
                .mark_uncertain()
                .expect("retain uncertain detached work");
        }
        let listed = backend
            .recover(&home, &owner_id, super::super::RecoveryAction::List)
            .expect("review owner recovery records");
        let records = listed["records"].as_array().expect("array result");
        let failed = records
            .iter()
            .find(|row| row["member"]["actor_id"] == "failed")
            .expect("matching recovery record");
        assert_eq!(
            records
                .iter()
                .filter(|row| row["acceptance_supported"] == true)
                .count(),
            1,
            "the healthy PTY cannot be released by accepting a lease"
        );
        let id = failed["id"].as_str().expect("string result").to_owned();
        let version = failed["version"]
            .as_str()
            .expect("string result")
            .to_owned();
        let lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
            .expect("check fixture lock")
            .expect("home lock is available");
        assert!(
            backend
                .recover(
                    &home,
                    &owner_id,
                    super::super::RecoveryAction::Accept {
                        id: id.clone(),
                        version: version.clone()
                    }
                )
                .is_err()
        );
        drop(lock);
        assert!(
            backend
                .recover(
                    &home,
                    &owner_id,
                    super::super::RecoveryAction::Accept {
                        id: id.clone(),
                        version: "stale".into()
                    }
                )
                .is_err()
        );
        backend
            .recover(
                &home,
                &owner_id,
                super::super::RecoveryAction::Accept { id, version },
            )
            .expect("review owner recovery records");
        assert!(
            !backend
                .actors
                .lock()
                .expect("fixture registry lock")
                .contains_key(&key(&missing))
        );
        assert!(
            backend
                .actors
                .lock()
                .expect("fixture registry lock")
                .contains_key(&key(&healthy))
        );
        assert_eq!(
            cccc_runtime::status(&healthy.group_id, &healthy.actor_id)
                .expect("read terminal status")
                .pid,
            pid
        );
        cccc_runtime::stop(&healthy.group_id, &healthy.actor_id).expect("stop fixture terminal");
        backend
            .clear_detached_work(&key(&healthy))
            .expect("clear confirmed detached-work lease");
    }

    #[test]
    fn failed_launch_with_confirmed_cleanup_does_not_block_a_later_start() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
        let actor = ActorRef {
            group_id: "g_failed_owner_start".into(),
            actor_id: "peer".into(),
            actor_created_at: "created".into(),
            generation: None,
        };
        let start = || Mutation::Start {
            actor: actor.clone(),
            launch: Box::new(PreparedLaunch::Pty {
                spec: cccc_runtime::LaunchSpec {
                    group_id: actor.group_id.clone(),
                    actor_id: actor.actor_id.clone(),
                    runner: cccc_contracts::RunnerKind::Pty,
                    command: vec![
                        temp.path()
                            .join("missing-command")
                            .to_string_lossy()
                            .into_owned(),
                    ],
                    cwd: temp.path().into(),
                    env: Default::default(),
                    cols: 80,
                    rows: 24,
                },
                history: cccc_runtime::HistoryConfig {
                    path: temp.path().join("history"),
                    max_bytes: 4096,
                    hot_bytes: 4096,
                    persist: false,
                },
                launch: cccc_runtime::ActorLaunch {
                    runtime: cccc_contracts::ActorRuntime::Custom,
                    submit: cccc_contracts::Actor::new("peer").submit,
                },
                preamble: String::new(),
            }),
        };
        let backend = Backend::default();
        backend.validate(&start()).expect("validate owner mutation");
        backend.reserve(&start()).expect("reserve member operation");
        assert!(backend.execute(&home, start()).is_err());
        assert!(backend.is_empty().expect("check owner registry"));
        backend.validate(&start()).expect("validate owner mutation");
    }

    #[test]
    fn duplicate_stops_do_not_take_more_reserved_slots() {
        let backend = Backend::default();
        let actor = ActorRef {
            group_id: "g_stop_reservation".into(),
            actor_id: "peer".into(),
            actor_created_at: "created".into(),
            generation: None,
        };
        backend
            .actors
            .lock()
            .expect("fixture registry lock")
            .insert(key(&actor), actor.clone());
        backend
            .cancelled
            .lock()
            .expect("fixture registry lock")
            .insert(key(&actor), Arc::new(AtomicBool::new(false)));
        let stop = Mutation::Stop {
            actor: actor.clone(),
        };
        backend.validate(&stop).expect("validate owner mutation");
        backend.reserve(&stop).expect("reserve member operation");
        assert_eq!(
            backend
                .validate(&stop)
                .expect_err("conflicting mutation must be rejected")
                .code,
            "already_stopping"
        );
        backend
            .validate(&Mutation::StopAll)
            .expect("validate owner mutation");
        backend
            .reserve(&Mutation::StopAll)
            .expect("reserve member operation");
        assert_eq!(
            backend
                .validate(&Mutation::StopAll)
                .expect_err("conflicting mutation must be rejected")
                .code,
            "already_stopping"
        );
    }
}
