//! One on-demand provider session. Task authority exists only during one turn.
use super::*;

pub(super) struct Resident {
    pub session: Arc<AnalystSession>,
    pub token: String,
    pub fingerprint: String,
    pub available: AtomicBool,
    pub terminal_gate: Mutex<()>,
    pub last: Mutex<Option<Arc<Execution>>>,
    turns: Mutex<std::collections::BTreeSet<String>>,
    observer: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Resident {
    pub(super) fn busy(&self) -> bool {
        !self
            .turns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    fn observe(&self, event: &super::super::codex_voice_analyst::AnalystEvent) -> bool {
        if event.generation != self.session.generation()
            || event.message["params"]["threadId"]
                .as_str()
                .is_some_and(|id| id != self.session.thread_id())
        {
            return false;
        }
        let method = event.message["method"].as_str();
        if method == Some(super::super::codex_voice_analyst::MANAGED_AGENT_DISCONNECTED_METHOD) {
            self.available.store(false, Ordering::Release);
            return true;
        }
        let Some(id) = event.message["params"]["turn"]["id"]
            .as_str()
            .filter(|id| !id.is_empty())
        else {
            return false;
        };
        let mut turns = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        match method {
            Some("turn/started") => turns.insert(id.to_owned()),
            Some("turn/completed") => turns.remove(id),
            _ => false,
        }
    }
}

impl Drop for Resident {
    fn drop(&mut self) {
        if let Some(observer) = self
            .observer
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            observer.abort();
        }
    }
}

impl Manager {
    pub(super) fn resident(&self) -> Option<Arc<Resident>> {
        self.resident
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(super) async fn stop_resident(&self) -> io::Result<()> {
        if let Some(resident) = self.resident() {
            resident.available.store(false, Ordering::Release);
            let terminal = {
                let _gate = resident
                    .terminal_gate
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                terminal::stop_generation(resident.session.generation())
            };
            resident.session.stop(resident.session.generation()).await?;
            terminal?;
            let observer = resident
                .observer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            if let Some(observer) = observer {
                observer.abort();
                let _ = observer.await;
            }
        }
        super::super::codex_voice_analyst::cleanup_secretary_runtime(
            &self.home,
            &self.store.runtime_workspace(),
        )
        .await?;
        revoke_session_grant(&self.store)?;
        self.resident
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        Ok(())
    }

    /// Only the serial owner calls this, after the previous turn is settled.
    pub(super) async fn session_for(
        &self,
        config: &ResolvedAgentRuntime,
        cancellation: watch::Receiver<bool>,
        token: &str,
    ) -> io::Result<Arc<Resident>> {
        let fingerprint = digest(&config.fingerprint());
        if let Some(resident) = self.resident()
            && resident.available.load(Ordering::Acquire)
            && resident.fingerprint == fingerprint
            && resident.session.process_running()
        {
            return Ok(resident);
        }
        self.stop_resident().await?;
        std::fs::create_dir_all(self.store.runtime_workspace())?;
        let session = Arc::new(
            AnalystSession::launch_secretary(
                &self.home,
                LaunchConfig {
                    workdir: self.store.runtime_workspace(),
                    runtime: config.runtime,
                    runtime_mode: config.runtime_mode,
                    command: config.command.clone(),
                    environment: config.environment.clone(),
                    resume_thread_id: None,
                },
                cancellation,
                token,
            )
            .await?,
        );
        // Subscribe before exposing the terminal. Native/manual turns must be
        // observed even while no Secretary task or browser viewer is active.
        let mut events = session.subscribe();
        let resident = Arc::new(Resident {
            session,
            token: token.into(),
            fingerprint,
            available: AtomicBool::new(true),
            terminal_gate: Mutex::new(()),
            last: Mutex::new(None),
            turns: Mutex::new(Default::default()),
            observer: Mutex::new(None),
        });
        let weak = Arc::downgrade(&resident);
        let home = self.home.clone();
        let observer = managed_runtime().spawn(async move {
            loop {
                let event = events.recv().await;
                let Some(resident) = weak.upgrade() else {
                    break;
                };
                let changed = match event {
                    Ok(event) => resident.observe(&event),
                    Err(_) => {
                        // Lost lifecycle events cannot be replaced with an idle guess.
                        resident.available.store(false, Ordering::Release);
                        if let Some(owner) = lookup(&home) {
                            owner.notify();
                        }
                        break;
                    }
                };
                if changed && let Some(owner) = lookup(&home) {
                    owner.notify();
                }
            }
        });
        *resident.observer.lock().unwrap_or_else(|e| e.into_inner()) = Some(observer);
        *self.resident.lock().unwrap_or_else(|e| e.into_inner()) = Some(resident.clone());
        Ok(resident)
    }

    pub(super) async fn cleanup_execution(
        &self,
        execution: &Execution,
        keep: bool,
    ) -> io::Result<()> {
        if keep
            && self
                .resident()
                .is_some_and(|r| r.available.load(Ordering::Acquire) && r.session.process_running())
        {
            return Ok(());
        }
        let startup = execution
            .startup_cleanup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(startup) = startup {
            startup.stop()?;
        }
        self.stop_resident().await
    }

    /// No automatic launch/replay. A stale or explicitly reset resident is only stopped.
    pub(super) async fn maintain_resident(&self) -> io::Result<()> {
        let reset = self.reset_requested.load(Ordering::Acquire);
        if self.resident().is_none()
            && super::super::codex_voice_analyst::secretary_cleanup_pending(
                &self.store.runtime_workspace(),
            )
        {
            self.stop_resident().await?;
            if !cccc_runtime::owned_process_recovery_pending() {
                for task in self.store.unfinished()? {
                    if !task.phase.executing()
                        && !super::super::codex_voice_analyst::secretary_cleanup_pending(
                            &self.store.directory(&task.task_id)?.join("workspace"),
                        )
                    {
                        self.store.update(&task.task_id, |t| {
                            t.cleanup_confirmed = true;
                            Ok(())
                        })?;
                    }
                }
            }
        }
        if let Some(resident) = self.resident() {
            let stale = {
                let _permit = if let Some(locks) = &self.locks {
                    Some(locks.global_read().await)
                } else {
                    None
                };
                resolve_runtime(&self.home).map_or(true, |config| {
                    digest(&config.fingerprint()) != resident.fingerprint
                })
            };
            if reset
                || stale
                || !resident.available.load(Ordering::Acquire)
                || !resident.session.process_running()
            {
                self.stop_resident().await?;
            }
        }
        if reset {
            self.reset_requested.store(false, Ordering::Release);
        }
        let pending = self
            .grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, e)| *e.cancel.borrow())
            .map(|(key, e)| (key.clone(), e.clone()))
            .collect::<Vec<_>>();
        for (key, execution) in pending {
            self.cleanup_execution(&execution, false).await?;
            self.store.update(&execution.task_id, |task| {
                task.cleanup_confirmed = true;
                Ok(())
            })?;
            self.grants
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
            self.project(&self.store.load(&execution.task_id)?).await;
        }
        Ok(())
    }
}

pub(super) fn revoke_session_grant(store: &SecretaryTaskStore) -> io::Result<()> {
    match std::fs::remove_file(store.session_grant_file()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    configuration::require_user(request)?;
    let Some(owner) = lookup(home) else {
        return object(json!({"phase":"unavailable","diagnostic":owner_readiness_error(home)}));
    };
    let Some(resident) = owner.resident() else {
        let starting = owner
            .grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .find(|e| !*e.cancel.borrow())
            .and_then(|e| owner.store.load(&e.task_id).ok());
        return object(
            json!({"phase":if starting.is_some() {"starting"} else {"not_started"},
            "task":starting.as_ref().map(projection),"diagnostic":configuration::readiness_error(home)}),
        );
    };
    let execution = resident
        .last
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let task = execution
        .as_ref()
        .and_then(|e| owner.store.load(&e.task_id).ok());
    let task_busy = execution.as_ref().is_some_and(|e| !*e.cancel.borrow());
    let manual_turn = resident.busy() && !task_busy;
    let busy = task_busy || manual_turn;
    let phase = if owner.reset_requested.load(Ordering::Acquire)
        || !resident.available.load(Ordering::Acquire)
    {
        "stopping"
    } else if !resident.session.process_running() {
        "disconnected"
    } else if busy {
        "working"
    } else {
        "ready"
    };
    object(
        json!({"phase":phase,"generation":resident.session.generation(),"runtime":resident.session.runtime(),
        "native_terminal":resident.session.tui_ready(),"manual_turn":manual_turn,
        "task":task.as_ref().filter(|_|!manual_turn).map(projection),
        "group_title":task.as_ref().filter(|_|!manual_turn).and_then(|t| cccc_core::GroupStore::new(home.clone()).ok()?.load(&t.target.group_id).ok()).map(|g|g.title),
        "progress":execution.as_ref().filter(|_|!manual_turn).map(|e|e.progress.lock().unwrap_or_else(|e| e.into_inner()).snapshot()),
        "activity":execution.as_ref().filter(|_|task_busy).map(|e|e.activity.lock().unwrap_or_else(|e| e.into_inner()).clone())}),
    )
}

pub(super) fn reset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    configuration::require_user(request)?;
    let owner = lookup(home)
        .ok_or_else(|| OpError::new("unavailable", "Secretary owner is unavailable"))?;
    // Serialize with admission. Reset never interrupts somebody else's task.
    let _acceptance = owner.acceptance.lock().unwrap_or_else(|e| e.into_inner());
    if !owner
        .grants
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
    {
        return Err(OpError::new(
            "secretary_busy",
            "Wait for the current task or cancel it before starting a new session",
        ));
    }
    let resident = owner
        .resident()
        .ok_or_else(|| OpError::new("unavailable", "Secretary is not started"))?;
    if required_arg(request, "generation")? != resident.session.generation() {
        return Err(OpError::new(
            "stale_generation",
            "Secretary session has changed",
        ));
    }
    if resident.busy() {
        return Err(OpError::new(
            "secretary_busy",
            "Wait for or interrupt the terminal turn before starting a new session",
        ));
    }
    owner.reset_requested.store(true, Ordering::Release);
    resident.available.store(false, Ordering::Release);
    owner.notify();
    object(json!({"resetting":true}))
}
