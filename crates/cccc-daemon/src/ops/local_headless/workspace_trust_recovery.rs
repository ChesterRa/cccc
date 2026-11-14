//! Cancellation and commit boundary for a pending interactive trust prompt.
use super::supervisor::{Key, StartGuard};
use cccc_core::HomeLayout;
use std::collections::HashMap;
use std::io;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU8, Ordering},
};

#[cfg(test)]
use std::sync::atomic::AtomicBool;

const ACTIVE: u8 = 0;
const CANCELLED: u8 = 1;
const FAILED: u8 = 2;

fn pending() -> &'static Mutex<HashMap<Key, Arc<AtomicU8>>> {
    static PENDING: OnceLock<Mutex<HashMap<Key, Arc<AtomicU8>>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) struct Recovery {
    key: Key,
    cancelled: Arc<AtomicU8>,
}

impl Recovery {
    pub(super) fn register(key: Key) -> io::Result<Self> {
        let cancelled = Arc::new(AtomicU8::new(ACTIVE));
        if let Some(previous) = pending()
            .lock()
            .map_err(|_| super::poisoned())?
            .insert(key.clone(), Arc::clone(&cancelled))
        {
            previous.store(CANCELLED, Ordering::Release);
        }
        Ok(Self { key, cancelled })
    }

    pub(super) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) == CANCELLED
    }

    /// Stop blocking automatic start before a genuine failure closes the temporary PTY.
    pub(super) fn retire_failed_start(&self) {
        let _ =
            self.cancelled
                .compare_exchange(ACTIVE, FAILED, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Publish refusal only while this exact recovery still owns the Actor.
    #[cfg(test)]
    pub(super) fn pause_if_current(&self) -> io::Result<bool> {
        self.finish_if_current(true)
    }

    /// Keep the same owner fence for an exit event without changing failure retry policy.
    pub(super) fn finish_if_current(&self, pause: bool) -> io::Result<bool> {
        let items = pending().lock().map_err(|_| super::poisoned())?;
        if self.cancelled()
            || !items
                .get(&self.key)
                .is_some_and(|current| Arc::ptr_eq(current, &self.cancelled))
        {
            return Ok(false);
        }
        if pause {
            super::pause_confirmation(&self.key.0, &self.key.1)?;
        }
        Ok(true)
    }
}

impl Drop for Recovery {
    fn drop(&mut self) {
        if let Ok(mut items) = pending().lock()
            && items
                .get(&self.key)
                .is_some_and(|current| Arc::ptr_eq(current, &self.cancelled))
        {
            items.remove(&self.key);
        }
    }
}

pub(super) fn cancel(key: &Key) -> io::Result<()> {
    if let Some(cancelled) = pending().lock().map_err(|_| super::poisoned())?.get(key) {
        cancelled.store(CANCELLED, Ordering::Release);
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

pub(super) fn registered(key: &Key) -> io::Result<bool> {
    Ok(pending()
        .lock()
        .map_err(|_| super::poisoned())?
        .contains_key(key))
}

pub(super) fn active(key: &Key) -> io::Result<bool> {
    Ok(pending()
        .lock()
        .map_err(|_| super::poisoned())?
        .get(key)
        .is_some_and(|cancelled| cancelled.load(Ordering::Acquire) == ACTIVE))
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
            if super::super::codex_voice_analyst::claude_confirmation_launch(&error).is_none() {
                return Err(error);
            }
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
