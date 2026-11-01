//! Redacted process-recovery review data. An accepted record means the user
//! acknowledged uncertain old work; it is not proof that that work ended.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryRecord {
    pub id: String,
    pub version: String,
    pub kind: &'static str,
    pub created_at_unix_seconds: Option<u64>,
    pub origin: &'static str,
    pub reason: &'static str,
    pub retryable: bool,
    pub acceptance_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryDiagnostic {
    pub reason: &'static str,
    pub hint: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoverySnapshot {
    pub records: Vec<RecoveryRecord>,
    pub diagnostics: Vec<RecoveryDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryResult {
    Cleared,
    StillPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryError {
    #[error("recovery evidence changed; list it again")]
    Changed,
    #[error("recovery evidence is unavailable")]
    Unavailable,
}

/// Read the active process owner's pending guard records without signalling.
pub fn list_active(ledger: &std::path::Path) -> Result<RecoverySnapshot, RecoveryError> {
    #[cfg(unix)]
    {
        crate::process_tree::guard::list_active(ledger)
    }
    #[cfg(not(unix))]
    {
        let _ = ledger;
        Err(RecoveryError::Unavailable)
    }
}

/// Retry cleanup for exactly the record and revision the caller reviewed.
/// This is synchronous; async callers must run it outside executor workers.
pub fn retry_active(
    ledger: &std::path::Path,
    id: &str,
    version: &str,
) -> Result<RetryResult, RecoveryError> {
    #[cfg(unix)]
    {
        crate::process_tree::guard::retry_active(ledger, id, version)
    }
    #[cfg(not(unix))]
    {
        let _ = (ledger, id, version);
        Err(RecoveryError::Unavailable)
    }
}

/// Accept the remaining risk for exactly one previously reviewed record.
/// This never clears unrelated Voice lifecycle gates or sends a signal.
pub fn accept_active(
    ledger: &std::path::Path,
    id: &str,
    version: &str,
) -> Result<(), RecoveryError> {
    #[cfg(unix)]
    {
        crate::process_tree::guard::accept_active(ledger, id, version)
    }
    #[cfg(not(unix))]
    {
        let _ = (ledger, id, version);
        Err(RecoveryError::Unavailable)
    }
}

/// A host holding this owner's instance lock may review its own ledger whether
/// it is guarded in this process or still awaiting initial reconciliation.
/// Both paths independently recheck the ledger under their respective lock.
pub fn list_owned_ledger(ledger: &std::path::Path) -> Result<RecoverySnapshot, RecoveryError> {
    #[cfg(unix)]
    if crate::process_tree::guard::active_for(ledger) {
        return list_active(ledger);
    }
    list_locked_ledger(ledger)
}

pub fn retry_owned_ledger(
    ledger: &std::path::Path,
    id: &str,
    version: &str,
) -> Result<RetryResult, RecoveryError> {
    #[cfg(unix)]
    if crate::process_tree::guard::active_for(ledger) {
        return retry_active(ledger, id, version);
    }
    retry_locked_ledger(ledger, id, version)
}

pub fn accept_owned_ledger(
    ledger: &std::path::Path,
    id: &str,
    version: &str,
) -> Result<(), RecoveryError> {
    #[cfg(unix)]
    if crate::process_tree::guard::active_for(ledger) {
        return accept_active(ledger, id, version);
    }
    accept_locked_ledger(ledger, id, version)
}

/// Inspect an inactive ledger while the caller holds that owner's actual
/// exclusive instance lock. This function never starts or stops the owner.
pub fn list_locked_ledger(ledger: &std::path::Path) -> Result<RecoverySnapshot, RecoveryError> {
    #[cfg(unix)]
    {
        crate::process_tree::guard::list_locked_ledger(ledger)
    }
    #[cfg(not(unix))]
    {
        let _ = ledger;
        Err(RecoveryError::Unavailable)
    }
}

/// Retry one inactive owner's reviewed record while its exclusive instance
/// lock is held. Async callers must run this blocking operation off-worker.
pub fn retry_locked_ledger(
    ledger: &std::path::Path,
    id: &str,
    version: &str,
) -> Result<RetryResult, RecoveryError> {
    #[cfg(unix)]
    {
        crate::process_tree::guard::retry_locked_ledger(ledger, id, version)
    }
    #[cfg(not(unix))]
    {
        let _ = (ledger, id, version);
        Err(RecoveryError::Unavailable)
    }
}

/// Accept exactly one inactive owner's reviewed record while its exclusive
/// instance lock is held; all interrupted sources are absorbed first.
pub fn accept_locked_ledger(
    ledger: &std::path::Path,
    id: &str,
    version: &str,
) -> Result<(), RecoveryError> {
    #[cfg(unix)]
    {
        crate::process_tree::guard::accept_locked_ledger(ledger, id, version)
    }
    #[cfg(not(unix))]
    {
        let _ = (ledger, id, version);
        Err(RecoveryError::Unavailable)
    }
}
