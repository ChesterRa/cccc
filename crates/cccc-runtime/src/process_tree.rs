//! Process resources reachable independently of session/protocol shutdown locks.
#[cfg(unix)]
pub(crate) mod guard;
#[cfg(unix)]
mod guard_record;
#[cfg(target_os = "linux")]
mod linux_identity;
mod resource;
mod spawn;
use resource::Resource;
use std::collections::HashMap;
use std::io;
use std::process::{Child, Command};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Evidence from the actual process-creation boundary, not from an error kind.
/// `Started` does not assert submission: child code may have run, so callers
/// must not infer that retrying a side-effecting command is safe.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    NotStarted(io::Error),
    #[error(transparent)]
    Started(io::Error),
}

/// A process may have executed before its supervisor could register it.
/// The underlying OS error is retained as the source, but its kind must not be
/// mistaken for proof that no child ran.
#[derive(Debug, thiserror::Error)]
#[error("the process may have started before supervision was established: {0}")]
pub struct ProcessStartUnconfirmed(#[source] io::Error);

fn unconfirmed_start(error: io::Error) -> io::Error {
    io::Error::other(ProcessStartUnconfirmed(error))
}

impl CommandError {
    /// Return the I/O error carried by the start-evidence variant. An
    /// unconfirmed registration is deliberately wrapped as `Other`; its
    /// underlying OS error remains available through the error source.
    pub fn into_io_error(self) -> io::Error {
        match self {
            Self::NotStarted(error) | Self::Started(error) => error,
        }
    }
}

#[derive(Default)]
struct Registry {
    closed: bool,
    next_id: u64,
    resources: HashMap<u64, Resource>,
}

fn registry() -> MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Owns a process tree until it is terminated, independently of its session.
/// All child polling must go through `try_wait`, and termination must precede
/// blocking waits. This keeps Unix group leaders unreaped while registered.
/// Unix embedding hosts must also avoid automatic or wildcard child reaping for
/// the lifetime of registered children: an external reaper can invalidate a
/// numeric group identity between its ownership check and a group signal.
#[derive(Debug)]
pub struct OwnedProcessTree {
    id: u64,
}

impl OwnedProcessTree {
    pub fn spawn(command: &mut Command) -> io::Result<(Child, Self)> {
        Self::spawn_registered(command, 0).map_err(CommandError::into_io_error)
    }

    /// Supply Windows flags explicitly because std::Command has no flag getter.
    #[cfg(windows)]
    pub fn spawn_with_creation_flags(
        command: &mut Command,
        flags: u32,
    ) -> io::Result<(Child, Self)> {
        Self::spawn_registered(command, flags).map_err(CommandError::into_io_error)
    }

    pub(crate) fn spawn_registered(
        command: &mut Command,
        flags: u32,
    ) -> Result<(Child, Self), CommandError> {
        let mut registry = registry();
        registry.require_open().map_err(CommandError::NotStarted)?;
        #[cfg(unix)]
        let intent = guard::begin_intent().map_err(CommandError::NotStarted)?;
        // Serialize admission with force termination. Windows additionally
        // assigns the Job before resuming the child; the mutex alone cannot
        // prevent child code from executing during registration.
        let (child, resource) = match spawn::standard(command, flags) {
            Ok(started) => started,
            Err(error) => {
                #[cfg(unix)]
                if let Some(intent) = &intent {
                    let retained = if matches!(&error, CommandError::NotStarted(_)) {
                        guard::cancel_unstarted_intent(intent.entry.id)
                            .or_else(|_| guard::retain_uncertain_intent(intent.entry.id))
                    } else {
                        guard::retain_uncertain_intent(intent.entry.id)
                    };
                    if let Err(cleanup_error) = retained {
                        crate::diagnostic(format_args!(
                            "could not persist process launch outcome: {cleanup_error}"
                        ));
                    }
                }
                return Err(error);
            }
        };
        #[cfg(unix)]
        return Self::finish_started(child, resource, &mut registry, intent);
        #[cfg(windows)]
        {
            let owner = registry.insert(resource);
            Ok((child, owner))
        }
    }

    #[cfg(unix)]
    fn finish_started(
        mut child: Child,
        resource: Resource,
        registry: &mut Registry,
        intent: Option<guard::PreparedIntent>,
    ) -> Result<(Child, Self), CommandError> {
        match registry.insert_guarded(resource, intent) {
            Ok(owner) => Ok((child, owner)),
            Err((error, id, intent_id)) => {
                if let Some(intent_id) = intent_id
                    && let Err(retain_error) = guard::retain_uncertain_intent(intent_id)
                {
                    crate::diagnostic(format_args!(
                        "could not persist process launch outcome: {retain_error}"
                    ));
                }
                if registry.terminate(id).is_ok() {
                    // Killing the original group does not imply that its
                    // leader is still in it. Stop the unreaped child itself
                    // before waiting, and do not wait if that signal fails.
                    spawn::stop_owned_child(&mut child);
                }
                Err(CommandError::Started(unconfirmed_start(error)))
            }
        }
    }

    pub(crate) fn spawn_pty(
        spawn: impl FnOnce() -> Result<Box<dyn portable_pty::Child + Send + Sync>, CommandError>,
    ) -> io::Result<(Box<dyn portable_pty::Child + Send + Sync>, Self)> {
        let mut registry = registry();
        registry.require_open()?;
        #[cfg(unix)]
        let intent = guard::begin_intent()?;
        let mut child = match spawn() {
            Ok(child) => child,
            Err(error) => {
                #[cfg(unix)]
                if let Some(intent) = &intent {
                    let outcome = if matches!(&error, CommandError::NotStarted(_)) {
                        guard::cancel_unstarted_intent(intent.entry.id)
                            .or_else(|_| guard::retain_uncertain_intent(intent.entry.id))
                    } else {
                        guard::retain_uncertain_intent(intent.entry.id)
                    };
                    if let Err(retain_error) = outcome {
                        crate::diagnostic(format_args!(
                            "could not persist PTY launch outcome: {retain_error}"
                        ));
                    }
                }
                return Err(match error {
                    CommandError::NotStarted(error) => error,
                    CommandError::Started(error) => unconfirmed_start(error),
                });
            }
        };
        let resource = match Resource::pty(child.as_ref()) {
            Ok(resource) => resource,
            Err(error) => {
                #[cfg(unix)]
                if let Some(intent) = &intent
                    && let Err(retain_error) = guard::retain_uncertain_intent(intent.entry.id)
                {
                    crate::diagnostic(format_args!(
                        "could not persist PTY launch outcome: {retain_error}"
                    ));
                }
                let stopped = child.kill().is_ok();
                drop(registry);
                if stopped {
                    let _ = child.wait();
                } else {
                    // A failed signal is not evidence that the child exited;
                    // do not block forever waiting for it on this error path.
                    let _ = child.try_wait();
                }
                return Err(unconfirmed_start(error));
            }
        };
        #[cfg(unix)]
        let owner = match registry.insert_guarded(resource, intent) {
            Ok(owner) => owner,
            Err((error, id, intent_id)) => {
                if let Some(intent_id) = intent_id
                    && let Err(retain_error) = guard::retain_uncertain_intent(intent_id)
                {
                    crate::diagnostic(format_args!(
                        "could not persist PTY launch outcome: {retain_error}"
                    ));
                }
                if registry.terminate(id).is_ok() {
                    let _ = child.wait();
                }
                return Err(unconfirmed_start(error));
            }
        };
        #[cfg(windows)]
        let owner = registry.insert(resource);
        Ok((child, owner))
    }

    /// Nonblocking exit polling, with identity revocation before reaping.
    pub fn try_wait<T>(
        &self,
        poll: impl FnOnce() -> io::Result<Option<T>>,
    ) -> io::Result<Option<T>> {
        let mut registry = registry();
        #[cfg(unix)]
        if let Some(resource) = registry.resources.get(&self.id) {
            match resource.exited() {
                Ok(false) => return Ok(None),
                Ok(true) => registry.terminate(self.id)?,
                Err(error) => {
                    // An external reaper invalidated our ownership. Never
                    // signal a potentially reused numeric process identity.
                    if error.raw_os_error() == Some(nix::libc::ECHILD) {
                        registry.resources.remove(&self.id);
                        registry.publish();
                    }
                    return Err(error);
                }
            }
        }
        // On Windows the Job handle remains stable across reaping. On Unix
        // the resource has already been revoked before poll can reap the PID.
        let result = poll()?;
        if result.is_some() {
            registry.terminate(self.id)?;
        }
        Ok(result)
    }

    /// Issue termination only; never wait for readers, protocols or processes.
    pub fn terminate(&self) -> io::Result<()> {
        registry().terminate(self.id)
    }

    /// Identity of a currently registered Unix group, for safe diagnostics.
    /// The registration can disappear before a later operation; this does not
    /// itself authorize signalling a numeric PID.
    #[cfg(unix)]
    pub fn registered_group_identity(&self) -> Option<(i32, u64)> {
        registry().resources.get(&self.id).map(|resource| {
            let group = resource.owned_group();
            (group.pgid, group.spawned_at)
        })
    }

    pub fn request_stop(&self) -> io::Result<()> {
        let mut registry = registry();
        if let Some(resource) = registry.resources.get_mut(&self.id) {
            resource.request_stop()?;
        }
        Ok(())
    }
}

impl Drop for OwnedProcessTree {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

impl Registry {
    fn require_open(&self) -> io::Result<()> {
        if self.closed {
            Err(io::Error::other("process owner is shutting down"))
        } else {
            Ok(())
        }
    }

    #[cfg(windows)]
    fn insert(&mut self, resource: Resource) -> OwnedProcessTree {
        self.next_id += 1;
        self.resources.insert(self.next_id, resource);
        self.publish();
        OwnedProcessTree { id: self.next_id }
    }

    #[cfg(unix)]
    fn insert_guarded(
        &mut self,
        mut resource: Resource,
        intent: Option<guard::PreparedIntent>,
    ) -> Result<OwnedProcessTree, (io::Error, u64, Option<uuid::Uuid>)> {
        resource.attach_intent(intent.as_ref());
        self.next_id += 1;
        let id = self.next_id;
        self.resources.insert(id, resource);
        let groups = self
            .resources
            .values()
            .map(Resource::owned_group)
            .collect::<Vec<_>>();
        let intent_id = intent.as_ref().map(|intent| intent.entry.id);
        if let Err(error) = guard::publish_started(&groups, intent_id) {
            return Err((error, id, intent_id));
        }
        Ok(OwnedProcessTree { id })
    }

    fn terminate(&mut self, id: u64) -> io::Result<()> {
        if let Some(resource) = self.resources.get_mut(&id) {
            resource.terminate()?;
        }
        if self.resources.remove(&id).is_some() {
            self.publish();
        }
        Ok(())
    }

    /// Keep the abrupt-exit guard's view equal to the owned set. Its failure must
    /// never fail the spawn or termination that changed the set.
    fn publish(&self) {
        #[cfg(unix)]
        {
            let groups = self
                .resources
                .values()
                .map(Resource::owned_group)
                .collect::<Vec<_>>();
            if let Err(error) = guard::publish(&groups) {
                crate::diagnostic(format_args!(
                    "could not record owned process groups: {error}"
                ));
            }
        }
    }
}

/// Make this process's owned process groups end with it, including after SIGKILL
/// or a crash, and terminate groups a previous owner using `ledger` left behind.
/// Call before spawning anything that must not outlive this process.
#[cfg(unix)]
pub fn protect_owned_process_groups(ledger: &std::path::Path) -> io::Result<()> {
    let registry = registry();
    if guard::enable(ledger)? {
        registry.publish();
    }
    Ok(())
}

/// Recover an old Web owner's separate ledger, optionally enabling protection
/// for this process. The caller must hold that Web owner's instance lock.
/// Never replace a different live guard (such as an embedded daemon's guard).
#[cfg(unix)]
pub fn prepare_web_process_guard(ledger: &std::path::Path, activate: bool) -> io::Result<()> {
    // An old, separately owned Web ledger can take time to reconcile. Do not
    // hold the process registry across that work: live Actor/Voice teardown
    // also needs it. The offline lock fences this check against activation.
    if !guard::active_for(ledger) {
        if activate && guard::has_active_guard() {
            return Err(io::Error::other(
                "another owned process guard is active in this process",
            ));
        }
        guard::recover_ledger(ledger)?;
    }
    let registry = registry();
    if guard::active_for(ledger) {
        let groups = registry
            .resources
            .values()
            .map(Resource::owned_group)
            .collect::<Vec<_>>();
        guard::publish(&groups)?;
        drop(registry);
        return guard::refresh_active(ledger);
    }
    if activate && guard::has_active_guard() {
        return Err(io::Error::other(
            "another owned process guard is active in this process",
        ));
    }
    if activate {
        guard::enable(ledger)?;
        let groups = registry
            .resources
            .values()
            .map(Resource::owned_group)
            .collect::<Vec<_>>();
        // Unlike routine registry publication, startup must report a failure
        // before allowing the new Voice owner to launch.
        guard::publish(&groups)?;
        drop(registry);
        guard::refresh_active(ledger)?;
    }
    Ok(())
}

/// A previous owner's groups could not be safely reconciled. Capacity owners
/// must not count those trees as released merely because their client died.
pub fn owned_process_recovery_pending() -> bool {
    #[cfg(unix)]
    {
        guard::recovery_pending()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Permanently close process admission and terminate this process's owned trees.
/// This is a final-exit operation, not a replacement for ordinary actor_stop.
/// The registry lock only protects spawn, nonblocking polling and OS signals;
/// it is never held by normal shutdown while waiting on a session or protocol.
pub fn force_terminate_owned() -> io::Result<()> {
    let mut registry = registry();
    registry.closed = true;
    let ids: Vec<_> = registry.resources.keys().copied().collect();
    let mut errors = Vec::new();
    for id in ids {
        if let Err(error) = registry.terminate(id) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(errors.join("; ")))
    }
}

#[cfg(all(test, unix))]
mod tests;

#[cfg(all(test, windows))]
mod windows_tests;
