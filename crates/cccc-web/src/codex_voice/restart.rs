use super::CodexVoiceSessions;
use crate::api::ApiError;
use serde_json::json;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(super) const OPEN: u8 = 0;
const CLOSING: u8 = 1;
pub(super) const RECOVERY_PENDING: u8 = 2;
pub(super) const PREPARED: u8 = 3;
const SHUTTING_DOWN: u8 = 4;

// Claude Agent SDK close may wait up to 11 seconds for confirmation and then
// 10 seconds for its task. Allow those stages plus owned-child and terminal
// cleanup, without blocking Apply indefinitely on a stuck resource.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceGateError {
    Restarting,
    RecoveryPending,
    ProcessRecoveryPending,
    ResetRecoveryPending,
    ReplacementRecoveryPending,
}

impl fmt::Display for VoiceGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Restarting => f.write_str("Voice is stopping or restarting"),
            Self::RecoveryPending => f.write_str("Voice cleanup is not yet confirmed"),
            Self::ProcessRecoveryPending => {
                f.write_str("Web-owned process recovery is not yet confirmed")
            }
            Self::ResetRecoveryPending => f.write_str("Voice reset cleanup is not yet confirmed"),
            Self::ReplacementRecoveryPending => {
                f.write_str("Voice replacement cleanup is not yet confirmed")
            }
        }
    }
}

impl std::error::Error for VoiceGateError {}

impl VoiceGateError {
    pub(crate) fn into_api_error(self) -> ApiError {
        let details = json!({"retryable":true});
        match self {
            Self::Restarting => ApiError::conflict(
                "codex_voice_restarting",
                "Voice is stopping or restarting. Try again shortly.",
                details,
            ),
            Self::RecoveryPending => ApiError::unavailable(
                "codex_voice_recovery_pending",
                "Previous Voice resources have not been confirmed cleaned up. Try again or check the logs.",
            )
            .with_details(details),
            Self::ProcessRecoveryPending => ApiError::unavailable(
                "codex_voice_recovery_pending",
                "Web-owned process recovery could not be confirmed.",
            )
            .with_details(json!({"retryable":true,"recovery_owners":["web"]})),
            Self::ResetRecoveryPending => ApiError::unavailable(
                "codex_voice_recovery_pending",
                "Voice reset cleanup is not yet confirmed. Check Voice status before trying again.",
            )
            .with_details(json!({"retryable":false})),
            Self::ReplacementRecoveryPending => ApiError::unavailable(
                "codex_voice_recovery_pending",
                "Voice replacement cleanup is not yet confirmed. Check Voice status before trying again.",
            )
            .with_details(json!({"retryable":false})),
        }
    }
}

pub(crate) fn api_error(error: &anyhow::Error) -> Option<ApiError> {
    error
        .downcast_ref::<VoiceGateError>()
        .copied()
        .map(VoiceGateError::into_api_error)
}

impl CodexVoiceSessions {
    pub(super) fn ensure_open(&self) -> anyhow::Result<()> {
        match self.restart_phase.load(Ordering::Acquire) {
            OPEN => Ok(()),
            RECOVERY_PENDING => Err(VoiceGateError::RecoveryPending.into()),
            _ => Err(VoiceGateError::Restarting.into()),
        }
    }

    pub(super) fn begin_shutdown(&self) {
        self.restart_phase.store(SHUTTING_DOWN, Ordering::Release);
    }

    /// Stop the current owner before requesting a Web rebind. Once prepared,
    /// no new Voice work can start on the old manager.
    pub(crate) async fn prepare_web_restart(self: &Arc<Self>) -> Result<(), VoiceGateError> {
        self.cleanup_transition(PREPARED).await
    }

    /// A user-initiated Voice start may retry an earlier incomplete cleanup.
    pub(crate) async fn recover_before_start(self: &Arc<Self>) -> Result<(), VoiceGateError> {
        match self.restart_phase.load(Ordering::Acquire) {
            OPEN => {}
            RECOVERY_PENDING => {
                // A timed-out cleanup can still be running. A user retry need
                // not wait through another deadline while that owner holds the
                // gate; it can retry once the existing cleanup has finished.
                if self.cleanup_gate.try_lock().is_err() {
                    return Err(VoiceGateError::RecoveryPending);
                }
                self.cleanup_transition(OPEN).await?;
            }
            _ => return Err(VoiceGateError::Restarting),
        }
        if let Some(recovery) = self.process_recovery.as_ref() {
            let reconcile = Arc::clone(&recovery.reconcile);
            match tokio::task::spawn_blocking(move || reconcile()).await {
                Ok(Ok(())) => {}
                _ => return Err(VoiceGateError::ProcessRecoveryPending),
            }
        }
        Ok(())
    }

    /// If the restart signal itself fails, the already-clean manager may be
    /// used again. No work is automatically replayed.
    pub(crate) fn cancel_prepared_restart(&self) {
        let _ = self.restart_phase.compare_exchange(
            PREPARED,
            OPEN,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    async fn cleanup_transition(self: &Arc<Self>, next: u8) -> Result<(), VoiceGateError> {
        self.reserve_cleanup(true)?;
        self.cleanup_reserved(next).await
    }

    /// Reserve the owner while its state lock is held, before any reset work
    /// or Web rebind can race a new Voice start.
    pub(super) fn reserve_cleanup(&self, allow_pending: bool) -> Result<(), VoiceGateError> {
        loop {
            let phase = self.restart_phase.load(Ordering::Acquire);
            if phase == RECOVERY_PENDING && !allow_pending {
                return Err(VoiceGateError::RecoveryPending);
            }
            if !matches!(phase, OPEN | RECOVERY_PENDING) {
                return Err(VoiceGateError::Restarting);
            }
            if self
                .restart_phase
                .compare_exchange(phase, CLOSING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
        }
        Ok(())
    }

    pub(super) async fn cleanup_reserved(self: &Arc<Self>, next: u8) -> Result<(), VoiceGateError> {
        self.cleanup_reserved_with_timeout(next, CLEANUP_TIMEOUT)
            .await
    }

    pub(super) async fn cleanup_reserved_with_timeout(
        self: &Arc<Self>,
        next: u8,
        timeout: Duration,
    ) -> Result<(), VoiceGateError> {
        // A Stop may already hold the gate while an admitted Start finishes.
        // Wait for it within the same deadline as cleanup instead of reporting
        // a failure before the previous owner has had a chance to finish.
        // This outer guard also covers cancellation while waiting for the gate.
        let _transition = PendingOnDrop(Arc::clone(self));
        let deadline = tokio::time::Instant::now() + timeout;
        let cleanup =
            match tokio::time::timeout_at(deadline, Arc::clone(&self.cleanup_gate).lock_owned())
                .await
            {
                Ok(cleanup) => cleanup,
                Err(_) => return Err(VoiceGateError::RecoveryPending),
            };
        let sessions = Arc::clone(self);
        let task = tokio::spawn(async move {
            let _cleanup = cleanup;
            // This also runs if the task panics or its HTTP caller disconnects.
            // Hold the cleanup gate until the transition has been recorded.
            let _transition = PendingOnDrop(Arc::clone(&sessions));
            sessions.cleanup_owned().await
        });
        match tokio::time::timeout_at(deadline, task).await {
            Ok(Ok(Ok(()))) => self
                .restart_phase
                .compare_exchange(RECOVERY_PENDING, next, Ordering::AcqRel, Ordering::Acquire)
                .map(|_| ())
                .map_err(|_| VoiceGateError::Restarting),
            failure => {
                // Dropping a timed-out JoinHandle detaches its task; it keeps
                // the cleanup gate and owned handles until it actually ends.
                self.mark_recovery_pending();
                let reason = match failure {
                    Err(_) => {
                        self.terminate_known_local_owner();
                        "timed out"
                    }
                    Ok(Err(_)) => "worker failed",
                    Ok(Ok(Err(_))) => "owned resource cleanup failed",
                    Ok(Ok(Ok(()))) => unreachable!(),
                };
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice cleanup {reason}; recovery is pending"
                ));
                Err(VoiceGateError::RecoveryPending)
            }
        }
    }

    fn mark_recovery_pending(&self) {
        let _ = self.restart_phase.compare_exchange(
            CLOSING,
            RECOVERY_PENDING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// A partially attempted replacement can have changed settings or started
    /// an owner. Retain the owner and require a fresh recovery before Voice start.
    pub(super) fn replacement_recovery_pending(&self) -> VoiceGateError {
        let _ = self.restart_phase.compare_exchange(
            OPEN,
            RECOVERY_PENDING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        VoiceGateError::ReplacementRecoveryPending
    }

    fn terminate_known_local_owner(&self) {
        // Never wait on the state lock after the deadline. If another request
        // still owns it, keep recovery pending and let the cleanup task finish.
        let analyst = self
            .state
            .try_lock()
            .ok()
            .and_then(|state| state.analyst.as_ref().map(|value| value.analyst()));
        if let Some(analyst) = analyst {
            tokio::task::spawn_blocking(move || {
                let outcome = match analyst.terminate_local_processes() {
                    Ok(true) => "requested termination of owned local processes",
                    Ok(false) => "has no owned local process to signal",
                    Err(_) => "could not signal all owned local processes",
                };
                cccc_runtime::diagnostic(format_args!("[cccc] Voice recovery: {outcome}"));
            });
        }
    }

    pub(super) fn prepared_reset_guard(self: &Arc<Self>) -> PreparedResetOnDrop {
        PreparedResetOnDrop(Arc::clone(self))
    }
}

struct PendingOnDrop(Arc<CodexVoiceSessions>);

impl Drop for PendingOnDrop {
    fn drop(&mut self) {
        self.0.mark_recovery_pending();
    }
}

/// A failed or abandoned replacement must keep Voice closed until its
/// possibly-started resources have been reconciled.
pub(super) struct PreparedResetOnDrop(Arc<CodexVoiceSessions>);

impl Drop for PreparedResetOnDrop {
    fn drop(&mut self) {
        let _ = self.0.restart_phase.compare_exchange(
            PREPARED,
            RECOVERY_PENDING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn a_library_manager_without_recovery_capability_remains_available() {
        let sessions = Arc::new(CodexVoiceSessions::new());
        sessions
            .recover_before_start()
            .await
            .expect("standalone managers do not perform host recovery");
        sessions.ensure_open().expect("Voice remains available");
    }

    #[tokio::test]
    async fn a_host_recovery_failure_can_be_retried_without_restarting_web() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let fixture = Arc::clone(&attempts);
        let sessions = Arc::new(CodexVoiceSessions::with_process_recovery(Some(
            crate::VoiceProcessRecovery {
                reconcile: Arc::new(move || {
                    if fixture.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(std::io::Error::other("fixture recovery pending"))
                    } else {
                        Ok(())
                    }
                }),
                review: Arc::new(|_| {
                    Err(cccc_runtime::process_recovery::RecoveryError::Unavailable)
                }),
                review_daemon_offline: Arc::new(|_| Ok(None)),
                check_daemon_guard: true,
            },
        )));
        assert_eq!(
            sessions.recover_before_start().await,
            Err(VoiceGateError::ProcessRecoveryPending)
        );
        sessions
            .recover_before_start()
            .await
            .expect("a new user request retries host recovery");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_idle_manager_prepares_immediately_and_can_be_reopened() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        sessions
            .prepare_web_restart()
            .await
            .expect("an idle Voice manager can be stopped");
        assert_eq!(
            sessions
                .ensure_open()
                .expect_err("restart gate")
                .downcast_ref(),
            Some(&VoiceGateError::Restarting)
        );
        sessions.cancel_prepared_restart();
        sessions
            .ensure_open()
            .expect("failed Apply can reopen Voice");
    }

    #[tokio::test]
    async fn an_inflight_cleanup_blocks_replacement_until_an_explicit_retry() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let in_flight = sessions.cleanup_gate.lock().await;

        sessions.reserve_cleanup(false).expect("reserve cleanup");
        assert_eq!(
            sessions
                .cleanup_reserved_with_timeout(PREPARED, Duration::from_millis(20))
                .await,
            Err(VoiceGateError::RecoveryPending)
        );
        assert_eq!(
            sessions
                .ensure_open()
                .expect_err("recovery gate")
                .downcast_ref(),
            Some(&VoiceGateError::RecoveryPending)
        );
        assert_eq!(
            sessions.recover_before_start().await,
            Err(VoiceGateError::RecoveryPending)
        );
        drop(in_flight);
        sessions
            .recover_before_start()
            .await
            .expect("explicit retry can reopen a clean manager");
        sessions.ensure_open().expect("Voice is available again");
    }

    #[tokio::test]
    async fn restart_waits_for_an_inflight_stop_within_its_deadline() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let in_flight = sessions.cleanup_gate.lock().await;
        sessions.reserve_cleanup(false).expect("reserve cleanup");
        let waiter_sessions = Arc::clone(&sessions);
        let waiter = tokio::spawn(async move {
            waiter_sessions
                .cleanup_reserved_with_timeout(PREPARED, Duration::from_secs(1))
                .await
        });
        tokio::task::yield_now().await;
        assert!(
            !waiter.is_finished(),
            "restart must wait for the prior owner"
        );
        drop(in_flight);
        assert_eq!(waiter.await.expect("worker"), Ok(()));
        assert_eq!(sessions.restart_phase.load(Ordering::Acquire), PREPARED);
    }

    #[tokio::test]
    async fn cancelling_restart_while_waiting_for_the_gate_leaves_recovery_pending() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let in_flight = sessions.cleanup_gate.lock().await;
        let waiter_sessions = Arc::clone(&sessions);
        let waiter = tokio::spawn(async move { waiter_sessions.prepare_web_restart().await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while sessions.restart_phase.load(Ordering::Acquire) != CLOSING {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("restart reserves the owner");
        waiter.abort();
        let _ = waiter.await;
        assert_eq!(
            sessions.restart_phase.load(Ordering::Acquire),
            RECOVERY_PENDING
        );
        drop(in_flight);
        sessions
            .recover_before_start()
            .await
            .expect("a later user request can retry cleanup");
    }

    #[tokio::test]
    async fn an_abandoned_restart_request_does_not_leave_the_manager_closing() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let state = sessions.state.lock().await;
        let request_sessions = Arc::clone(&sessions);
        let request = tokio::spawn(async move { request_sessions.prepare_web_restart().await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while sessions.restart_phase.load(Ordering::Acquire) != CLOSING {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cleanup begins");
        request.abort();
        drop(state);
        tokio::time::timeout(Duration::from_secs(1), async {
            while sessions.restart_phase.load(Ordering::Acquire) != RECOVERY_PENDING {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached cleanup records a retryable state");
        sessions
            .recover_before_start()
            .await
            .expect("a later user request can recover");
    }

    #[tokio::test]
    async fn completing_abandoned_cleanup_does_not_reopen_a_shutting_down_manager() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let state = sessions.state.lock().await;
        let request_sessions = Arc::clone(&sessions);
        let request = tokio::spawn(async move { request_sessions.prepare_web_restart().await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while sessions.restart_phase.load(Ordering::Acquire) != CLOSING {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cleanup begins");
        request.abort();
        sessions.begin_shutdown();
        drop(state);
        let _cleanup = tokio::time::timeout(Duration::from_secs(1), sessions.cleanup_gate.lock())
            .await
            .expect("cleanup finishes");
        assert_eq!(
            sessions.restart_phase.load(Ordering::Acquire),
            SHUTTING_DOWN
        );
        assert_eq!(
            sessions.recover_before_start().await,
            Err(VoiceGateError::Restarting)
        );
    }

    #[tokio::test]
    async fn a_timed_out_cleanup_remains_pending_until_it_finishes() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let state = sessions.state.lock().await;
        sessions.reserve_cleanup(false).expect("reserve cleanup");
        assert_eq!(
            sessions
                .cleanup_reserved_with_timeout(PREPARED, Duration::from_millis(20))
                .await,
            Err(VoiceGateError::RecoveryPending)
        );
        assert_eq!(
            sessions.recover_before_start().await,
            Err(VoiceGateError::RecoveryPending)
        );
        drop(state);
        let cleanup = tokio::time::timeout(Duration::from_secs(1), sessions.cleanup_gate.lock())
            .await
            .expect("detached cleanup finishes");
        drop(cleanup);
        sessions
            .recover_before_start()
            .await
            .expect("user retry can recover");
    }

    #[tokio::test]
    async fn errors_identify_retryable_rejection_and_incomplete_reset() {
        use axum::response::IntoResponse;
        for (error, status, retryable) in [
            (VoiceGateError::Restarting, 409, true),
            (VoiceGateError::RecoveryPending, 503, true),
            (VoiceGateError::ProcessRecoveryPending, 503, true),
            (VoiceGateError::ResetRecoveryPending, 503, false),
            (VoiceGateError::ReplacementRecoveryPending, 503, false),
        ] {
            let response = error.into_api_error().into_response();
            assert_eq!(response.status().as_u16(), status);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("error body");
            let body: serde_json::Value = serde_json::from_slice(&body).expect("error JSON");
            assert_eq!(body["error"]["details"]["retryable"], retryable);
            if error == VoiceGateError::ProcessRecoveryPending {
                assert_eq!(body["error"]["details"]["recovery_owners"], json!(["web"]));
            } else {
                assert!(body["error"]["details"].get("recovery_owners").is_none());
            }
        }
    }

    #[tokio::test]
    async fn late_settings_and_reset_reject_without_waiting_for_the_owner_lock() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let temp = tempfile::tempdir().expect("temporary home");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        let _state = sessions.state.lock().await;
        sessions.reserve_cleanup(false).expect("reserve cleanup");

        let settings = tokio::time::timeout(
            Duration::from_secs(1),
            sessions.apply_analyst_settings(
                &home,
                Default::default(),
                Default::default(),
                Vec::new(),
                false,
                false,
            ),
        )
        .await
        .expect("settings must not wait on a closing owner");
        let settings = match settings {
            Ok(_) => panic!("settings must be rejected"),
            Err(error) => error,
        };
        assert_eq!(settings.downcast_ref(), Some(&VoiceGateError::Restarting));

        let reset = tokio::time::timeout(
            Duration::from_secs(1),
            sessions.reset_analyst(&home, "previous"),
        )
        .await
        .expect("reset must not wait on a closing owner")
        .expect_err("reset must be rejected");
        assert_eq!(reset.downcast_ref(), Some(&VoiceGateError::Restarting));
    }

    async fn persona_session(home: &cccc_core::HomeLayout) -> Arc<super::super::ActiveSession> {
        use super::super::{ActiveSession, CONNECTION_UNATTACHED, CodexVoiceCall};
        use cccc_contracts::codex_voice::{VoiceApplicationContext, VoiceCallMode};

        let context = VoiceApplicationContext::new_with_mode(
            "fixture".into(),
            "local test".into(),
            VoiceCallMode::Persona,
        )
        .expect("persona context");
        let call = Arc::new(
            CodexVoiceCall::start(home, None, Some(context))
                .await
                .expect("local recording lease"),
        );
        Arc::new(ActiveSession {
            verbosity: Default::default(),
            notification_paused: tokio::sync::watch::channel(false).0,
            call,
            analyst: None,
            client_session_id: "fixture".into(),
            offer_digest: [0; 32],
            answer_sdp: String::new(),
            provider_connection: None,
            voice: "cove".into(),
            connection_state: std::sync::atomic::AtomicU8::new(CONNECTION_UNATTACHED),
        })
    }

    #[tokio::test]
    async fn a_failed_call_cleanup_retains_the_owner_until_retry() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let temp = tempfile::tempdir().expect("temporary home");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        let session = persona_session(&home).await;
        sessions.state.lock().await.active = Some(Arc::clone(&session));
        let lock_path = home
            .root()
            .join("state/voice_secretary_recording_lease.json.lock");
        std::fs::remove_file(&lock_path).expect("remove fixture lock file");
        std::fs::create_dir(&lock_path).expect("make lock path inaccessible");

        assert_eq!(
            sessions.prepare_web_restart().await,
            Err(VoiceGateError::RecoveryPending)
        );
        assert!(
            sessions
                .state
                .lock()
                .await
                .active
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &session)),
            "failed cleanup must retain its exact owner"
        );

        std::fs::remove_dir(&lock_path).expect("restore fixture lock path");
        sessions
            .recover_before_start()
            .await
            .expect("cleanup can be retried");
        assert!(sessions.state.lock().await.active.is_none());
        assert_eq!(
            cccc_core::voice_recording_lease::current(&home).expect("lease state"),
            serde_json::json!({})
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn a_locked_call_lease_does_not_block_the_web_runtime() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let temp = tempfile::tempdir().expect("temporary home");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        sessions.state.lock().await.active = Some(persona_session(&home).await);
        let lock_path = home
            .root()
            .join("state/voice_secretary_recording_lease.json.lock");
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)
            .expect("open fixture lock");
        fs2::FileExt::lock_exclusive(&lock).expect("hold fixture lock");
        let (release, wait) = std::sync::mpsc::channel();
        let lock_holder = std::thread::spawn(move || {
            let _ = wait.recv_timeout(Duration::from_secs(3));
            fs2::FileExt::unlock(&lock).expect("release fixture lock");
        });
        let request_sessions = Arc::clone(&sessions);
        let request = tokio::spawn(async move { request_sessions.prepare_web_restart().await });
        let started = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let elapsed = started.elapsed();
        release.send(()).expect("release lock holder");
        lock_holder.join().expect("lock holder exits");
        request.await.expect("request task").expect("cleanup");
        assert!(
            elapsed < Duration::from_secs(2),
            "Web runtime was blocked for {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn both_stop_paths_retain_a_failed_call_for_recovery() {
        for only_unattached in [false, true] {
            let sessions = Arc::new(CodexVoiceSessions::default());
            let temp = tempfile::tempdir().expect("temporary home");
            let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
            home.initialize().expect("initialize home");
            let session = persona_session(&home).await;
            let generation = session.call.generation().to_owned();
            sessions.state.lock().await.active = Some(Arc::clone(&session));
            let lock = home
                .root()
                .join("state/voice_secretary_recording_lease.json.lock");
            std::fs::remove_file(&lock).expect("remove fixture lock file");
            std::fs::create_dir(&lock).expect("make fixture lock inaccessible");

            let stopped = if only_unattached {
                sessions.stop_if_unattached(&generation).await
            } else {
                sessions.stop(&generation).await
            };
            assert!(stopped.is_err(), "failed release must be reported");
            assert!(
                sessions
                    .state
                    .lock()
                    .await
                    .active
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &session)),
                "failed Stop must retain its exact call"
            );
            assert_eq!(
                sessions.prepare_web_restart().await,
                Err(VoiceGateError::RecoveryPending),
                "Apply must not pass while release is unconfirmed"
            );

            std::fs::remove_dir(&lock).expect("restore fixture lock path");
            sessions
                .recover_before_start()
                .await
                .expect("user can retry cleanup");
            assert!(sessions.state.lock().await.active.is_none());
            let next = persona_session(&home).await;
            next.call
                .stop_for_web(next.call.generation())
                .await
                .expect("new call acquired and released the lease");
        }
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn abandoned_stop_keeps_the_gate_until_its_worker_finishes() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let temp = tempfile::tempdir().expect("temporary home");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        let session = persona_session(&home).await;
        let generation = session.call.generation().to_owned();
        sessions.state.lock().await.active = Some(Arc::clone(&session));
        let lock_path = home
            .root()
            .join("state/voice_secretary_recording_lease.json.lock");
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)
            .expect("fixture lock");
        fs2::FileExt::lock_exclusive(&lock).expect("hold fixture lock");
        let (release, wait) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _ = wait.recv_timeout(Duration::from_secs(5));
            fs2::FileExt::unlock(&lock).expect("unlock fixture");
        });

        let request_sessions = Arc::clone(&sessions);
        let request = tokio::spawn(async move { request_sessions.stop(&generation).await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while session.connection_state.load(Ordering::Acquire)
                != super::super::CONNECTION_CLOSING
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Stop was admitted");
        request.abort();
        assert!(
            sessions.cleanup_gate.try_lock().is_err(),
            "worker holds cleanup gate"
        );
        let apply_sessions = Arc::clone(&sessions);
        let apply = tokio::spawn(async move { apply_sessions.prepare_web_restart().await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while sessions.restart_phase.load(Ordering::Acquire) != CLOSING {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Apply reserves the owner");
        assert!(!apply.is_finished(), "Apply must wait for the Stop worker");
        release.send(()).expect("release lock holder");
        holder.join().expect("lock holder exits");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), apply)
                .await
                .expect("Apply finishes")
                .expect("Apply worker"),
            Ok(())
        );
        assert!(sessions.state.lock().await.active.is_none());
        sessions.cancel_prepared_restart();
        sessions
            .recover_before_start()
            .await
            .expect("later retry reopens Voice");
        assert_eq!(
            cccc_core::voice_recording_lease::current(&home).expect("lease state"),
            serde_json::json!({})
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn start_does_not_reuse_a_call_while_its_stop_is_in_progress() {
        use sha2::{Digest, Sha256};

        let sessions = Arc::new(CodexVoiceSessions::default());
        let temp = tempfile::tempdir().expect("temporary home");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        let mut session = persona_session(&home).await;
        Arc::get_mut(&mut session)
            .expect("single fixture owner")
            .offer_digest = Sha256::digest(b"offer").into();
        let config = super::super::RealtimeCallConfig {
            provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
            grok_executable: None,
            auth_path: temp.path().join("unused-auth.json"),
            base_url: "http://127.0.0.1:1".into(),
            voice: "cove".into(),
            preferences: Default::default(),
            application_context: session.call.application_context().cloned(),
        };
        let generation = session.call.generation().to_owned();
        sessions.state.lock().await.active = Some(Arc::clone(&session));
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(
                home.root()
                    .join("state/voice_secretary_recording_lease.json.lock"),
            )
            .expect("fixture lock");
        fs2::FileExt::lock_exclusive(&lock).expect("hold lock");
        let (release, wait) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _ = wait.recv_timeout(Duration::from_secs(5));
            fs2::FileExt::unlock(&lock).expect("unlock fixture");
        });
        let stop_sessions = Arc::clone(&sessions);
        let stop = tokio::spawn(async move { stop_sessions.stop(&generation).await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while session.connection_state.load(Ordering::Acquire)
                != super::super::CONNECTION_CLOSING
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Stop admitted");
        let duplicate = sessions
            .start_with_realtime(&home, "fixture", "offer", config)
            .await;
        let rejected = match duplicate {
            Err(error) => {
                error.downcast_ref::<VoiceGateError>() == Some(&VoiceGateError::Restarting)
            }
            Ok(_) => false,
        };
        release.send(()).expect("release lock holder");
        holder.join().expect("lock holder exits");
        stop.await.expect("stop task").expect("stop completes");
        assert!(
            rejected,
            "Start must not return a closing session as active"
        );
        assert!(sessions.state.lock().await.active.is_none());
        sessions
            .recover_before_start()
            .await
            .expect("Start can retry after Stop completes");
        let next = persona_session(&home).await;
        next.call
            .stop_for_web(next.call.generation())
            .await
            .expect("the next call can acquire and release the lease");
    }

    #[tokio::test]
    async fn successful_stop_allows_a_new_call_immediately() {
        let sessions = Arc::new(CodexVoiceSessions::default());
        let temp = tempfile::tempdir().expect("temporary home");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        let session = persona_session(&home).await;
        let generation = session.call.generation().to_owned();
        sessions.state.lock().await.active = Some(session);
        assert!(sessions.stop(&generation).await.expect("stop call"));
        sessions.ensure_open().expect("no recovery required");
        assert!(sessions.state.lock().await.active.is_none());
        let next = persona_session(&home).await;
        next.call
            .stop_for_web(next.call.generation())
            .await
            .expect("new call acquired and released the lease");
    }
}
