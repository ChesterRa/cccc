//! Local session ownership, independent of the collaboration daemon's lifetime.
//!
//! This private protocol is versioned separately from the public daemon IPC.
//! It executes admitted member operations; routing and delivery settlement stay
//! in the daemon.

#[cfg(unix)]
mod admission;
#[cfg(unix)]
mod backend;
pub mod browser;
#[cfg(unix)]
mod browser_backend;
pub(crate) mod browser_operations;
#[cfg(unix)]
mod client;
#[cfg(all(test, unix))]
mod confirmation_tests;
#[cfg(all(test, unix))]
mod contract_tests;
#[cfg(unix)]
mod controller;
mod facade;
#[cfg(unix)]
mod installer;
#[cfg(unix)]
mod mcp;
#[cfg(unix)]
mod preservation;
#[cfg(unix)]
mod projection;
#[cfg(unix)]
mod protocol;
pub(crate) mod receipt;
#[cfg(unix)]
mod recovery;
#[cfg(unix)]
mod remote;
pub(crate) mod runtime;
mod shutdown;
#[cfg(all(test, unix))]
mod startup_input_tests;
#[cfg(all(test, unix, debug_assertions))]
mod test_entry;
#[cfg(unix)]
mod transport;

#[doc(hidden)]
pub use cccc_runtime::process_watchdog_test_entry;
pub(crate) use facade::*;
#[cfg(unix)]
pub use preservation::AgentObservations;
pub use receipt::{AgentIdentity, ShutdownMode, ShutdownReport, read_shutdown_report};
pub use shutdown::startup_retained;

/// The independent holder and its native recovery must be enabled together.
/// Executable relay support alone does not establish holder preservation.
pub const fn platform_supported() -> bool {
    cfg!(any(
        target_os = "linux",
        all(target_os = "macos", target_arch = "aarch64")
    ))
}

/// Read current holder and saved remote-work facts without starting or
/// acquiring resources. The deadline covers the complete observation.
#[cfg(unix)]
pub async fn observations(
    home: &cccc_core::HomeLayout,
    budget: std::time::Duration,
) -> std::io::Result<AgentObservations> {
    preservation::observations(home, budget).await
}

#[cfg(not(unix))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentObservations {
    pub running: Vec<AgentIdentity>,
    pub unconfirmed: Vec<AgentIdentity>,
    pub unavailable_owners: Vec<String>,
}

#[cfg(not(unix))]
pub async fn observations(
    _home: &cccc_core::HomeLayout,
    _budget: std::time::Duration,
) -> std::io::Result<AgentObservations> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "independent member observations are unavailable on this platform",
    ))
}

pub async fn preservation_check(
    home: &cccc_core::HomeLayout,
    candidate: &cccc_core::update_capabilities::UpdateCapabilities,
) -> cccc_core::update_capabilities::KeepDecision {
    #[cfg(unix)]
    return preservation::check(home, candidate).await;
    #[cfg(not(unix))]
    {
        let _ = (home, candidate);
        cccc_core::update_capabilities::KeepDecision {
            allowed: false,
            refusals: vec![cccc_core::update_capabilities::KeepRefusal {
                code: "keep_not_supported".into(),
                group_id: None,
                actor_id: None,
                owner_id: None,
                display_name: None,
                message: "Keeping independent members is unavailable on this platform.".into(),
            }],
        }
    }
}
pub(crate) use shutdown::{
    begin_shutdown, maintenance_selected, prepare_shutdown, select_shutdown, selected_shutdown,
    shutdown_request,
};

/// The private owner protocol this binary implements. This is a compiled
/// declaration, not evidence that an owner is running or preservation is ready.
pub fn ownership_contract() -> Option<u32> {
    #[cfg(unix)]
    return Some(protocol::CONTRACT);
    #[cfg(not(unix))]
    None
}
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
    if platform_supported() {
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

/// Only the actual owner publishes a binding; controllers cannot synthesize
/// one for an older owner. Its stable locator belongs in the main MCP entry.
pub(crate) fn mcp_binding_file(group: &str, actor: &str) -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    return owner_backend().get()?.mcp_binding_file(group, actor);
    #[cfg(not(unix))]
    {
        let _ = (group, actor);
        None
    }
}

/// Stop retained member owners without starting the collaboration daemon or any
/// provider. The same home lock excludes a concurrent controller startup.
/// None means no live holder was found. A holder can have only a browser.
pub async fn stop_without_daemon(
    home: cccc_core::HomeLayout,
) -> anyhow::Result<Option<ShutdownReport>> {
    #[cfg(unix)]
    if platform_supported() {
        return tokio::task::spawn_blocking(move || controller::stop_offline(&home)).await?;
    }
    {
        let _ = home;
        anyhow::bail!("independent session ownership is not implemented on this platform")
    }
}

/// Whether an installer may replace the program while no daemon runs: either
/// no session owner runs, or the last daemon exit kept its agents and every
/// owner answers now. Err carries the reason and the next step. The check
/// creates or changes no file under CCCC_HOME and holds no lock afterwards.
pub async fn installer_check(home: &cccc_core::HomeLayout) -> Result<(), String> {
    #[cfg(unix)]
    return installer::check(home).await;
    #[cfg(not(unix))]
    {
        let _ = home;
        Ok(())
    }
}

/// Internal process entry. The launcher must not preassign a process group:
/// this process establishes a new session before creating any member resource.
pub async fn run(home: cccc_core::HomeLayout, owner_id: &str) -> anyhow::Result<()> {
    run_owner(home, owner_id, None).await
}

/// Supply the executable's browser launcher to the independent holder itself.
/// Browser processes and their profile leases never belong to the daemon port.
pub async fn run_with_browser_launcher(
    home: cccc_core::HomeLayout,
    owner_id: &str,
    launcher: std::sync::Arc<dyn browser::BrowserLauncher>,
) -> anyhow::Result<()> {
    run_owner(home, owner_id, Some(launcher)).await
}

async fn run_owner(
    home: cccc_core::HomeLayout,
    owner_id: &str,
    launcher: Option<std::sync::Arc<dyn browser::BrowserLauncher>>,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    if platform_supported() {
        return transport::run(home, owner_id, launcher).await;
    }
    {
        let _ = (home, owner_id, launcher);
        anyhow::bail!("independent session ownership is not implemented on this platform")
    }
}

/// Executable entry for development test harnesses. Release binaries contain
/// neither this helper nor the alternate child-launch path.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn run_test_child() {
    run_test_child_impl(None);
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn run_test_child_with_browser_launcher(
    launcher: std::sync::Arc<dyn browser::BrowserLauncher>,
) {
    run_test_child_impl(Some(launcher));
}

#[cfg(debug_assertions)]
fn run_test_child_impl(launcher: Option<std::sync::Arc<dyn browser::BrowserLauncher>>) {
    let Ok(owner) = std::env::var("CCCC_TEST_SESSION_OWNER_CHILD") else {
        return;
    };
    let home = cccc_core::HomeLayout::resolve().expect("isolated owner home");
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("owner test executor")
        .block_on(run_owner(home, &owner, launcher))
        .expect("owner child");
}

/// Declare the internal executable entry in a Rust integration-test harness.
/// This includes the watchdog entry; do not also declare it in the same module.
#[doc(hidden)]
#[macro_export]
macro_rules! session_owner_test_entry {
    ($launcher:expr) => {
        $crate::session_owner::process_watchdog_test_entry!();
        #[cfg(all(unix, debug_assertions))]
        #[test]
        #[ignore = "internal session-owner subprocess entry"]
        fn __cccc_session_owner_child() {
            $crate::session_owner::run_test_child_with_browser_launcher($launcher);
        }
    };
    () => {
        $crate::session_owner::process_watchdog_test_entry!();
        #[cfg(all(unix, debug_assertions))]
        #[test]
        #[ignore = "internal session-owner subprocess entry"]
        fn __cccc_session_owner_child() {
            $crate::session_owner::run_test_child();
        }
    };
}
