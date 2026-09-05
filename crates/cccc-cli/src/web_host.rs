//! Combined Web and daemon host lifecycle.

use anyhow::{Result, bail};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use std::time::Duration;

use crate::daemon_lifecycle::{self, DaemonSlot};
use crate::web_instance;

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
) -> Result<()> {
    home.initialize()?;
    let mut binding = crate::web_launch::resolve(&home, host_override.as_deref(), port_override)?;
    let client = DaemonClient::new(home.clone());
    let instance = claim_web_instance(&home, &client).await?;
    instance.hold_until_process_exit();
    let daemon_slot = daemon_lifecycle::resolve_slot(&home, &client).await?;
    let daemon_missing = daemon_slot == DaemonSlot::Vacant;

    // Running the daemon as a task inside the Web host is unreliable on
    // Windows once the daemon installs its kill-on-close Job object, so it
    // gets its own process there. Everywhere else it is a task in this one.
    #[cfg(windows)]
    let detached_daemon = crate::detached_daemon_owner::shared(if daemon_missing {
        crate::detached_daemon_owner::OwnedDetachedDaemon::start(&home, &client).await?
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
        let ready = daemon_lifecycle::wait_until_ready_or_owner_exits(
            &readiness,
            &mut daemon_exit_rx,
            DAEMON_READY_TIMEOUT,
        )
        .await;
        if !ready {
            let cause = finish_embedded_daemon(&client, embedded_daemon.take()).await;
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

    daemon_lifecycle::announce_ownership(&client, daemon_owned, daemon_slot).await;
    let owned_home = daemon_owned.then(|| home.clone());
    #[cfg(windows)]
    let shutdown_watchdog = tokio::spawn(crate::shutdown::watch_for_interrupt(
        owned_home,
        std::sync::Arc::clone(&detached_daemon),
    ));
    #[cfg(not(windows))]
    let shutdown_watchdog = tokio::spawn(crate::shutdown::watch_for_interrupt(owned_home));

    let mode = web_mode.unwrap_or_else(cccc_web::WebMode::from_env);
    let result = loop {
        let monitored_home = home.clone();
        let mut daemon_exited = daemon_exit_rx.clone();
        let shutdown = async move {
            tokio::select! {
                () = daemon_lifecycle::wait_for_loss(&monitored_home) => {}
                _ = daemon_exited.wait_for(|exited| *exited) => {}
            }
            eprintln!("CCCC daemon stopped; Web server closed");
        };
        match cccc_web::serve_until_mode_supervised(
            home.clone(),
            &binding.host,
            binding.port,
            mode,
            shutdown,
        )
        .await
        {
            Ok(cccc_web::ServeOutcome::Stopped(_)) => break Ok(()),
            Ok(cccc_web::ServeOutcome::RestartRequested) => {
                binding = match crate::web_launch::resolve(&home, None, None) {
                    Ok(binding) => binding,
                    Err(error) => break Err(error),
                };
                eprintln!(
                    "[cccc] Applying saved Web binding: http://{}:{}",
                    binding.host, binding.port
                );
            }
            Err(error) => break Err(error),
        }
    };

    cccc_mcp::shutdown(&home).await;
    #[cfg(windows)]
    if let Err(error) = crate::detached_daemon_owner::stop_shared(&detached_daemon, &client).await {
        eprintln!("failed to stop Web-owned daemon: {error}");
    }
    #[cfg(not(windows))]
    if let Some(error) = finish_embedded_daemon(&client, embedded_daemon.take()).await {
        eprintln!("embedded daemon stopped: {error:#}");
    }
    if daemon_owned {
        report_surviving_daemon(&home).await;
    }
    shutdown_watchdog.abort();
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
            call(client, "shutdown", serde_json::json!({})),
        )
        .await;
    }
    match tokio::time::timeout(daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT, &mut daemon).await {
        Ok(Ok(Err(error))) => Some(error),
        Ok(Err(error)) if !error.is_cancelled() => {
            Some(anyhow::anyhow!("embedded daemon task failed: {error}"))
        }
        Err(_) => {
            eprintln!("embedded daemon did not stop in time; cancelling it");
            daemon.abort();
            let _ = daemon.await;
            None
        }
        _ => None,
    }
}

async fn report_surviving_daemon(home: &HomeLayout) {
    if crate::daemon_takeover::wait_for_lock_release(
        home,
        daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT,
    )
    .await
    .is_ok()
    {
        return;
    }
    let owner = crate::daemon_takeover::daemon_pid(home)
        .map_or_else(|| "unknown PID".into(), |pid| format!("PID {pid}"));
    eprintln!(
        "warning: the CCCC daemon started by this process ({owner}) is still running and still holds {}.\nStop it with `cccc daemon stop` before starting CCCC again.",
        crate::daemon_takeover::lock_path(home).display()
    );
}
