//! Rust-side DeepSeek process lifecycle registry.
mod delivery;
mod delivery_projection;
mod launch_command;
mod lifecycle;
mod recovery;
mod turn_failure;
mod turn_timeout;

pub use delivery::deliver;
pub use lifecycle::apply;

use cccc_contracts::Actor;
use cccc_core::{GroupDoc, HomeLayout};
use cccc_runtime::deepseek_supervisor::DeepSeekSupervisor;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

type Key = (String, String);

pub(super) struct RuntimeEntry {
    pub(super) supervisor: std::sync::Mutex<DeepSeekSupervisor>,
    pub(super) running: AtomicBool,
    pub(super) manual_restart_required: AtomicBool,
    pub(super) generation: String,
}

pub(super) fn sessions() -> &'static RwLock<HashMap<Key, Arc<RuntimeEntry>>> {
    static SESSIONS: OnceLock<RwLock<HashMap<Key, Arc<RuntimeEntry>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn cancel_flags() -> &'static RwLock<HashMap<Key, Arc<AtomicBool>>> {
    static FLAGS: OnceLock<RwLock<HashMap<Key, Arc<AtomicBool>>>> = OnceLock::new();
    FLAGS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn start(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    cwd: &Path,
) -> std::io::Result<()> {
    let key = (group.group_id.clone(), actor.id.clone());
    stop(&group.group_id, &actor.id);
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let generation = uuid::Uuid::new_v4().simple().to_string();
    let mut supervisor = DeepSeekSupervisor::default();
    let mut env = actor.env.clone();
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    env.insert("CCCC_GROUP_ID".into(), group.group_id.clone());
    env.insert("CCCC_ACTOR_ID".into(), actor.id.clone());
    let session_root = home
        .root()
        .join("groups")
        .join(&group.group_id)
        .join("state/deepseek")
        .join(&actor.id)
        .join("sessions");
    std::fs::create_dir_all(&session_root)?;
    env.insert(
        "CCCC_DEEPSEEK_SESSION_ROOT".into(),
        session_root.to_string_lossy().into_owned(),
    );
    let command = launch_command::resolve(actor, &env)?;
    let env = env.into_iter().collect::<Vec<_>>();
    supervisor
        .start(&command, cwd, &env)
        .map_err(std::io::Error::other)?;
    if let Err(error) = supervisor.handshake(cwd, Duration::from_secs(5)) {
        let _ = supervisor.stop();
        return Err(std::io::Error::other(format!(
            "deepseek ACP handshake failed: {error}"
        )));
    }
    sessions()
        .write()
        .map_err(|_| std::io::Error::other("deepseek session lock poisoned"))?
        .insert(
            key,
            Arc::new(RuntimeEntry {
                supervisor: std::sync::Mutex::new(supervisor),
                running: AtomicBool::new(true),
                manual_restart_required: AtomicBool::new(false),
                generation: generation.clone(),
            }),
        );
    cancel_flags()
        .write()
        .map_err(|_| std::io::Error::other("deepseek cancel lock poisoned"))?
        .insert((group.group_id.clone(), actor.id.clone()), cancel_flag);
    if let Err(error) = cccc_core::deepseek_restart_gate::record_running_generation(
        home,
        &group.group_id,
        &actor.id,
        &actor.created_at,
        &generation,
    ) {
        stop(&group.group_id, &actor.id);
        return Err(error);
    }
    Ok(())
}

pub fn stop(group_id: &str, actor_id: &str) {
    let key = (group_id.to_owned(), actor_id.to_owned());
    if let Ok(flags) = cancel_flags().read() {
        if let Some(flag) = flags.get(&key) {
            flag.store(true, Ordering::Release);
        }
    }
    if let Some(holder) = sessions().write().ok().and_then(|mut map| map.remove(&key)) {
        holder.running.store(false, Ordering::Release);
        if let Ok(mut supervisor) = holder.supervisor.lock() {
            let _ = supervisor.stop();
        }
    }
    if let Ok(mut flags) = cancel_flags().write() {
        flags.remove(&key);
    }
}

pub fn stop_group(group_id: &str) {
    let actor_ids = sessions()
        .read()
        .map(|map| {
            map.keys()
                .filter(|(candidate, _)| candidate == group_id)
                .map(|(_, actor_id)| actor_id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for actor_id in actor_ids {
        stop(group_id, &actor_id);
    }
}

pub fn stop_all() {
    let keys = sessions()
        .read()
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    for (group_id, actor_id) in keys {
        stop(&group_id, &actor_id);
    }
}

pub(super) fn cancellation_requested(group_id: &str, actor_id: &str) -> bool {
    let key = (group_id.to_owned(), actor_id.to_owned());
    cancel_flags()
        .read()
        .ok()
        .and_then(|flags| flags.get(&key).cloned())
        .is_some_and(|flag| flag.load(Ordering::Acquire))
}

pub fn running(group_id: &str, actor_id: &str) -> bool {
    let key = (group_id.to_owned(), actor_id.to_owned());
    sessions()
        .read()
        .ok()
        .and_then(|map| map.get(&key).cloned())
        .is_some_and(|holder| {
            if !holder.running.load(Ordering::Acquire) {
                return false;
            }
            match holder.supervisor.try_lock() {
                Ok(mut supervisor) => {
                    let running = supervisor.is_running();
                    holder.running.store(running, Ordering::Release);
                    running
                }
                Err(std::sync::TryLockError::WouldBlock) => true,
                Err(std::sync::TryLockError::Poisoned(_)) => false,
            }
        })
}

pub(super) fn manual_restart_required(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> bool {
    let key = (group.group_id.clone(), actor.id.clone());
    if sessions()
        .read()
        .ok()
        .and_then(|map| map.get(&key).cloned())
        .is_some_and(|holder| holder.manual_restart_required.load(Ordering::Acquire))
    {
        return true;
    }
    match cccc_core::deepseek_restart_gate::manual_restart_required(
        home,
        &group.group_id,
        &actor.id,
        &actor.created_at,
    ) {
        Ok(required) => required,
        Err(error) => {
            tracing::error!(
                %error,
                group_id = %group.group_id,
                actor_id = %actor.id,
                "failed to read DeepSeek manual restart gate"
            );
            true
        }
    }
}

#[cfg(test)]
#[path = "deepseek_runtime/dedupe_tests.rs"]
mod dedupe_tests;
#[cfg(test)]
#[path = "deepseek_runtime/delivery_tests.rs"]
mod delivery_tests;
#[cfg(test)]
#[path = "deepseek_runtime/launch_command_tests.rs"]
mod launch_command_tests;
#[cfg(test)]
#[path = "deepseek_runtime/timeout_tests.rs"]
mod timeout_tests;
