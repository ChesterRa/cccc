//! Combined Web and daemon host lifecycle.

use anyhow::{Result, bail};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use std::time::Duration;

use crate::daemon_lifecycle::{self, DaemonSlot};
use crate::host_state::{DaemonState, HostPhase, HostState, ShutdownReport, StopResult, WebState};
use crate::web_instance;
use tokio::sync::{mpsc, watch};

pub(crate) fn use_terminal_interface(
    enabled: bool,
    terminal: bool,
    mode: cccc_web::WebMode,
) -> bool {
    enabled && terminal && mode == cccc_web::WebMode::Normal
}

struct Interface {
    state: watch::Sender<HostState>,
    session: crate::tui::Session,
    // Drop after the terminal session, so even cleanup warnings cannot corrupt it.
    _diagnostics: cccc_runtime::DiagnosticLog,
}

impl Interface {
    async fn start(
        home: &HomeLayout,
        daemon: DaemonState,
        control: &mut crate::shutdown::TerminalControl,
    ) -> Result<Self> {
        let diagnostics =
            cccc_runtime::log_diagnostics_to(&home.daemon_dir().join("cccc-web.log"))?;
        let (state, receiver) = watch::channel(HostState {
            daemon,
            web: WebState::Starting,
            phase: HostPhase::Running,
        });
        let (intents, receiver_intents) = mpsc::unbounded_channel();
        control.connect(receiver_intents);
        let session = crate::tui::start(home.clone(), receiver, intents).await?;
        Ok(Self {
            state,
            session,
            _diagnostics: diagnostics,
        })
    }

    fn listening(&self) -> cccc_web::ListeningCallback {
        let state = self.state.clone();
        std::sync::Arc::new(move |address| {
            state.send_modify(|state| {
                state.web = WebState::Listening {
                    url: crate::web_endpoint::format(&address.ip().to_string(), address.port()),
                };
            });
        })
    }

    fn stopping(&self) {
        self.state
            .send_modify(|state| state.phase = HostPhase::Stopping);
    }

    async fn finish(self, report: ShutdownReport) -> Result<()> {
        self.state.send_modify(|state| {
            state.web = WebState::Stopped;
            state.phase = HostPhase::Stopped { report };
        });
        self.session.finish().await
    }
}

// Only the round trip that hands over the shutdown request, not the shutdown
// it starts.
#[cfg(not(windows))]
const DAEMON_SHUTDOWN_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
// One attempt in the readiness poll, not a verdict: a miss schedules a retry
// and only `DAEMON_READY_TIMEOUT` concludes anything.
#[cfg(not(windows))]
const DAEMON_READINESS_POLL_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(not(windows))]
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(30);
const WEB_INSTANCE_EXIT_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) async fn launch(
    home: HomeLayout,
    host_override: Option<String>,
    port_override: Option<u16>,
    web_mode: Option<cccc_web::WebMode>,
    terminal: bool,
) -> Result<()> {
    home.initialize()?;
    let mut binding = crate::web_launch::resolve(&home, host_override.as_deref(), port_override)?;
    let client = DaemonClient::new(home.clone());
    let instance = claim_web_instance(&home, &client).await?;
    instance.hold_until_process_exit();
    let daemon_slot = daemon_lifecycle::resolve_slot(&home, &client).await?;
    let daemon_missing = daemon_slot == DaemonSlot::Vacant;
    let mut terminal_control = if terminal {
        Some(crate::shutdown::TerminalControl::new()?)
    } else {
        None
    };

    // Running the daemon as a task inside the Web host is unreliable on
    // Windows once the daemon installs its kill-on-close Job object, so it
    // gets its own process there. Everywhere else it is a task in this one.
    #[cfg(windows)]
    let detached_daemon = crate::detached_daemon_owner::shared(if daemon_missing {
        tokio::select! {
            owner = crate::detached_daemon_owner::OwnedDetachedDaemon::start(&home, &client) => owner?,
            () = async {
                if let Some(control) = &terminal_control { control.controller.requested().await; }
                else { std::future::pending::<()>().await; }
            } => {
                eprintln!("CCCC stopped during startup.");
                return Ok(());
            }
        }
    } else {
        None
    });
    // Ownership is what was actually started, not what was missing: a
    // competing launcher may have won the race in between.
    #[cfg(windows)]
    let daemon_owned = detached_daemon.lock().await.is_some();
    #[cfg(windows)]
    let (_daemon_exit_tx, daemon_exit_rx) = tokio::sync::watch::channel(false);

    #[cfg(not(windows))]
    let (daemon_exit_tx, mut daemon_exit_rx) = tokio::sync::watch::channel(false);
    #[cfg(not(windows))]
    let mut embedded_daemon = None;
    #[cfg(not(windows))]
    if daemon_missing {
        let daemon_home = home.clone();
        embedded_daemon = Some(tokio::spawn(async move {
            let result = cccc_daemon::run(daemon_home).await;
            let _ = daemon_exit_tx.send(true);
            result
        }));
        let readiness = DaemonClient::new(home.clone()).with_timeout(DAEMON_READINESS_POLL_TIMEOUT);
        let ready = tokio::select! {
            ready = daemon_lifecycle::wait_until_ready_or_owner_exits(
                &readiness, &mut daemon_exit_rx, DAEMON_READY_TIMEOUT,
            ) => ready,
            () = async {
                if let Some(control) = &terminal_control { control.controller.requested().await; }
                else { std::future::pending::<()>().await; }
            } => false,
        };
        if !ready {
            let cause = finish_embedded_daemon(&client, embedded_daemon.take()).await;
            if terminal_control
                .as_ref()
                .is_some_and(|control| control.controller.is_requested())
            {
                if let Some(error) = cause {
                    return Err(error);
                }
                eprintln!("CCCC stopped during startup.");
                return Ok(());
            }
            return Err(match cause {
                Some(cause) => cause.context("Rust daemon failed to become ready"),
                None => anyhow::anyhow!(
                    "Rust daemon failed to become ready; see {}",
                    home.daemon_dir().join("ccccd.log").display()
                ),
            });
        }
    }
    #[cfg(not(windows))]
    let daemon_owned = embedded_daemon
        .as_ref()
        .is_some_and(|daemon| !daemon.is_finished());

    let external_pid = match daemon_slot {
        DaemonSlot::Adopted { pid } => pid,
        DaemonSlot::Vacant => crate::daemon_takeover::daemon_pid(&home),
    };
    #[cfg(windows)]
    let owned_pid = detached_daemon
        .lock()
        .await
        .as_ref()
        .map(|owner| owner.pid());
    #[cfg(not(windows))]
    let owned_pid = daemon_owned.then_some(std::process::id());
    let daemon_state = if let Some(pid) = owned_pid {
        DaemonState::StartedHere { pid }
    } else {
        DaemonState::External { pid: external_pid }
    };

    let (mut interface, startup_error) = if let Some(control) = terminal_control.as_mut() {
        if control.controller.is_requested() {
            (None, None)
        } else {
            match Interface::start(&home, daemon_state, control).await {
                Ok(interface) => (Some(interface), None),
                Err(error) => (
                    None,
                    Some(error.context("could not open the CCCC terminal interface; run `cccc web` to start without it")),
                ),
            }
        }
    } else {
        daemon_lifecycle::announce_ownership(&client, daemon_owned, daemon_slot).await;
        (None, None)
    };

    // The plain host retains its existing signal behavior. The TUI's controller
    // also receives raw key events and keeps running through Web restarts.
    let owned_home = daemon_owned.then(|| home.clone());
    #[cfg(windows)]
    let plain_watchdog = (!terminal).then(|| {
        tokio::spawn(crate::shutdown::watch_for_interrupt(
            owned_home,
            std::sync::Arc::clone(&detached_daemon),
        ))
    });
    #[cfg(not(windows))]
    let plain_watchdog =
        (!terminal).then(|| tokio::spawn(crate::shutdown::watch_for_interrupt(owned_home)));

    let mode = web_mode.unwrap_or_else(cccc_web::WebMode::from_env);
    let mut result = if let Some(error) = startup_error {
        Err(error)
    } else {
        loop {
            let monitored_home = home.clone();
            let mut daemon_exited = daemon_exit_rx.clone();
            let controller = terminal_control
                .as_ref()
                .map(|control| control.controller.clone());
            if controller
                .as_ref()
                .is_some_and(|controller| controller.is_requested())
            {
                break Ok(());
            }
            let state = interface.as_ref().map(|interface| interface.state.clone());
            let shutdown = async move {
                let daemon_lost = tokio::select! {
                    () = daemon_lifecycle::wait_for_loss(&monitored_home) => true,
                    _ = daemon_exited.wait_for(|exited| *exited) => true,
                    () = async {
                        if let Some(controller) = &controller { controller.requested().await; }
                        else { std::future::pending::<()>().await; }
                    } => false,
                };
                if daemon_lost {
                    cccc_runtime::diagnostic(format_args!(
                        "CCCC daemon stopped; Web server closed"
                    ));
                }
                if let Some(controller) = controller {
                    controller.request();
                }
                if let Some(state) = state {
                    state.send_modify(|state| state.phase = HostPhase::Stopping);
                }
            };
            let outcome = {
                let serving = cccc_web::serve_until_mode_supervised(
                    home.clone(),
                    &binding.host,
                    binding.port,
                    mode,
                    shutdown,
                    interface.as_ref().map(Interface::listening),
                );
                tokio::pin!(serving);
                if let Some(control) = &terminal_control {
                    tokio::select! {
                        biased;
                        () = control.controller.requested() => {
                            // The Web library also observes OS signals. Even when
                            // its own signal wins that race, cancel installation
                            // now, rather than after Web's connection drain.
                            if let Some(interface) = interface.as_mut() {
                                interface.stopping();
                                interface.session.stop_work().await;
                            }
                            serving.await
                        }
                        outcome = &mut serving => outcome,
                    }
                } else {
                    serving.await
                }
            };
            match outcome {
                Ok(cccc_web::ServeOutcome::Stopped(_)) => break Ok(()),
                Ok(cccc_web::ServeOutcome::RestartRequested) => {
                    if terminal_control
                        .as_ref()
                        .is_some_and(|control| control.controller.is_requested())
                    {
                        break Ok(());
                    }
                    if let Some(interface) = &interface {
                        interface
                            .state
                            .send_modify(|state| state.web = WebState::Restarting);
                    }
                    binding = match crate::web_launch::resolve(&home, None, None) {
                        Ok(binding) => binding,
                        Err(error) => break Err(error),
                    };
                    cccc_runtime::diagnostic(format_args!(
                        "[cccc] Applying saved Web binding: http://{}:{}",
                        binding.host, binding.port
                    ));
                }
                Err(error) => {
                    if let Some(interface) = &interface {
                        interface.state.send_modify(|state| {
                            state.web = WebState::Error {
                                message: error.to_string(),
                            }
                        });
                    }
                    break Err(error);
                }
            }
        }
    };

    let web_result = match &result {
        Ok(()) => StopResult::Stopped,
        Err(error) => StopResult::Failed {
            message: error.to_string(),
        },
    };
    if let Some(control) = &terminal_control {
        control.controller.request();
    }
    if let Some(interface) = interface.as_mut() {
        interface.stopping();
        interface.state.send_modify(|state| {
            if !matches!(state.web, WebState::Error { .. }) {
                state.web = WebState::Stopped;
            }
        });
        interface.session.stop_work().await;
    }
    cccc_mcp::shutdown(&home).await;
    let mut daemon_result = StopResult::KeptExternal { pid: external_pid };
    #[cfg(windows)]
    if daemon_owned {
        daemon_result =
            match crate::detached_daemon_owner::stop_shared(&detached_daemon, &client).await {
                Ok(()) => StopResult::Stopped,
                Err(error) => StopResult::Failed {
                    message: error.to_string(),
                },
            };
    }
    #[cfg(not(windows))]
    if daemon_owned {
        daemon_result = match finish_embedded_daemon(&client, embedded_daemon.take()).await {
            None => StopResult::Stopped,
            Some(error) => StopResult::Failed {
                message: error.to_string(),
            },
        };
    }
    if daemon_owned && let Err(error) = verify_daemon_stopped(&home).await {
        daemon_result = StopResult::Failed {
            message: error.to_string(),
        };
    }
    if let StopResult::Failed { message } = &daemon_result {
        cccc_runtime::diagnostic(format_args!(
            "failed to stop the CCCC-owned daemon: {message}"
        ));
        if result.is_ok() {
            result = Err(anyhow::anyhow!(message.clone()));
        }
    }
    if let Some(interface) = interface {
        let report = ShutdownReport {
            web: web_result,
            daemon: daemon_result,
            log_path: home.daemon_dir().join("cccc-web.log"),
        };
        if let Err(error) = interface.finish(report).await {
            if result.is_ok() {
                result = Err(error);
            }
        }
    }
    if let Some(watchdog) = plain_watchdog {
        watchdog.abort();
    }
    // Dropping the TUI controller stops its signal and force-exit watchers only
    // after the final receipt and terminal restoration have completed.
    drop(terminal_control);
    result
}

async fn claim_web_instance(
    home: &HomeLayout,
    client: &DaemonClient,
) -> Result<web_instance::WebInstance> {
    match web_instance::try_claim(home)? {
        web_instance::Claim::Acquired(instance) => Ok(instance),
        web_instance::Claim::Running(running) => {
            confirm_and_stop_existing(home, client, running.pid).await?;
            web_instance::wait_until_free(home, WEB_INSTANCE_EXIT_TIMEOUT).await
        }
    }
}

async fn confirm_and_stop_existing(
    home: &HomeLayout,
    client: &DaemonClient,
    pid: Option<u32>,
) -> Result<()> {
    if !web_instance::confirm_stop(home, pid)? {
        bail!(
            "another CCCC process is already running for CCCC_HOME={}{}",
            home.root().display(),
            pid.map_or_else(String::new, |pid| format!(" (pid={pid})"))
        );
    }
    // Legacy and incompatible daemons are asked to stop too: anything that
    // answers is the process whose Web lock this launch is waiting on.
    if daemon_lifecycle::answers(client).await && !daemon_lifecycle::stop(client, home).await?.ok {
        bail!("failed to stop the existing CCCC process");
    }
    Ok(())
}

/// Stop the embedded daemon and report why it ended, when it ended badly.
///
/// The reason is returned rather than printed because a daemon that failed
/// during startup ends here holding the only account of why the launch cannot
/// continue, and the caller is the one that knows whether this is a shutdown or
/// a failure.
#[cfg(not(windows))]
async fn finish_embedded_daemon(
    client: &DaemonClient,
    daemon: Option<tokio::task::JoinHandle<Result<()>>>,
) -> Option<anyhow::Error> {
    use crate::commands::common::call;

    let mut daemon = daemon?;
    if !daemon.is_finished() {
        // Bounded separately from the daemon's own shutdown: accepting the
        // request is quick even when acting on it is not, and the wait for it
        // to finish belongs to the join below. Without this bound the client's
        // default deadline would spend a minute here before that wait starts.
        let _ = tokio::time::timeout(
            DAEMON_SHUTDOWN_REQUEST_TIMEOUT,
            call(
                client,
                "shutdown",
                serde_json::json!({"expected_pid":std::process::id()}),
            ),
        )
        .await;
    }
    match tokio::time::timeout(daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT, &mut daemon).await {
        Ok(Ok(Err(error))) => Some(error),
        Ok(Err(error)) if !error.is_cancelled() => {
            Some(anyhow::anyhow!("embedded daemon task failed: {error}"))
        }
        Err(_) => {
            cccc_runtime::diagnostic(format_args!(
                "embedded daemon did not stop in time; cancelling it"
            ));
            daemon.abort();
            let _ = daemon.await;
            Some(anyhow::anyhow!(
                "embedded daemon did not finish shutdown before its deadline"
            ))
        }
        _ => None,
    }
}

async fn verify_daemon_stopped(home: &HomeLayout) -> Result<()> {
    if crate::daemon_takeover::wait_for_lock_release(
        home,
        daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT,
    )
    .await
    .is_ok()
    {
        return Ok(());
    }
    let owner = crate::daemon_takeover::daemon_pid(home)
        .map_or_else(|| "unknown PID".into(), |pid| format!("PID {pid}"));
    bail!(
        "the CCCC daemon started by this process ({owner}) is still running and still holds {}. Stop it with `cccc daemon stop` before starting CCCC again.",
        crate::daemon_takeover::lock_path(home).display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_enabled_interactive_normal_launch_uses_the_interface() {
        use cccc_web::WebMode::{Exhibit, Normal};
        assert!(use_terminal_interface(true, true, Normal));
        assert!(!use_terminal_interface(false, true, Normal));
        assert!(!use_terminal_interface(true, false, Normal));
        assert!(!use_terminal_interface(true, true, Exhibit));
    }
}
