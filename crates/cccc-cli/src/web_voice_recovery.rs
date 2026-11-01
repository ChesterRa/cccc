use cccc_core::HomeLayout;
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};

/// Constructed only after the normal Web host has claimed its instance lock.
pub(crate) fn new(home: &HomeLayout, daemon_owned: bool) -> cccc_web::VoiceProcessRecovery {
    #[cfg(unix)]
    {
        // A separate directory prevents either guard's watch-list cleanup from
        // matching the other owner's files.
        let ledger = home
            .root()
            .join("web-processes")
            .join("owned-process-groups.json");
        let recovery_ledger = ledger.clone();
        let unconfirmed = Arc::new(AtomicBool::new(false));
        let reconcile_unconfirmed = Arc::clone(&unconfirmed);
        let reconcile = Arc::new(move || {
            let result = cccc_runtime::prepare_web_process_guard(&recovery_ledger, !daemon_owned);
            reconcile_unconfirmed.store(result.is_err(), Ordering::Release);
            if let Err(error) = &result {
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice process recovery pending in CCCC_HOME/web-processes: {error}"
                ));
            }
            result
        });
        let review = Arc::new(move |request| {
            use cccc_runtime::process_recovery as recovery;
            use cccc_web::VoiceRecoveryRequest;
            use serde_json::json;

            match request {
                VoiceRecoveryRequest::List => {
                    let mut snapshot = recovery::list_owned_ledger(&ledger)?;
                    annotate_empty_unconfirmed(&mut snapshot, unconfirmed.load(Ordering::Acquire));
                    Ok(
                        json!({"owner":"web","records":snapshot.records,"diagnostics":snapshot.diagnostics}),
                    )
                }
                VoiceRecoveryRequest::Retry { id, version } => {
                    let result = recovery::retry_owned_ledger(&ledger, &id, &version)?;
                    let mut snapshot = recovery::list_owned_ledger(&ledger)?;
                    annotate_empty_unconfirmed(&mut snapshot, unconfirmed.load(Ordering::Acquire));
                    let record = snapshot.records.into_iter().find(|record| record.id == id);
                    Ok(
                        json!({"owner":"web","result":result,"record":record,"diagnostics":snapshot.diagnostics}),
                    )
                }
                VoiceRecoveryRequest::Accept { id, version } => {
                    recovery::accept_owned_ledger(&ledger, &id, &version)?;
                    Ok(json!({"owner":"web","accepted":true}))
                }
            }
        });
        let daemon_home = home.clone();
        let review_daemon_offline = Arc::new(move |request| {
            use crate::args::{RecoveryAction, RecoveryOwner};
            use cccc_runtime::process_recovery::RecoveryError;
            use cccc_web::VoiceRecoveryRequest;

            let Some(lock) =
                crate::process_recovery::try_owner_lock(&daemon_home, RecoveryOwner::Daemon)
                    .map_err(|_| RecoveryError::Unavailable)?
            else {
                return Ok(None);
            };
            let _lock = lock;
            let action = match request {
                VoiceRecoveryRequest::List => RecoveryAction::List,
                VoiceRecoveryRequest::Retry { id, version } => {
                    RecoveryAction::Retry { id, version }
                }
                VoiceRecoveryRequest::Accept { id, version } => RecoveryAction::Accept {
                    id,
                    version,
                    accept_risk: true,
                },
            };
            crate::process_recovery::offline(&daemon_home, RecoveryOwner::Daemon, action)
                .map(Some)
                .map_err(|error| {
                    error
                        .downcast_ref::<RecoveryError>()
                        .copied()
                        .unwrap_or(RecoveryError::Unavailable)
                })
        });
        cccc_web::VoiceProcessRecovery {
            reconcile,
            review,
            review_daemon_offline,
            check_daemon_guard: true,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (home, daemon_owned);
        cccc_web::VoiceProcessRecovery {
            reconcile: Arc::new(|| Ok(())),
            review: Arc::new(|_| Err(cccc_runtime::process_recovery::RecoveryError::Unavailable)),
            review_daemon_offline: Arc::new(|_| {
                Err(cccc_runtime::process_recovery::RecoveryError::Unavailable)
            }),
            check_daemon_guard: false,
        }
    }
}

#[cfg(unix)]
fn annotate_empty_unconfirmed(
    snapshot: &mut cccc_runtime::process_recovery::RecoverySnapshot,
    unconfirmed: bool,
) {
    if unconfirmed && snapshot.records.is_empty() && snapshot.diagnostics.is_empty() {
        snapshot
            .diagnostics
            .push(cccc_runtime::process_recovery::RecoveryDiagnostic {
                reason: "other",
                hint: "retry_later",
                relative_path: None,
            });
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use cccc_runtime::process_recovery::RecoverySnapshot;

    #[test]
    fn failed_host_initialization_is_not_an_empty_success() {
        let mut snapshot = RecoverySnapshot {
            records: vec![],
            diagnostics: vec![],
        };
        annotate_empty_unconfirmed(&mut snapshot, true);
        assert_eq!(snapshot.diagnostics[0].hint, "retry_later");
        annotate_empty_unconfirmed(&mut snapshot, true);
        assert_eq!(snapshot.diagnostics.len(), 1);
    }

    #[test]
    fn offline_daemon_review_holds_the_owner_lock_and_checks_the_revision() {
        use crate::args::RecoveryOwner;
        use cccc_runtime::process_recovery::RecoveryError;
        use cccc_web::VoiceRecoveryRequest;

        let temp = tempfile::tempdir().expect("temporary home");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        let recovery = new(&home, false);
        let held = crate::process_recovery::try_owner_lock(&home, RecoveryOwner::Daemon)
            .expect("lock")
            .expect("available");
        assert!(
            (recovery.review_daemon_offline)(VoiceRecoveryRequest::List)
                .expect("active")
                .is_none()
        );
        drop(held);

        let ledger = home.daemon_dir().join("owned-process-groups.json");
        std::fs::write(&ledger, b"{").expect("malformed evidence");
        let listed = (recovery.review_daemon_offline)(VoiceRecoveryRequest::List)
            .expect("list")
            .expect("offline");
        assert_eq!(listed["records"][0]["kind"], "opaque_evidence");
        let id = listed["records"][0]["id"].as_str().expect("id").to_owned();
        let version = listed["records"][0]["version"]
            .as_str()
            .expect("version")
            .to_owned();
        assert_eq!(
            (recovery.review_daemon_offline)(VoiceRecoveryRequest::Accept {
                id: id.clone(),
                version: "stale".into()
            }),
            Err(RecoveryError::Changed)
        );
        assert!(ledger.exists());
        let accepted =
            (recovery.review_daemon_offline)(VoiceRecoveryRequest::Accept { id, version })
                .expect("accept")
                .expect("offline");
        assert_eq!(accepted["accepted"], true);
        assert!(!ledger.exists());
    }
}
