//! Process resources reachable independently of session/protocol shutdown locks.
mod resource;
mod spawn;
use resource::Resource;
use std::collections::HashMap;
use std::io;
use std::process::{Child, Command};
use std::sync::{Mutex, MutexGuard, OnceLock};

#[derive(Default)]
struct Registry {
    closed: bool,
    next_id: u64,
    resources: HashMap<u64, Resource>,
    retained_files: HashMap<u64, Vec<std::fs::File>>,
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
#[derive(Debug)]
pub struct OwnedProcessTree {
    id: u64,
}

impl OwnedProcessTree {
    /// 可选资源跟随本次进程身份；停止失败或仅发出信号时不释放。
    pub fn retain_files(&self, files: Vec<std::fs::File>) {
        if !files.is_empty() {
            registry()
                .retained_files
                .entry(self.id)
                .or_default()
                .extend(files);
        }
    }

    /// 仅在拥有者已通过 wait/try_wait 确认退出后调用。
    pub fn release_files_after_exit(&self) {
        registry().retained_files.remove(&self.id);
    }

    pub fn spawn(command: &mut Command) -> io::Result<(Child, Self)> {
        Self::spawn_registered(command, 0)
    }

    /// Supply Windows flags explicitly because std::Command has no flag getter.
    #[cfg(windows)]
    pub fn spawn_with_creation_flags(
        command: &mut Command,
        flags: u32,
    ) -> io::Result<(Child, Self)> {
        Self::spawn_registered(command, flags)
    }

    fn spawn_registered(command: &mut Command, flags: u32) -> io::Result<(Child, Self)> {
        let mut registry = registry();
        registry.require_open()?;
        // Serialize admission with force termination. Windows additionally
        // assigns the Job before resuming the child; the mutex alone cannot
        // prevent child code from executing during registration.
        let (child, resource) = spawn::standard(command, flags)?;
        let owner = registry.insert(resource);
        Ok((child, owner))
    }

    pub(crate) fn spawn_pty(
        spawn: impl FnOnce() -> io::Result<Box<dyn portable_pty::Child + Send + Sync>>,
    ) -> io::Result<(Box<dyn portable_pty::Child + Send + Sync>, Self)> {
        let mut registry = registry();
        registry.require_open()?;
        let mut child = spawn()?;
        let resource = match Resource::pty(child.as_ref()) {
            Ok(resource) => resource,
            Err(error) => {
                let _ = child.kill();
                drop(registry);
                let _ = child.wait();
                return Err(error);
            }
        };
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
            registry.retained_files.remove(&self.id);
        }
        Ok(result)
    }

    /// Issue termination only; never wait for readers, protocols or processes.
    pub fn terminate(&self) -> io::Result<()> {
        registry().terminate(self.id)
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

    fn insert(&mut self, resource: Resource) -> OwnedProcessTree {
        self.next_id += 1;
        self.resources.insert(self.next_id, resource);
        OwnedProcessTree { id: self.next_id }
    }

    fn terminate(&mut self, id: u64) -> io::Result<()> {
        if let Some(resource) = self.resources.get_mut(&id) {
            resource.terminate()?;
        }
        self.resources.remove(&id);
        Ok(())
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
