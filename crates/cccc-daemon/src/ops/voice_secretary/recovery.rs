use super::*;

impl Manager {
    pub(super) async fn recover_documents(&self) -> io::Result<()> {
        if resolve_runtime(&self.home).is_err() || cccc_runtime::owned_process_recovery_pending() {
            return Ok(());
        }
        for task in self.store.unfinished()? {
            self.recover_document(&task).await?;
        }
        Ok(())
    }

    /// Only execution with confirmed task-only native writes can be replayed. After
    /// process cleanup, an absent journal proves there is no unconfirmed commit.
    /// Retry once from a fresh current snapshot; never apply a stale candidate.
    pub(super) async fn recover_document(&self, task: &SecretaryTask) -> io::Result<()> {
        if self.closing.load(Ordering::Acquire)
            || !recoverable(task)
            || resolve_runtime(&self.home).is_err()
            || cccc_runtime::owned_process_recovery_pending()
        {
            return Ok(());
        }
        let _permit = if let Some(locks) = &self.locks {
            Some(locks.group_write(&task.target.group_id).await)
        } else {
            None
        };
        let _acceptance = self.acceptance.lock().unwrap_or_else(|e| e.into_inner());
        let old = self.store.load(&task.task_id)?;
        if !recoverable(&old) || self.store.queued()?.len() >= MAX_QUEUED_TASKS {
            return Ok(());
        }
        if let Some(next) = self.store.successor(&old.task_id)? {
            self.store.update(&old.task_id, |task| {
                task.superseded_by = next.task_id;
                Ok(())
            })?;
            return Ok(());
        }
        if super::super::assistants::secretary_validate_request(&self.home, &old).is_err() {
            return Ok(());
        }
        let mut next = SecretaryTask::new(old.target.clone(), old.inputs.clone());
        next.guidance = old.guidance.clone();
        next.followup = old.followup.clone();
        next.previous_task_id = old.task_id.clone();
        next.recovery_attempts = old.recovery_attempts + 1;
        self.store.create(&next)?;
        self.store.update(&old.task_id, |task| {
            task.superseded_by = next.task_id.clone();
            Ok(())
        })?;
        self.notify();
        Ok(())
    }
}

fn recoverable(task: &SecretaryTask) -> bool {
    task.isolated_document_writes
        && task.target.kind == SecretaryTaskKind::Document
        && task.target.request_id.is_empty()
        && task.cleanup_confirmed
        && task.superseded_by.is_empty()
        && task.recovery_attempts < 1
        && task.prepared_version.is_empty()
        && task.cancellation_reason != "user"
        && task.cancellation_reason != "group_deleted"
        && (matches!(
            task.phase,
            SecretaryTaskPhase::Conflict
                | SecretaryTaskPhase::Failed
                | SecretaryTaskPhase::Unconfirmed
        ) || task.phase == SecretaryTaskPhase::Cancelled
            && task.cancellation_reason == "shutdown")
}
