//! Local session ownership, independent of the collaboration daemon's lifetime.
//!
//! This private protocol is versioned separately from the public daemon IPC.
//! It executes admitted member operations; routing and delivery settlement stay
//! in the daemon.

#[cfg(unix)]
mod admission;
#[cfg(unix)]
mod backend;
#[cfg(unix)]
mod client;
#[cfg(unix)]
mod controller;
mod facade;
#[cfg(unix)]
mod projection;
#[cfg(unix)]
mod protocol;
pub(crate) mod receipt;
#[cfg(unix)]
mod recovery;
pub(crate) mod runtime;
mod shutdown;
#[cfg(all(test, unix, debug_assertions))]
mod test_entry;
#[cfg(unix)]
mod transport;

pub(crate) use facade::*;
pub use receipt::{AgentIdentity, ShutdownMode, ShutdownReport, read_shutdown_report};
pub(crate) use shutdown::{
    begin_shutdown, maintenance_selected, prepare_shutdown, select_shutdown, shutdown_request,
    startup_retained,
};

/// The foreground host owns this in-process daemon and intends an ordinary
/// stop. Select that before a queued IPC request or an abort can lose intent.
pub fn begin_owned_stop(home: &cccc_core::HomeLayout) {
    shutdown::begin_shutdown(home);
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum RecoveryAction {
    List,
    Retry { id: String, version: String },
    Accept { id: String, version: String },
}

/// Offline review uses the same home and owner locks as daemon startup.
pub fn recover(
    home: &cccc_core::HomeLayout,
    owner_id: &str,
    action: RecoveryAction,
) -> anyhow::Result<serde_json::Value> {
    #[cfg(unix)]
    if cfg!(target_os = "linux") {
        return recovery::recover(home, owner_id, action);
    }
    {
        let _ = (home, owner_id, action);
        anyhow::bail!("session-owner recovery is not implemented on this platform")
    }
}

#[cfg(unix)]
fn owner_backend() -> &'static std::sync::OnceLock<std::sync::Arc<backend::Backend>> {
    static OWNER: std::sync::OnceLock<std::sync::Arc<backend::Backend>> =
        std::sync::OnceLock::new();
    &OWNER
}

#[cfg(unix)]
fn install_backend(backend: std::sync::Arc<backend::Backend>) -> std::io::Result<()> {
    owner_backend()
        .set(backend)
        .map_err(|_| std::io::Error::other("session owner entry already initialized"))
}

pub(crate) fn is_owner() -> bool {
    #[cfg(unix)]
    return owner_backend().get().is_some();
    #[cfg(not(unix))]
    false
}

/// The Claude launcher calls before it can create or adopt an independent job.
pub(crate) fn protect_detached_work(group: &str, actor: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Some(backend) = owner_backend().get() {
        return backend.protect_detached_work(group, actor);
    }
    let _ = (group, actor);
    Ok(())
}

/// Call only with proof of no launch, a native prelaunch refusal, or the
/// existing native Stop/rollback's authoritative job-removal confirmation.
pub(crate) fn confirm_detached_work_stopped(group: &str, actor: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Some(backend) = owner_backend().get() {
        return backend.clear_detached_work(&(group.into(), actor.into()));
    }
    let _ = (group, actor);
    Ok(())
}

/// Keep a reader's exit observation in its owner. The daemon reconciles it
/// through its existing Group-maintenance path; the owner never writes that
/// collaboration event itself.
pub(crate) fn retain_provider_exit(group: &str, actor: &str, generation: &str) -> bool {
    #[cfg(unix)]
    if let Some(backend) = owner_backend().get() {
        backend.record_provider_exit(group, actor, generation);
        return true;
    }
    let _ = (group, actor, generation);
    false
}

/// The spawning controller's environment for this member, captured per Start.
/// This is live owner state, never logged or persisted in discovery metadata.
pub(crate) fn launch_environment(
    group: &str,
    actor: &str,
) -> Option<std::collections::BTreeMap<String, String>> {
    #[cfg(unix)]
    return owner_backend().get()?.environment(group, actor);
    #[cfg(not(unix))]
    {
        let _ = (group, actor);
        None
    }
}

/// Stop retained member owners without starting the collaboration daemon or any
/// provider. The same home lock excludes a concurrent controller startup.
pub async fn stop_without_daemon(home: cccc_core::HomeLayout) -> anyhow::Result<ShutdownReport> {
    #[cfg(unix)]
    if cfg!(target_os = "linux") {
        return tokio::task::spawn_blocking(move || controller::stop_offline(&home)).await?;
    }
    {
        let _ = home;
        anyhow::bail!("independent session ownership is not implemented on this platform")
    }
}

/// Internal process entry. The launcher must not preassign a process group:
/// this process establishes a new session before creating any member resource.
pub async fn run(home: cccc_core::HomeLayout, owner_id: &str) -> anyhow::Result<()> {
    #[cfg(unix)]
    if cfg!(target_os = "linux") {
        return transport::run(home, owner_id).await;
    }
    {
        let _ = (home, owner_id);
        anyhow::bail!("independent session ownership is not implemented on this platform")
    }
}

/// Executable entry for development test harnesses. Release binaries contain
/// neither this helper nor the alternate child-launch path.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn run_test_child() {
    let Ok(owner) = std::env::var("CCCC_TEST_SESSION_OWNER_CHILD") else {
        return;
    };
    let home = cccc_core::HomeLayout::resolve().expect("isolated owner home");
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("owner test executor")
        .block_on(run(home, &owner))
        .expect("owner child");
}

/// Declare the internal executable entry in a Rust integration-test harness.
#[doc(hidden)]
#[macro_export]
macro_rules! session_owner_test_entry {
    () => {
        #[cfg(all(unix, debug_assertions))]
        #[test]
        #[ignore = "internal session-owner subprocess entry"]
        fn __cccc_session_owner_child() {
            $crate::session_owner::run_test_child();
        }
    };
}
