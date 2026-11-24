use super::*;

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
type ShutdownHookKey = (String, String);
#[cfg(test)]
type ShutdownHookFn = Box<dyn FnOnce() + Send>;

#[cfg(test)]
static SHUTDOWN_HOOKS: OnceLock<Mutex<HashMap<ShutdownHookKey, ShutdownHookFn>>> = OnceLock::new();

#[cfg(test)]
pub(crate) struct ShutdownHook {
    key: (String, String),
}

#[cfg(test)]
impl Drop for ShutdownHook {
    fn drop(&mut self) {
        if let Ok(mut hooks) = SHUTDOWN_HOOKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
        {
            hooks.remove(&self.key);
        }
    }
}

/// Why an Actor's delivery worker stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    /// Unfinished work keeps its ordinary retry semantics.
    Stop,
    /// The Actor's runtime owner is being replaced. Work whose submission may
    /// have started is recorded as ambiguous and never offered again; work
    /// that was never submitted stays retryable for the new owner.
    Replace,
}

pub fn shutdown_actor(group_id: &str, actor_id: &str) {
    shutdown_actor_for(group_id, actor_id, ShutdownReason::Stop);
}

/// Stop and join the Actor's worker. Its outcomes are recorded before this
/// returns, except a record that failed to persist: that claim stays claimed
/// and the caller settles it.
pub fn shutdown_actor_for(group_id: &str, actor_id: &str, reason: ShutdownReason) {
    gaps::clear(|(group, actor)| group == group_id && actor == actor_id);
    clear_refills(|(group, actor)| group == group_id && actor == actor_id);
    let worker = workers()
        .lock()
        .ok()
        .and_then(|mut workers| workers.remove(&(group_id.to_owned(), actor_id.to_owned())));
    if let Some(worker) = worker {
        #[cfg(test)]
        {
            let mut worker = worker;
            worker.shutdown_hook = take_shutdown_hook(group_id, actor_id);
            worker.shutdown_for(reason);
        }
        #[cfg(not(test))]
        worker.shutdown_for(reason);
    }
    clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
}

#[cfg(test)]
pub(crate) fn on_shutdown(
    group_id: &str,
    actor_id: &str,
    hook: impl FnOnce() + Send + 'static,
) -> ShutdownHook {
    let key = (group_id.to_owned(), actor_id.to_owned());
    SHUTDOWN_HOOKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("shutdown hooks")
        .insert(key.clone(), Box::new(hook));
    ShutdownHook { key }
}

#[cfg(test)]
fn take_shutdown_hook(group_id: &str, actor_id: &str) -> Option<Box<dyn FnOnce() + Send>> {
    SHUTDOWN_HOOKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("shutdown hooks")
        .remove(&(group_id.to_owned(), actor_id.to_owned()))
}

pub fn shutdown_group(group_id: &str) {
    gaps::clear(|(group, _)| group == group_id);
    clear_refills(|(group, _)| group == group_id);
    clear_group_catchup(group_id);
    shutdown_group_workers(group_id);
}

/// A user's next send can resume a paused group without a lifecycle restart.
pub fn pause_group(group_id: &str) {
    set_group_paused(group_id, true);
    shutdown_group_workers(group_id);
}

fn shutdown_group_workers(group_id: &str) {
    let removed = workers()
        .lock()
        .map(|mut workers| {
            let keys = workers
                .keys()
                .filter(|(worker_group_id, _)| worker_group_id == group_id)
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| workers.remove(&key))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for worker in removed {
        worker.shutdown();
    }
    clear_in_flight(|item| item.0 == group_id);
}

pub fn shutdown_all() {
    gaps::clear(|_| true);
    clear_refills(|_| true);
    clear_all_group_catchups();
    let removed = workers()
        .lock()
        .map(|mut workers| {
            std::mem::take(&mut *workers)
                .into_values()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for worker in removed {
        worker.shutdown();
    }
    if let Ok(mut completions) = completions().lock() {
        completions.clear();
    }
    if let Ok(mut notices) = sender_notices().lock() {
        notices.clear();
    }
    if let Ok(mut pending) = in_flight().lock() {
        pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_and_group_shutdown_remove_workers() {
        let first = ("g_cleanup".to_owned(), "actor-1".to_owned());
        let second = ("g_cleanup".to_owned(), "actor-2".to_owned());
        workers().lock().expect("workers").extend([
            (first.clone(), spawn_worker(&first)),
            (second.clone(), spawn_worker(&second)),
        ]);

        shutdown_actor(&first.0, &first.1);
        assert!(!workers().lock().expect("workers").contains_key(&first));
        shutdown_group(&second.0);
        assert!(!workers().lock().expect("workers").contains_key(&second));
    }
}
