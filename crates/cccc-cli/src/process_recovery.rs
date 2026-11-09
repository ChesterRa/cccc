use anyhow::{Context, Result, bail};
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use fs2::FileExt;
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};

use crate::args::{RecoveryAction, RecoveryOwner};

pub(crate) async fn run(
    home: HomeLayout,
    client: &DaemonClient,
    owner: RecoveryOwner,
    action: RecoveryAction,
) -> Result<()> {
    if matches!(
        &action,
        RecoveryAction::Accept {
            accept_risk: false,
            ..
        }
    ) {
        bail!("accepting a record requires --accept-risk; old work may still be running");
    }
    let lock_home = home.clone();
    let lock = tokio::task::spawn_blocking(move || try_owner_lock(&lock_home, owner)).await??;
    if let Some(lock) = lock {
        let value = tokio::task::spawn_blocking(move || {
            let _lock = lock;
            offline(&home, owner, action)
        })
        .await??;
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    if matches!(owner, RecoveryOwner::Web) {
        // The live-binding lookup verifies the Web runtime's identity. It can
        // perform I/O, and its local address need not be reachable remotely.
        let binding = tokio::task::spawn_blocking(move || cccc_daemon::live_web_binding(&home))
            .await
            .ok()
            .flatten();
        bail!("{}", web_running_message(binding));
    }
    let (op, args) = match action {
        RecoveryAction::List => ("owned_process_recovery_list", json!({"by":"user"})),
        RecoveryAction::Retry { id, version } => (
            "owned_process_recovery_retry",
            json!({"by":"user","id":id,"version":version}),
        ),
        RecoveryAction::Accept { id, version, .. } => (
            "owned_process_recovery_accept",
            json!({"by":"user","id":id,"version":version}),
        ),
    };
    // A timeout after exchange began is outcome-unknown. Never retry that
    // mutation through an offline path; the user can list the records again.
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("fixed object").clone(),
        })
        .await?;
    crate::commands::common::print(response)
}

fn web_running_message(binding: Option<(String, u16)>) -> String {
    let local_address = binding.map_or_else(String::new, |(host, port)| {
        format!(
            " Its verified local binding is {} (remote access may use a different address).",
            crate::web_endpoint::format(&host, port)
        )
    });
    format!(
        "CCCC Web is running for this home.{local_address} Use its private recovery view, or stop it before offline review."
    )
}

pub(crate) fn try_owner_lock(home: &HomeLayout, owner: RecoveryOwner) -> Result<Option<File>> {
    std::fs::create_dir_all(home.daemon_dir())?;
    let path = match owner {
        RecoveryOwner::Daemon => cccc_daemon::DaemonPaths::new(home.clone()).lock,
        RecoveryOwner::Web => home.daemon_dir().join("cccc-web.lock"),
    };
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .context("open process owner lock")?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(file)),
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn offline(
    home: &HomeLayout,
    owner: RecoveryOwner,
    action: RecoveryAction,
) -> Result<Value> {
    let (owner_name, ledger) = match owner {
        RecoveryOwner::Daemon => (
            "daemon",
            home.daemon_dir().join("owned-process-groups.json"),
        ),
        RecoveryOwner::Web => (
            "web",
            home.root()
                .join("web-processes")
                .join("owned-process-groups.json"),
        ),
    };
    match action {
        RecoveryAction::List => {
            let snapshot = cccc_runtime::process_recovery::list_locked_ledger(&ledger)?;
            Ok(
                json!({"owner":owner_name,"records":snapshot.records,"diagnostics":snapshot.diagnostics}),
            )
        }
        RecoveryAction::Retry { id, version } => {
            let result =
                cccc_runtime::process_recovery::retry_locked_ledger(&ledger, &id, &version)?;
            let snapshot = cccc_runtime::process_recovery::list_locked_ledger(&ledger)?;
            let record = snapshot.records.into_iter().find(|record| record.id == id);
            Ok(
                json!({"owner":owner_name,"result":result,"record":record,"diagnostics":snapshot.diagnostics}),
            )
        }
        RecoveryAction::Accept { id, version, .. } => {
            cccc_runtime::process_recovery::accept_locked_ledger(&ledger, &id, &version)?;
            Ok(json!({"owner":owner_name,"accepted":true}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On Unix, a child spawned by a concurrently running test can keep an
    /// inherited copy of the lock's descriptor until it execs, and the flock
    /// with it, so a released lock can stay busy until that copy closes.
    fn reclaim<T>(mut attempt: impl FnMut() -> Option<T>) -> Option<T> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let claimed = attempt();
            if claimed.is_some() || std::time::Instant::now() >= deadline {
                return claimed;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn offline_review_never_takes_an_active_owners_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        for owner in [RecoveryOwner::Daemon, RecoveryOwner::Web] {
            let held = try_owner_lock(&home, owner)
                .expect("claim")
                .expect("available");
            assert!(
                try_owner_lock(&home, owner)
                    .expect("test contention")
                    .is_none()
            );
            drop(held);
            assert!(reclaim(|| try_owner_lock(&home, owner).expect("reclaim")).is_some());
        }
    }

    #[test]
    fn running_web_hint_does_not_assume_a_remote_address() {
        let message = web_running_message(Some(("0.0.0.0".into(), 8848)));
        assert!(message.contains("http://127.0.0.1:8848"));
        assert!(message.contains("remote access may use a different address"));
        assert!(web_running_message(None).contains("stop it before offline review"));
    }
}
