//! Combined Web and daemon host lifecycle.

use anyhow::{Result, bail};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use std::time::Duration;

use crate::daemon_lifecycle::{self, DaemonSlot};
use crate::host_state::{
    AgentsResult, DaemonState, HostPhase, HostState, ShutdownReport, StopResult, UpdateResult,
    WebState,
};
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
        entry: Option<crate::update_checker::UpdateEntry>,
        update: Option<UpdateResult>,
    ) -> Result<Self> {
        let diagnostics =
            cccc_runtime::log_diagnostics_to(&home.daemon_dir().join("cccc-web.log"))?;
        let (state, receiver) = watch::channel(HostState {
            daemon,
            web: WebState::Starting,
            phase: HostPhase::Running,
            update,
        });
        let (intents, receiver_intents) = mpsc::unbounded_channel();
        control.connect(receiver_intents);
        let session = crate::tui::start(home.clone(), receiver, intents, entry).await?;
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

    /// Shows that an update is replacing this process, while the daemon stops.
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    fn updating(&self, notice: String) {
        self.state
            .send_modify(|state| state.phase = HostPhase::Updating { notice });
    }

    /// Closes the interface before this process is replaced: the terminal goes
    /// back to its own modes, and `message` says what starts next.
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    async fn finish_for_update(self, message: String) -> Result<()> {
        self.state.send_modify(|state| {
            state.web = WebState::Stopped;
            state.phase = HostPhase::Replaced { message };
        });
        self.session.finish().await
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

/// How a host launch ended: done, or replaced by the installed program to
/// finish an attended update.
pub(crate) enum LaunchExit {
    Done,
    Relay(Box<crate::update_host::Handoff>),
}

/// Carry out what the launch left to do once everything else has stopped.
pub(crate) async fn finish_launch(exit: LaunchExit) -> Result<()> {
    match exit {
        LaunchExit::Done => Ok(()),
        #[cfg(unix)]
        LaunchExit::Relay(handoff) => {
            let handoff = *handoff;
            crate::update_host::relay(&handoff.context, handoff.prepared, handoff.relay).await
        }
        #[cfg(not(unix))]
        LaunchExit::Relay(_) => bail!("this platform cannot hand an update over in place"),
    }
}

pub(crate) async fn launch(
    home: HomeLayout,
    host_override: Option<String>,
    port_override: Option<u16>,
    web_mode: Option<cccc_web::WebMode>,
    terminal: bool,
) -> Result<LaunchExit> {
    launch_with(
        home,
        host_override,
        port_override,
        web_mode,
        terminal,
        None,
        None,
    )
    .await
}

/// The previous program, started again after an update was given up. The
/// terminal interface says so once it opens.
pub(crate) async fn launch_after_abandoned(
    home: HomeLayout,
    host_override: Option<String>,
    port_override: Option<u16>,
    web_mode: Option<cccc_web::WebMode>,
    terminal: bool,
    result: UpdateResult,
) -> Result<LaunchExit> {
    launch_with(
        home,
        host_override,
        port_override,
        web_mode,
        terminal,
        None,
        Some(result),
    )
    .await
}

/// The program an attended update relayed to: start from the recorded launch
/// and confirm the update once the daemon is ready and the Web listens.
pub(crate) async fn launch_resumed(
    home: HomeLayout,
    host_override: Option<String>,
    port_override: Option<u16>,
    web_mode: Option<cccc_web::WebMode>,
    terminal: bool,
    resumed: crate::update_host::Resumed,
) -> Result<LaunchExit> {
    launch_with(
        home,
        host_override,
        port_override,
        web_mode,
        terminal,
        Some(resumed),
        None,
    )
    .await
}

async fn launch_with(
    home: HomeLayout,
    host_override: Option<String>,
    port_override: Option<u16>,
    web_mode: Option<cccc_web::WebMode>,
    terminal: bool,
    resumed: Option<crate::update_host::Resumed>,
    update_result: Option<UpdateResult>,
) -> Result<LaunchExit> {
    home.initialize()?;
    // Before anything that could stop another host: while an update holds this
    // home, only its own relay may start here.
    cccc_core::attended_update::check_start(
        &home,
        resumed
            .as_ref()
            .map(|resumed| resumed.transaction.dir.as_path()),
        &cccc_core::attended_update::BuildIdentity::current(),
    )?;
    let mut binding = crate::web_launch::resolve(&home, host_override.as_deref(), port_override)?;
    let client = DaemonClient::new(home.clone());
    let instance = claim_web_instance(&home, &client).await?;
    instance.hold_until_process_exit();
    let daemon_slot = daemon_lifecycle::resolve_slot(&home, &client).await?;
    let daemon_missing = daemon_slot == DaemonSlot::Vacant;
    // Only this update's relay may start the daemon, so another one running
    // means the update cannot be finished here.
    if resumed.is_some() && (!daemon_missing || cfg!(windows)) {
        bail!(
            "another CCCC daemon is running for CCCC_HOME={}; the update cannot be finished; recover with `{}`",
            home.root().display(),
            crate::update_host::recovery_command(&home)
        );
    }
    let mut terminal_control = if terminal {
        Some(crate::shutdown::TerminalControl::new(daemon_missing)?)
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
                return Ok(LaunchExit::Done);
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
        let update_resume = resumed
            .as_ref()
            .map(|resumed| resumed.transaction.dir.clone());
        embedded_daemon = Some(tokio::spawn(async move {
            let result = match update_resume {
                Some(transaction) => {
                    cccc_daemon::run_with_diagnostics_resuming(daemon_home, &transaction).await
                }
                None => cccc_daemon::run_with_diagnostics(daemon_home).await,
            };
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
            let cause = finish_embedded_daemon(&home, &client, embedded_daemon.take(), None).await;
            if terminal_control
                .as_ref()
                .is_some_and(|control| control.controller.is_requested())
            {
                if let Some(error) = cause {
                    return Err(error);
                }
                eprintln!("CCCC stopped during startup.");
                return Ok(LaunchExit::Done);
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
    let voice_recovery = crate::web_voice_recovery::new(&home, daemon_owned);

    let mode = web_mode.unwrap_or_else(cccc_web::WebMode::from_env);
    // An attended update replaces this process in place, so only a Linux or
    // macOS host that runs its own daemon offers one, with or without the terminal
    // interface: a standalone installation installs a release, and any
    // installation restarts into the program it was started from once that has
    // been rebuilt. It records that program now, before a build can replace it.
    let replaces_itself = cfg!(any(target_os = "linux", target_os = "macos"))
        && daemon_owned
        && mode == cccc_web::WebMode::Normal;
    let launch_program = replaces_itself
        .then(crate::update_checker::LaunchProgram::current)
        .flatten();
    let coordinator = replaces_itself
        .then(|| crate::update_host::UpdateContext::for_running_program(&home).ok())
        .flatten()
        .map(|context| crate::update_host::Coordinator::new(context, launch_program.clone()))
        .filter(|coordinator| {
            use cccc_web::UpdateHost;
            coordinator.release_in_place() || coordinator.local_restart()
        });
    // Any administrator of this Web can check for a newer release and for a
    // rebuilt program; only a host that can replace itself also offers to
    // carry either out.
    let update_checker: Option<cccc_web::UpdateCheckHook> = (mode == cccc_web::WebMode::Normal)
        .then(|| {
            std::sync::Arc::new(crate::update_checker::HostChecker::new(
                home.clone(),
                launch_program,
            )) as cccc_web::UpdateCheckHook
        });
    // The terminal interface starts an update through the Web serving now.
    let update_trigger = crate::update_checker::TriggerSlot::default();
    let update_entry = update_checker.clone().map(|checker| {
        crate::update_checker::UpdateEntry::new(
            checker,
            update_trigger.clone(),
            coordinator
                .as_ref()
                .is_some_and(cccc_web::UpdateHost::release_in_place),
            coordinator
                .as_ref()
                .is_some_and(cccc_web::UpdateHost::local_restart),
            coordinator
                .as_ref()
                .is_some_and(cccc_web::UpdateHost::release_keep),
        )
    });

    let (mut interface, startup_error) = if let Some(control) = terminal_control.as_mut() {
        if control.controller.is_requested() {
            (None, None)
        } else {
            match Interface::start(&home, daemon_state, control, update_entry, update_result)
                .await
            {
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

    // Where the Web actually listens, for the program an update relays to.
    let listening_port = std::sync::Arc::new(std::sync::atomic::AtomicU16::new(0));
    // The relayed program confirms its update once the Web listens again.
    let web_listening = resumed
        .as_ref()
        .map(|_| std::sync::Arc::new(tokio::sync::Notify::new()));
    let confirm_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let confirm_task = resumed.map(|resumed| {
        let listening = web_listening
            .clone()
            .expect("a resumed launch waits for its Web");
        let started = std::sync::Arc::clone(&confirm_started);
        let home = home.clone();
        // The terminal interface shows the result; without it, the log has it.
        let state = interface.as_ref().map(|interface| interface.state.clone());
        tokio::spawn(async move {
            listening.notified().await;
            started.store(true, std::sync::atomic::Ordering::Release);
            let confirmed =
                crate::update_host::confirm(&home, &resumed.transaction, &resumed.record).await;
            if let Some(state) = state {
                let record = &resumed.record;
                let local = record.source.release_dir().is_none();
                let result = match confirmed {
                    Ok(agents) => {
                        let done = if local {
                            format!("Restarted CCCC into the local build {}.", record.to_version)
                        } else {
                            format!(
                                "Updated CCCC {} to {}.",
                                record.from_version, record.to_version
                            )
                        };
                        let agents = crate::update_host::agents_line(&agents);
                        UpdateResult {
                            transaction: resumed.transaction.id.clone(),
                            failed: false,
                            message: if agents.is_empty() {
                                done
                            } else {
                                format!("{done} {agents}")
                            },
                        }
                    }
                    // The full problem, with its recovery command, is in the log;
                    // the notice is one line.
                    Err(_) => UpdateResult {
                        transaction: resumed.transaction.id.clone(),
                        failed: true,
                        message: format!(
                            "CCCC {} runs, but its {} was not confirmed; the CCCC log has the recovery command.",
                            record.to_version,
                            if local { "restart" } else { "update" }
                        ),
                    },
                };
                state.send_modify(|state| state.update = Some(result));
            }
        })
    });

    // The plain host retains its existing signal behavior, except that its first
    // interrupt requests the same stop as the TUI's controller. The TUI's
    // controller also receives raw key events and keeps running through Web
    // restarts. Its listener is registered here, before the Web starts.
    let owned_home = daemon_owned.then(|| home.clone());
    let plain_stop = (!terminal).then(crate::shutdown::Controller::new);
    #[cfg(windows)]
    let plain_watching = plain_stop.clone().map(|controller| {
        crate::shutdown::watch_for_interrupt(
            controller,
            owned_home,
            std::sync::Arc::clone(&detached_daemon),
        )
    });
    #[cfg(not(windows))]
    let plain_watching = plain_stop
        .clone()
        .map(|controller| crate::shutdown::watch_for_interrupt(controller, owned_home));
    let mut plain_watchdog = plain_watching.and_then(|watching| {
        watching
            .inspect_err(|error| {
                cccc_runtime::diagnostic(format_args!("could not listen for interrupts: {error}"))
            })
            .ok()
    });

    // A handover carries the update, and for one that keeps agents, the ID of
    // the stop the daemon already accepted.
    let loop_result: Result<Option<(crate::update_host::Prepared, Option<String>)>> = if let Some(
        error,
    ) =
        startup_error
    {
        Err(error)
    } else {
        loop {
            let monitored_home = home.clone();
            let mut daemon_exited = daemon_exit_rx.clone();
            let controller = terminal_control
                .as_ref()
                .map(|control| control.controller.clone())
                .or_else(|| plain_stop.clone());
            if controller
                .as_ref()
                .is_some_and(|controller| controller.is_requested())
            {
                break Ok(None);
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
            let on_listening: cccc_web::ListeningCallback = {
                let listening = interface.as_ref().map(Interface::listening);
                let signal = web_listening.clone();
                let port = std::sync::Arc::clone(&listening_port);
                let host = binding.host.clone();
                std::sync::Arc::new(move |address: std::net::SocketAddr| {
                    port.store(address.port(), std::sync::atomic::Ordering::Release);
                    match &listening {
                        Some(listening) => listening(address),
                        None => cccc_web::print_listening_banner(&host, address.port()),
                    }
                    if let Some(signal) = &signal {
                        signal.notify_one();
                    }
                })
            };
            let outcome = {
                let serving = cccc_web::serve_until_mode_supervised_with_voice_recovery(
                    home.clone(),
                    &binding.host,
                    binding.port,
                    mode,
                    shutdown,
                    Some(on_listening),
                    voice_recovery.clone(),
                    cccc_web::UpdateServices {
                        checker: update_checker.clone(),
                        host: coordinator
                            .as_ref()
                            .map(crate::update_host::Coordinator::hook),
                        on_trigger: Some(update_trigger.callback()),
                    },
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
            // That Web has closed: an update started through it now is given up.
            update_trigger.clear();
            let stop_requested = terminal_control
                .as_ref()
                .map(|control| &control.controller)
                .or(plain_stop.as_ref())
                .is_some_and(|controller| controller.is_requested());
            // Whatever the Web reserved and did not hand over is undone.
            if !matches!(outcome, Ok(cccc_web::ServeOutcome::UpdateRequested))
                && let Some(coordinator) = &coordinator
            {
                coordinator.close().await;
            }
            match outcome {
                Ok(cccc_web::ServeOutcome::Stopped(_)) => break Ok(None),
                Ok(cccc_web::ServeOutcome::UpdateRequested) => {
                    let Some(coordinator) = &coordinator else {
                        break Ok(None);
                    };
                    let prepared = coordinator.take_prepared();
                    match prepared {
                        // A stop that raced the handoff wins: nothing is replaced.
                        Some(prepared) if stop_requested => {
                            coordinator.undo(prepared).await;
                            break Ok(None);
                        }
                        Some(prepared) => {
                            // Keeping agents: the daemon checks every agent again
                            // and stops only if each one can be kept.
                            let started =
                                match crate::update_host::arm(coordinator.context(), &prepared) {
                                    Err(error) => Err(("arm", format!("{error:#}"), true)),
                                    Ok(()) if prepared.mode == cccc_web::UpdateMode::Keep => {
                                        match crate::update_host::request_keeping_stop(
                                            &client, &home, &prepared,
                                        )
                                        .await
                                        {
                                            crate::update_host::KeepingStop::Requested(id) => {
                                                Ok(Some(id))
                                            }
                                            crate::update_host::KeepingStop::Refused {
                                                reason,
                                                withdrawn,
                                            } => Err(("keep", reason, withdrawn)),
                                        }
                                    }
                                    Ok(()) => Ok(None),
                                };
                            match started {
                                Ok(preserving) => {
                                    // From here an interrupt must not force an exit or
                                    // stop agents halfway through the replacement.
                                    if let Some(watchdog) = plain_watchdog.take() {
                                        watchdog.abort();
                                    }
                                    if let Some(control) = terminal_control.as_mut() {
                                        control.stand_down();
                                    }
                                    break Ok(Some((prepared, preserving)));
                                }
                                // Nothing was stopped: report it and serve again.
                                Err((stage, reason, withdrawn)) => {
                                    cccc_runtime::diagnostic(format_args!(
                                        "[cccc] The {} did not start: {reason}",
                                        prepared.what()
                                    ));
                                    crate::update_host::record_not_started(
                                        coordinator.context(),
                                        &prepared,
                                        stage,
                                        &reason,
                                    );
                                    if let Some(interface) = &interface {
                                        let message = format!(
                                            "The {} did not start ({reason}). {} and no agent was stopped.",
                                            prepared.what(),
                                            prepared.unchanged()
                                        );
                                        interface.state.send_modify(|state| {
                                            state.update = Some(UpdateResult {
                                                transaction: prepared.transaction.id.clone(),
                                                failed: true,
                                                message,
                                            })
                                        });
                                    }
                                    if withdrawn {
                                        coordinator.undo(prepared).await;
                                    } else {
                                        coordinator.keep_for_recovery(prepared);
                                    }
                                    continue;
                                }
                            }
                        }
                        None if stop_requested => break Ok(None),
                        // Nothing prepared to finish: serve again.
                        None => continue,
                    }
                }
                Ok(cccc_web::ServeOutcome::RestartRequested) => {
                    if stop_requested {
                        break Ok(None);
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

    let (mut result, handover) = match loop_result {
        Ok(handover) => (Ok(()), handover),
        Err(error) => (Err(error), None),
    };
    if let Some((prepared, preserving)) = handover {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Some(coordinator) = &coordinator {
            // The relay reopens the Web where it listened, even when that came
            // from a saved binding or an ephemeral port rather than the options.
            let mut context = coordinator.context().clone();
            let port = match listening_port.load(std::sync::atomic::Ordering::Acquire) {
                0 => binding.port,
                port => port,
            };
            context.launch = crate::update_host::LaunchSpec::new(
                Some(binding.host.clone()),
                Some(port),
                Some(mode),
                terminal,
            );
            // The screen stays to show the update; the work it started on the
            // daemon ends before the daemon goes away.
            if let Some(interface) = interface.as_mut() {
                interface.updating(prepared.notice());
                interface.session.stop_work().await;
            }
            let exit = finish_handover(
                &home,
                &client,
                embedded_daemon.take(),
                context,
                prepared,
                preserving,
            )
            .await;
            // Whatever runs next replaces this process or ends it, so the
            // terminal interface closes first and gives the terminal back.
            if let Some(interface) = interface.take() {
                // An error is printed once, by `main`, with its recovery command.
                let message = match &exit {
                    Ok(LaunchExit::Relay(handoff)) => handoff.announcement(),
                    Ok(LaunchExit::Done) | Err(_) => String::new(),
                };
                if let Err(error) = interface.finish_for_update(message).await {
                    cccc_runtime::diagnostic(format_args!("{error:#}"));
                }
            }
            return exit;
        }
        // Only a Linux or macOS host with its own daemon offers updates.
        drop((prepared, preserving));
        bail!("this CCCC process cannot finish an update");
    }
    // A relay that never confirmed must not be cut off mid-confirm by shutdown.
    if let Some(task) = confirm_task {
        if confirm_started.load(std::sync::atomic::Ordering::Acquire) {
            let _ = task.await;
        } else {
            task.abort();
        }
    }

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
        daemon_result =
            match finish_embedded_daemon(&home, &client, embedded_daemon.take(), None).await {
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
    // The receipt of the daemon this launch started, whoever asked it to exit: this
    // launch's stop, or `cccc daemon stop [--keep-agents]` from another terminal. Only
    // where session owners run does a receipt list the agents.
    let agents = match owned_pid.filter(|_| cccc_daemon::session_owner::platform_supported()) {
        None => AgentsResult::NotOwned,
        Some(pid) => {
            let receipt = match cccc_daemon::session_owner::read_shutdown_report(&home, pid, None) {
                Ok(Some(report)) => daemon_lifecycle::Receipt::Confirmed(report),
                _ => daemon_lifecycle::Receipt::Missing,
            };
            if interface.is_none() {
                let intent = match &receipt {
                    daemon_lifecycle::Receipt::Confirmed(report)
                        if report.mode
                            == cccc_daemon::session_owner::ShutdownMode::PreserveAgents =>
                    {
                        daemon_lifecycle::StopIntent::KeepAgents
                    }
                    _ => daemon_lifecycle::StopIntent::StopAgents,
                };
                if let Some(line) = daemon_lifecycle::receipt_line(&home, intent, &receipt) {
                    cccc_runtime::diagnostic(format_args!("{line}"));
                }
            }
            daemon_lifecycle::agents_result(&receipt)
        }
    };
    if let Some(interface) = interface {
        let report = ShutdownReport {
            web: web_result,
            daemon: daemon_result,
            agents,
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
    result.map(|()| LaunchExit::Done)
}

/// Finish a handed-over update after the Web closed: stop this host's daemon
/// for the update, replace the files under the stopped daemon's lock, and
/// relay to the installed program. A stop or replacement that does not go
/// cleanly relays to the unchanged program instead, which starts as before.
/// A restart into a rebuilt program replaces no files, and the program at its
/// path is already the rebuilt one: when its stop does not go cleanly, the
/// host leaves its intent in place and names the recovery command.
/// `preserving` is the stop that keeps agents, already accepted by the daemon:
/// the host then only waits for the daemon to end.
#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn finish_handover(
    home: &HomeLayout,
    client: &DaemonClient,
    daemon: Option<tokio::task::JoinHandle<Result<()>>>,
    context: crate::update_host::UpdateContext,
    prepared: crate::update_host::Prepared,
    preserving: Option<String>,
) -> Result<LaunchExit> {
    use crate::update_host::{self, ApplyOutcome, RelayOutcome};
    use cccc_daemon::session_owner::ShutdownMode;

    cccc_mcp::shutdown(home).await;
    // The daemon binds its stop receipt to the request's ID, so only this
    // stop counts.
    let (request_id, mode, stop_error) = match preserving {
        Some(request_id) => {
            let stop_error = join_embedded_daemon(daemon).await;
            (request_id, ShutdownMode::PreserveAgents, stop_error)
        }
        None => {
            let request_id = uuid::Uuid::new_v4().to_string();
            let stop_error = finish_embedded_daemon(home, client, daemon, Some(&request_id)).await;
            (request_id, ShutdownMode::Stop, stop_error)
        }
    };
    let stopped = verify_daemon_stopped(home).await;
    let receipt = cccc_daemon::session_owner::read_shutdown_report(
        home,
        std::process::id(),
        Some(&request_id),
    );
    let (stop_problem, report) = match (stop_error, stopped, receipt) {
        (Some(error), _, _) | (None, Err(error), _) => (Some(format!("{error:#}")), None),
        (None, Ok(()), Ok(Some(report))) if report.mode != mode => (
            Some("the daemon did not stop the way this update asked".into()),
            None,
        ),
        // Agents a stop that keeps them could not confirm are named in the
        // result; a stop that ends them must confirm every one.
        (None, Ok(()), Ok(Some(report)))
            if report.unconfirmed.is_empty() || mode == ShutdownMode::PreserveAgents =>
        {
            (None, Some(report))
        }
        (None, Ok(()), Ok(Some(report))) => (
            Some(format!(
                "{} agent(s) could not be confirmed stopped; see `cccc daemon process-recovery`",
                report.unconfirmed.len()
            )),
            None,
        ),
        (None, Ok(()), Ok(None)) => (
            Some("the daemon left no stop receipt for this update".into()),
            None,
        ),
        (None, Ok(()), Err(error)) => (
            Some(format!("could not read the stop receipt: {error}")),
            None,
        ),
    };
    let abandon = |stage: &str, reason: String| RelayOutcome::Abandoned {
        stage: stage.into(),
        reason,
    };
    let stopped_halfway = |reason: String| {
        anyhow::anyhow!(
            "the {} stopped halfway ({reason}); recover with `{}`",
            prepared.what(),
            update_host::recovery_command(home)
        )
    };
    let (outcome, daemon_lock) = match stop_problem {
        Some(reason) if prepared.is_local() => return Err(stopped_halfway(reason)),
        Some(reason) => (abandon("stop", reason), None),
        None => match cccc_core::attended_update::claim_stopped(home, &prepared.transaction.id) {
            Err(error) if prepared.is_local() => return Err(stopped_halfway(error.to_string())),
            Err(error) => (abandon("claim", error.to_string()), None),
            Ok(lock) if prepared.is_local() => (RelayOutcome::LocalReady, Some(lock)),
            Ok(lock) => match update_host::apply(&context, &prepared, &lock).await {
                ApplyOutcome::Applied => (RelayOutcome::Applied, Some(lock)),
                ApplyOutcome::Restored { reason } => (abandon("apply", reason), Some(lock)),
                ApplyOutcome::Unknown { reason } => return Err(stopped_halfway(reason)),
            },
        },
    };
    if let RelayOutcome::Abandoned { stage, reason } = &outcome {
        cccc_runtime::diagnostic(format_args!(
            "[cccc] The update to CCCC {} was abandoned ({stage}): {reason}",
            prepared.version
        ));
        // Not applied yet: give the install lock and staged files back. After a
        // failed apply the installer already did.
        if stage != "apply"
            && let Err(error) = update_host::abort_unapplied(&context, &prepared).await
        {
            bail!(
                "the update to CCCC {} could not be undone ({error:#}); recover with `{}`",
                prepared.version,
                update_host::recovery_command(home)
            );
        }
        if let Err(error) = cccc_core::attended_update::disarm(home, &prepared.transaction.id) {
            bail!(
                "the update to CCCC {} left this home reserved ({error}); recover with `{}`",
                prepared.version,
                update_host::recovery_command(home)
            );
        }
    }
    // A relay that goes ahead carries the receipt: the program it starts looks
    // for the agents it names.
    let stopped = match &outcome {
        RelayOutcome::Abandoned { .. } => None,
        RelayOutcome::Applied | RelayOutcome::LocalReady => report,
    };
    if let Err(error) = update_host::write_relay(&context, &prepared, outcome.clone(), stopped) {
        bail!(
            "could not record the {} ({error:#}); recover with `{}`",
            prepared.what(),
            update_host::recovery_command(home)
        );
    }
    let relay = update_host::Relay {
        program: prepared.program(),
        transaction: prepared.transaction.clone(),
        daemon_lock,
    };
    Ok(LaunchExit::Relay(Box::new(update_host::Handoff {
        context,
        prepared,
        outcome,
        relay,
    })))
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
/// A request without an ID lets the daemon choose one; `null` is invalid.
#[cfg(not(windows))]
fn stop_request(request_id: Option<&str>) -> serde_json::Value {
    let mut request = serde_json::json!({"expected_pid":std::process::id()});
    if let Some(request_id) = request_id {
        request["request_id"] = serde_json::Value::from(request_id);
    }
    request
}

#[cfg(not(windows))]
async fn finish_embedded_daemon(
    home: &HomeLayout,
    client: &DaemonClient,
    daemon: Option<tokio::task::JoinHandle<Result<()>>>,
    request_id: Option<&str>,
) -> Option<anyhow::Error> {
    use crate::commands::common::call;

    let daemon = daemon?;
    // This host is stopping its own daemon, so the agents stop with it. Decide
    // that here rather than through the request below: a request queued behind
    // a long Group operation may still be waiting when the deadline cancels the
    // daemon, and a cancelled daemon with nothing chosen keeps its agents.
    #[cfg(unix)]
    cccc_daemon::session_owner::begin_owned_stop(home);
    #[cfg(not(unix))]
    let _ = home;
    if !daemon.is_finished() {
        // Bounded separately from the daemon's own shutdown: accepting the
        // request is quick even when acting on it is not, and the wait for it
        // to finish belongs to the join below. Without this bound the client's
        // default deadline would spend a minute here before that wait starts.
        let _ = tokio::time::timeout(
            DAEMON_SHUTDOWN_REQUEST_TIMEOUT,
            call(client, "shutdown", stop_request(request_id)),
        )
        .await;
    }
    join_embedded_daemon(Some(daemon)).await
}

/// Wait for the embedded daemon to end, cancelling it at the deadline, and
/// report why it ended, when it ended badly.
#[cfg(not(windows))]
async fn join_embedded_daemon(
    daemon: Option<tokio::task::JoinHandle<Result<()>>>,
) -> Option<anyhow::Error> {
    let mut daemon = daemon?;
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
