use std::future::Future;
use std::time::Duration;

const FORCE_EXIT_TIMEOUT: Duration = Duration::from_secs(15);
const INTERRUPTED_EXIT_CODE: i32 = 130;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForceExitReason {
    SecondInterrupt,
    Deadline,
}

pub(crate) async fn watch_for_interrupt() {
    if tokio::signal::ctrl_c().await.is_err() {
        return;
    }
    eprintln!("Stopping CCCC...");

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
