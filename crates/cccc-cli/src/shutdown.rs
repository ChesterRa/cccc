use cccc_core::HomeLayout;
use std::future::Future;
use std::time::Duration;

// Stopping every runtime takes as long as the runtimes take, and measured
// against a real home that is many seconds. A deadline shorter than an ordinary
// shutdown turns the normal path into the forced one, which is how actors get
// orphaned. This is only the backstop for a host nobody is watching -- an
// operator in a hurry presses Ctrl-C again and never waits it out.
const FORCE_EXIT_TIMEOUT: Duration = Duration::from_secs(60);
const INTERRUPTED_EXIT_CODE: i32 = 130;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForceExitReason {
    SecondInterrupt,
    Deadline,
}

/// Escalate to OS process-tree termination without entering normal cleanup.
pub(crate) async fn watch_for_interrupt(
    _home: Option<HomeLayout>,
    #[cfg(windows)] _detached_daemon: crate::detached_daemon_owner::SharedOwnedDetachedDaemon,
) {
    if tokio::signal::ctrl_c().await.is_err() {
        return;
    }
    eprintln!("Stopping CCCC... (press Ctrl-C again to stop immediately)");

    let reason = force_exit_reason(
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
        tokio::time::sleep(FORCE_EXIT_TIMEOUT),
    )
    .await;
    match reason {
        ForceExitReason::SecondInterrupt => {
            eprintln!("Second interrupt received; forcing CCCC to stop immediately");
        }
        ForceExitReason::Deadline => {
            eprintln!(
                "CCCC did not stop within {} seconds; forcing exit",
                FORCE_EXIT_TIMEOUT.as_secs()
            );
        }
    }
    force_exit();
}

fn force_exit() -> ! {
    if let Err(error) = cccc_runtime::force_terminate_owned() {
        eprintln!("could not terminate every owned process tree: {error}");
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
    use super::{ForceExitReason, force_exit_reason};
    use std::future::{pending, ready};

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
