//! Cancellation and commit boundary for a pending interactive trust prompt.
use super::supervisor::{Key, StartGuard};
use cccc_core::HomeLayout;
use std::collections::HashMap;
use std::io;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

fn pending() -> &'static Mutex<HashMap<Key, Arc<AtomicBool>>> {
    static PENDING: OnceLock<Mutex<HashMap<Key, Arc<AtomicBool>>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) struct Recovery {
    key: Key,
    cancelled: Arc<AtomicBool>,
}

impl Recovery {
    pub(super) fn register(key: Key) -> io::Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        if let Some(previous) = pending()
            .lock()
            .map_err(|_| super::poisoned())?
            .insert(key.clone(), Arc::clone(&cancelled))
        {
            previous.store(true, Ordering::Release);
        }
        Ok(Self { key, cancelled })
    }

    pub(super) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for Recovery {
    fn drop(&mut self) {
        if let Ok(mut items) = pending().lock() {
            if items
                .get(&self.key)
                .is_some_and(|current| Arc::ptr_eq(current, &self.cancelled))
            {
                items.remove(&self.key);
            }
        }
    }
}

pub(super) fn cancel(key: &Key) -> io::Result<()> {
    if let Some(cancelled) = pending().lock().map_err(|_| super::poisoned())?.get(key) {
        cancelled.store(true, Ordering::Release);
    }
    Ok(())
}

pub(super) fn keys() -> io::Result<Vec<Key>> {
    Ok(pending()
        .lock()
        .map_err(|_| super::poisoned())?
        .keys()
        .cloned()
        .collect())
}

/// External provider operations are callbacks; locking and cancellation always use the
/// same production path. Stop cancels before waiting for StartGuard, then stops any
/// session which committed before that cancellation.
pub(super) fn recover<T>(
    home: &HomeLayout,
    recovery: &Recovery,
    prompt_open: impl Fn() -> bool,
    launch: impl FnOnce() -> io::Result<T>,
    attach: impl FnOnce(T) -> io::Result<()>,
    discard: impl FnOnce(T) -> io::Result<()>,
) -> io::Result<bool> {
    // Match actor_runtime::start_local_headless: permit MUST precede StartGuard.
    let Ok(_permit) = crate::runtime_start_gate::permit(home) else {
        return Ok(true);
    };
    let _start = StartGuard::acquire(&recovery.key)?;
    if recovery.cancelled() || !prompt_open() {
        return Ok(true);
    }
    let app = match launch() {
        Ok(app) => app,
        Err(error) => {
            tracing::debug!(%error, group_id = %recovery.key.0, actor_id = %recovery.key.1,
                "Claude workspace is not ready for a managed session yet");
            return Ok(recovery.cancelled() || !prompt_open());
        }
    };
    if recovery.cancelled() || !prompt_open() {
        discard(app)?;
        return Ok(true);
    }
    attach(app)?;
    Ok(true)
}

#[cfg(test)]
#[path = "workspace_trust_recovery_tests.rs"]
mod tests;
