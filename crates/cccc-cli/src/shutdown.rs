use cccc_core::HomeLayout;
use std::future::Future;
use std::time::Duration;
use tokio::sync::watch;

// Stopping every runtime takes as long as the runtimes take, and measured
// against a real home that is many seconds. A deadline shorter than an ordinary
// shutdown turns the normal path into the forced one, which is how actors get
// orphaned. This is only the backstop for a host nobody is watching -- an
// operator in a hurry presses Ctrl-C again and never waits it out.
const FORCE_EXIT_TIMEOUT: Duration = Duration::from_secs(60);
// Agent View sessions are not owned process trees, so force_terminate_owned
// leaves them running. One bounded round of stop requests keeps them from
// being stranded without making the forced exit wait for confirmations.
const FORCE_STOP_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const INTERRUPTED_EXIT_CODE: i32 = 130;

#[derive(Clone, Copy, Debug, Default)]
struct StopState {
    requested: bool,
    interrupts: u8,
}

#[derive(Clone)]
pub(crate) struct Controller {
    state: watch::Sender<StopState>,
}

/// Tasks belong to the foreground launch, including its startup/error paths.
/// Dropping the launch must not leave a signal listener or force-exit timer.
pub(crate) struct TerminalControl {
    pub controller: Controller,
    signals: tokio::task::JoinHandle<()>,
    watchdog: tokio::task::JoinHandle<()>,
    intents: Option<tokio::task::JoinHandle<()>>,
}

impl TerminalControl {
    pub(crate) fn new() -> std::io::Result<Self> {
        let controller = Controller::new();
        let signals = watch_signals(controller.clone())?;
        let watchdog = tokio::spawn(watch_terminal_shutdown(controller.clone()));
        Ok(Self {
            controller,
            signals,
            watchdog,
            intents: None,
        })
    }

    pub(crate) fn connect(
        &mut self,
        mut intents: tokio::sync::mpsc::UnboundedReceiver<crate::host_state::HostIntent>,
    ) {
        let controller = self.controller.clone();
        self.intents = Some(tokio::spawn(async move {
            while let Some(intent) = intents.recv().await {
                match intent {
                    crate::host_state::HostIntent::Quit => controller.request(),
                    crate::host_state::HostIntent::Interrupt => controller.interrupt(),
                }
            }
        }));
    }
}

impl Drop for TerminalControl {
    fn drop(&mut self) {
        self.signals.abort();
        self.watchdog.abort();
        if let Some(intents) = &self.intents {
            intents.abort();
        }
    }
}

impl Controller {
    pub(crate) fn new() -> Self {
        let (state, _) = watch::channel(StopState::default());
        Self { state }
    }

    pub(crate) fn request(&self) {
        self.state.send_if_modified(|state| {
            let changed = !state.requested;
            state.requested = true;
            changed
        });
    }

    pub(crate) fn interrupt(&self) {
        self.state.send_modify(|state| {
            state.requested = true;
            state.interrupts = state.interrupts.saturating_add(1);
        });
    }

    pub(crate) fn is_requested(&self) -> bool {
        self.state.borrow().requested
    }

    pub(crate) async fn requested(&self) {
        let _ = self
            .state
            .subscribe()
            .wait_for(|state| state.requested)
            .await;
    }

    async fn forced(&self) {
        let _ = self
            .state
            .subscribe()
            .wait_for(|state| state.interrupts >= 2)
            .await;
    }
}

/// Terminal key events and system signals enter the same foreground lifetime.
pub(crate) fn watch_signals(
    controller: Controller,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate())?;
        let mut hangup = signal(SignalKind::hangup())?;
        Ok(tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => {
                        if result.is_err() { return; }
                        controller.interrupt();
                    }
                    event = terminate.recv() => if event.is_some() { controller.request(); } else { return; },
                    event = hangup.recv() => if event.is_some() { controller.request(); } else { return; },
                }
            }
        }))
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};
        let mut interrupt = ctrl_c()?;
        let mut r#break = ctrl_break()?;
        let mut close = ctrl_close()?;
        let mut logoff = ctrl_logoff()?;
        let mut shutdown = ctrl_shutdown()?;
        Ok(tokio::spawn(async move {
            loop {
                tokio::select! {
                    event = interrupt.recv() => if event.is_some() { controller.interrupt(); } else { return; },
                    event = r#break.recv() => if event.is_some() { controller.request(); } else { return; },
                    event = close.recv() => if event.is_some() { controller.request(); } else { return; },
                    event = logoff.recv() => if event.is_some() { controller.request(); } else { return; },
                    event = shutdown.recv() => if event.is_some() { controller.request(); } else { return; },
                }
            }
        }))
    }
    #[cfg(not(any(unix, windows)))]
    Ok(tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            controller.interrupt();
        }
    }))
}

pub(crate) async fn watch_terminal_shutdown(controller: Controller) {
    controller.requested().await;
    let reason =
        force_exit_reason(controller.forced(), tokio::time::sleep(FORCE_EXIT_TIMEOUT)).await;
    cccc_runtime::diagnostic(format_args!(
        "forcing CCCC to exit: {reason:?}; graceful cleanup was not confirmed"
    ));
    let _ = tokio::time::timeout(
        FORCE_STOP_REQUEST_TIMEOUT,
        cccc_daemon::request_managed_session_stop(),
    )
    .await;
    restore_terminal_before_exit();
    force_exit();
}

fn restore_terminal_before_exit() {
    let (done, completed) = std::sync::mpsc::sync_channel(1);
    let _ = std::thread::Builder::new()
        .name("cccc-terminal-restore".into())
        .spawn(move || {
            #[cfg(windows)]
            crate::console_encoding::restore_original();
            let _ = crate::tui::restore_terminal();
            eprintln!("CCCC is forcing exit; graceful service cleanup was not confirmed.");
            let _ = done.send(());
        });
    let _ = completed.recv_timeout(Duration::from_millis(300));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForceExitReason {
    SecondInterrupt,
    Deadline,
}

/// Bound provider stop requests, then terminate owned process trees without
/// entering normal cleanup or acquiring managed-session stop locks.
pub(crate) async fn watch_for_interrupt(
    _home: Option<HomeLayout>,
    #[cfg(windows)] _detached_daemon: crate::detached_daemon_owner::SharedOwnedDetachedDaemon,
) {
    if tokio::signal::ctrl_c().await.is_err() {
        return;
    }
    eprintln!("Stopping CCCC... (press Ctrl-C again to force exit)");

    let reason = force_exit_reason(
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
        tokio::time::sleep(FORCE_EXIT_TIMEOUT),
    )
    .await;
    match reason {
        ForceExitReason::SecondInterrupt => {
            eprintln!("Second interrupt received; forcing CCCC to stop");
        }
        ForceExitReason::Deadline => {
            eprintln!(
                "CCCC did not stop within {} seconds; forcing exit",
                FORCE_EXIT_TIMEOUT.as_secs()
            );
        }
    }
    let _ = tokio::time::timeout(
        FORCE_STOP_REQUEST_TIMEOUT,
        cccc_daemon::request_managed_session_stop(),
    )
    .await;
    force_exit();
}

fn force_exit() -> ! {
    if let Err(error) = cccc_runtime::force_terminate_owned() {
        cccc_runtime::diagnostic(format_args!(
            "could not terminate every owned process tree: {error}"
        ));
    }
    std::process::exit(INTERRUPTED_EXIT_CODE);
}

async fn force_exit_reason<S, D>(second_interrupt: S, deadline: D) -> ForceExitReason
where
    S: Future<Output = ()>,
    D: Future<Output = ()>,
{
    tokio::select! {
        _ = second_interrupt => ForceExitReason::SecondInterrupt,
        _ = deadline => ForceExitReason::Deadline,
    }
}

#[cfg(test)]
mod tests {
    use super::{Controller, ForceExitReason, force_exit_reason};
    use std::future::{pending, ready};

    #[tokio::test]
    async fn repeated_regular_requests_never_force_exit() {
        let controller = Controller::new();
        controller.request();
        controller.request();
        controller.requested().await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), controller.forced())
                .await
                .is_err()
        );
        controller.interrupt();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), controller.forced())
                .await
                .is_err()
        );
        controller.interrupt();
        controller.forced().await;
    }

    #[tokio::test]
    async fn first_interrupt_requests_shutdown_second_interrupt_forces_it() {
        let controller = Controller::new();
        assert!(!controller.is_requested());
        controller.interrupt();
        controller.requested().await;
        assert_eq!(controller.state.borrow().interrupts, 1);
        controller.interrupt();
        controller.forced().await;
    }

    #[tokio::test]
    async fn second_interrupt_forces_exit_before_the_deadline() {
        assert_eq!(
            force_exit_reason(ready(()), pending()).await,
            ForceExitReason::SecondInterrupt
        );
    }

    #[tokio::test]
    async fn deadline_forces_exit_without_a_second_interrupt() {
        assert_eq!(
            force_exit_reason(pending(), ready(())).await,
            ForceExitReason::Deadline
        );
    }
}

#[cfg(all(test, unix))]
#[path = "shutdown_process_tests.rs"]
mod process_tests;
