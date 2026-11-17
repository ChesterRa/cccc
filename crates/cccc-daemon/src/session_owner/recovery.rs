use super::RecoveryAction;
use super::transport::Paths;
use cccc_core::HomeLayout;
use cccc_runtime::process_recovery;
use serde_json::{Value, json};
use std::io;

/// Caller holds the dead owner's exclusive instance lock and the home lock.
pub(super) fn reconcile(paths: &Paths) -> io::Result<bool> {
    let ledger = paths.directory.join("owned-process-groups.json");
    cccc_runtime::recover_inactive_owned_process_groups(&ledger)?;
    let snapshot = process_recovery::list_locked_ledger(&ledger).map_err(io::Error::other)?;
    if snapshot.records.is_empty() && snapshot.diagnostics.is_empty() {
        retire(paths)?;
        return Ok(true);
    }
    Ok(false)
}

fn retire(paths: &Paths) -> io::Result<()> {
    for path in [&paths.address, &paths.socket] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn recover(
    home: &HomeLayout,
    owner_id: &str,
    action: RecoveryAction,
) -> anyhow::Result<Value> {
    let paths = Paths::new(home, owner_id)?;
    if !paths.directory.is_dir() {
        anyhow::bail!("session owner does not exist");
    }
    let _home_lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "first run `cccc daemon stop --keep-agents`, then review the session owner",
            )
        })?;
    let owner_lock = cccc_core::fs::try_exclusive_lock(&paths.lock)?;
    if owner_lock.is_none() {
        let client = super::client::Client::connect(super::client::load(home, owner_id)?)?;
        // The service independently takes this same home lock before review.
        drop(_home_lock);
        return client.recover(action).map_err(Into::into);
    }
    let _owner_lock = owner_lock;
    let ledger = paths.directory.join("owned-process-groups.json");
    let mut changed = false;
    match action {
        RecoveryAction::List => {}
        RecoveryAction::Retry { id, version } => {
            process_recovery::retry_locked_ledger(&ledger, &id, &version)?;
            changed = true;
        }
        RecoveryAction::Accept { id, version } => {
            process_recovery::accept_locked_ledger(&ledger, &id, &version)?;
            changed = true;
        }
    }
    let snapshot = process_recovery::list_locked_ledger(&ledger)?;
    if changed && snapshot.records.is_empty() && snapshot.diagnostics.is_empty() {
        retire(&paths)?;
    }
    Ok(json!({"owner":"session", "owner_id":paths.owner_id,
        "records":snapshot.records, "diagnostics":snapshot.diagnostics,
        "retired":!paths.address.exists()}))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn empty_dead_owner_retires_but_unknown_detached_work_needs_explicit_acceptance() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
        home.initialize().expect("initialize fixture state");
        let paths = Paths::new(&home, &uuid::Uuid::new_v4().simple().to_string())
            .expect("resolve owner paths");
        paths.initialize().expect("initialize fixture state");
        std::fs::write(&paths.address, b"retained evidence").expect("write fixture file");
        let ledger = paths.directory.join("owned-process-groups.json");
        std::fs::write(&ledger, b"[]").expect("write fixture file");
        {
            let _home = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
                .expect("check fixture lock")
                .expect("fixture lock is available");
            let _owner = cccc_core::fs::try_exclusive_lock(&paths.lock)
                .expect("check fixture lock")
                .expect("fixture lock is available");
            assert!(reconcile(&paths).expect("reconcile inactive owner evidence"));
        }
        assert!(!paths.address.exists());
        std::fs::write(&paths.address, b"retained evidence").expect("write fixture file");
        cccc_core::fs::write_json(&ledger, &json!({"version":2,"records":[{"type":"launch_intent","evidence":{"id":uuid::Uuid::new_v4(),"created_at":1,"origin":null}}]})).expect("write recovery fixture");
        {
            let _home = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
                .expect("check fixture lock")
                .expect("fixture lock is available");
            let _owner = cccc_core::fs::try_exclusive_lock(&paths.lock)
                .expect("check fixture lock")
                .expect("fixture lock is available");
            assert!(
                reconcile(&paths).is_err(),
                "unknown work must keep recovery pending"
            );
            assert!(
                paths.address.exists(),
                "unconfirmed work retains its address"
            );
            assert!(recover(&home, &paths.owner_id, RecoveryAction::List).is_err());
        }
        let listed = recover(&home, &paths.owner_id, RecoveryAction::List)
            .expect("review owner recovery records");
        assert_eq!(listed["records"].as_array().expect("array result").len(), 1);
        let item = &listed["records"][0];
        let id = item["id"].as_str().expect("string result").to_owned();
        let version = item["version"].as_str().expect("string result").to_owned();
        assert!(
            recover(
                &home,
                &paths.owner_id,
                RecoveryAction::Accept {
                    id: id.clone(),
                    version: "stale".into()
                }
            )
            .is_err()
        );
        assert!(paths.address.exists());
        let accepted = recover(
            &home,
            &paths.owner_id,
            RecoveryAction::Accept { id, version },
        )
        .expect("review owner recovery records");
        assert_eq!(accepted["retired"], true);
        assert!(
            accepted["records"]
                .as_array()
                .expect("array result")
                .is_empty()
        );
    }
}
