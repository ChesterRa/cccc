use super::*;
use crate::dispatch_concurrency::DispatchLocks;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::task::JoinHandle;

const INTERVAL: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

pub(crate) struct ReachRestore {
    cancelled: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

impl ReachRestore {
    pub(crate) fn start(home: HomeLayout, locks: DispatchLocks) -> Self {
        Self::start_with(home, locks, |home, locks, cancelled| {
            restore_once(
                home,
                locks,
                cancelled,
                web_runtime::live_web_port,
                |home| {
                    membership_cloudflared::installed_binary(home)
                        .map(|_| ())
                        .map_err(runtime_error)
                },
                |home, token| membership_cloudflared::start(home, token).map(|_| ()),
            )
        })
    }

    fn start_with(
        home: HomeLayout,
        locks: DispatchLocks,
        restore: fn(&HomeLayout, &DispatchLocks, &AtomicBool) -> Result<(), OpError>,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = tokio::spawn(async move {
            let mut backoff = INTERVAL;
            loop {
                let home = home.clone();
                let locks = locks.clone();
                let cancelled = worker_cancelled.clone();
                let result =
                    tokio::task::spawn_blocking(move || restore(&home, &locks, &cancelled)).await;
                let delay = if matches!(result, Ok(Ok(()))) {
                    backoff = INTERVAL;
                    INTERVAL
                } else {
                    let delay = backoff;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    delay
                };
                tokio::time::sleep(delay).await;
            }
        });
        Self { cancelled, task }
    }
}

impl Drop for ReachRestore {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.task.abort();
        // A bounded account request can still finish in spawn_blocking. Its
        // commit checks cancellation and the daemon's runtime start gate.
    }
}

#[derive(PartialEq)]
struct Intent {
    remote: Map<String, Value>,
    account_origin: String,
    device_id: Option<String>,
    device_token: String,
}

fn intent(home: &HomeLayout) -> Result<Option<Intent>, OpError> {
    let remote = settings::load(home).map_err(OpError::io)?.remote_access;
    if text(&remote, "provider", "off") != "reach" || !boolean(&remote, "enabled", false) {
        return Ok(None);
    }
    let state = membership::load(home).map_err(OpError::io)?;
    if !state.logged_in || state.disabled {
        return Ok(None);
    }
    let Some(device_token) = state
        .device_token
        .as_ref()
        .filter(|token| !token.is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(Intent {
        remote,
        account_origin: bound_account_origin(&state).map_err(|error| account_fail(home, error))?,
        device_id: state.device_id,
        device_token: device_token.clone(),
    }))
}

fn restore_once(
    home: &HomeLayout,
    locks: &DispatchLocks,
    cancelled: &AtomicBool,
    resolve_port: impl Fn(&HomeLayout) -> Result<u16, OpError>,
    check_helper: impl FnOnce(&HomeLayout) -> Result<(), OpError>,
    start_helper: impl FnOnce(&HomeLayout, &str) -> Result<(), RuntimeError>,
) -> Result<(), OpError> {
    // Never queue a background global writer, and never hold a permit during
    // account I/O. This also keeps a slow account from delaying local controls.
    let Some(permit) = locks.try_global_write() else {
        return Ok(());
    };
    if cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let Some(expected) = intent(home)? else {
        return Ok(());
    };
    if membership_cloudflared::status(home).running {
        return Ok(());
    }
    validate_reach_access(home).inspect_err(|error| {
        let _ = remember_error(home, &error.message);
    })?;
    drop(permit);

    // The Web host can start later than the daemon. Use its signed live binding,
    // never the configured/default port. Automatic restore does not download or
    // upgrade helpers; start performs the same pinned-binary check as manual on.
    let prepared = (|| {
        let port = resolve_port(home)?;
        check_helper(home)?;
        let client = AccountClient::with_timeout(&expected.account_origin, Some(15.0))
            .map_err(|error| OpError::new(error.code, error.message))?;
        let credentials = client
            .issue_reach(&expected.device_token, port)
            .map_err(|error| OpError::new(error.code, error.message))?;
        Ok::<_, OpError>((port, credentials))
    })();

    let Some(_permit) = locks.try_global_write() else {
        return Ok(());
    };
    if cancelled.load(Ordering::Acquire) || intent(home)?.as_ref() != Some(&expected) {
        return Ok(());
    }
    if membership_cloudflared::status(home).running {
        return Ok(());
    }
    let result = (|| {
        let (port, credentials) = prepared?;
        validate_reach_access(home)?;
        if resolve_port(home)? != port {
            return Err(OpError::new(
                "membership_gate",
                "CCCC Web binding changed during Reach restore; waiting for the next attempt",
            ));
        }
        let _start = crate::runtime_start_gate::permit(home)
            .map_err(|message| OpError::new("membership_gate", message))?;
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        commit_reach_start(home, &credentials, start_helper)
    })();
    if let Err(error) = &result {
        if matches!(
            error.code.as_str(),
            "membership_disabled" | "membership_not_logged_in"
        ) {
            mark_cut(home, None, None)?;
        }
        let _ = remember_error(home, &error.message);
    }
    result
}

#[cfg(test)]
#[path = "tests/restore.rs"]
mod tests;
