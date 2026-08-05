use cccc_core::HomeLayout;
use cccc_core::runtime_hook_identity::{
    RuntimeHookLaunchIdentity, read as read_identity, remove as remove_identity,
    write as write_identity,
};
use cccc_runtime::SessionStatus;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex, OnceLock};

type Key = (String, String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSetup {
    pub runtime: String,
    pub launch_token: String,
    pub hook_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSessionCapability {
    pub runtime: String,
    pub launch_token: String,
    pub pid: u32,
}

fn capabilities() -> &'static Mutex<HashMap<Key, HookSessionCapability>> {
    static CAPABILITIES: OnceLock<Mutex<HashMap<Key, HookSessionCapability>>> = OnceLock::new();
    CAPABILITIES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn launch_locks() -> &'static Mutex<HashMap<Key, Arc<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<Key, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn with_launch_lock<T>(group_id: &str, actor_id: &str, operation: impl FnOnce() -> T) -> T {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let lock = launch_locks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();
    let _guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    operation()
}

pub fn prepare_identity(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    setup: Option<&HookSetup>,
) -> io::Result<()> {
    revoke(group_id, actor_id);
    super::runtime_hook_input::reset(group_id, actor_id);
    let Some(setup) = setup else {
        return remove_identity(home, group_id, actor_id);
    };
    write_identity(
        home,
        &RuntimeHookLaunchIdentity::new(
            group_id,
            actor_id,
            &setup.runtime,
            &setup.launch_token,
            false,
            0,
        ),
    )
}

pub fn bind(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    setup: &HookSetup,
    status: &SessionStatus,
) -> io::Result<()> {
    let pid = status
        .pid
        .ok_or_else(|| io::Error::other("runtime hook session has no pid"))?;
    let identity = RuntimeHookLaunchIdentity::new(
        group_id,
        actor_id,
        &setup.runtime,
        &setup.launch_token,
        setup.hook_enabled,
        pid,
    );
    write_identity(home, &identity)?;
    if setup.hook_enabled {
        capabilities()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                (group_id.to_owned(), actor_id.to_owned()),
                HookSessionCapability {
                    runtime: setup.runtime.clone(),
                    launch_token: setup.launch_token.clone(),
                    pid,
                },
            );
    }
    Ok(())
}

pub fn revoke(group_id: &str, actor_id: &str) {
    capabilities()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&(group_id.to_owned(), actor_id.to_owned()));
}

#[must_use]
pub fn validated(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
    pid: Option<u32>,
) -> Option<HookSessionCapability> {
    let pid = pid?;
    let capability = capabilities()
        .lock()
        .ok()?
        .get(&(group_id.to_owned(), actor_id.to_owned()))
        .cloned()?;
    if capability.runtime != runtime || capability.pid != pid || capability.launch_token.is_empty()
    {
        return None;
    }
    let identity = read_identity(home, group_id, actor_id)?;
    (identity.hook_enabled
        && identity.runtime == capability.runtime
        && identity.launch_token == capability.launch_token
        && identity.pid == capability.pid)
        .then_some(capability)
}

#[cfg(test)]
pub fn bind_for_test(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    runtime: &str,
    token: &str,
    pid: u32,
) {
    let setup = HookSetup {
        runtime: runtime.into(),
        launch_token: token.into(),
        hook_enabled: true,
    };
    bind(
        home,
        group_id,
        actor_id,
        &setup,
        &SessionStatus {
            group_id: group_id.into(),
            actor_id: actor_id.into(),
            runner: cccc_contracts::RunnerKind::Pty,
            running: true,
            pid: Some(pid),
            started_at: String::new(),
            exit_code: None,
        },
    )
    .expect("bind test hook capability");
}
