//! The admission mutex fences streams and registers work before a task starts.
//! Neither a socket closing nor a new controller cancels already admitted work.

use super::protocol::{ExecutionFact, Failure, OperationId, OperationSource, OperationState};
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

const MAX_OPERATIONS: usize = 4096;
const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
const MAX_CLOSED_EPOCHS: usize = 32;
const CLOSED_RETENTION: Duration = Duration::from_secs(60 * 60);
const STOP_SLOTS: usize = 64;
const STOP_BYTES: usize = 1024 * 1024;

struct Epoch {
    id: u64,
    highest_admitted: u64,
}

struct ClosedEpoch {
    epoch: Epoch,
    closed_at: Instant,
}

struct Operation {
    source: Option<OperationSource>,
    state: OperationState,
    reserved_bytes: usize,
    finished_at: Option<Instant>,
    stop_scope: Option<bool>,
}

pub(super) struct Admissions {
    instance: String,
    next_epoch: u64,
    current: Option<Epoch>,
    closed: VecDeque<ClosedEpoch>,
    operations: BTreeMap<(u64, u64), Operation>,
    retained_bytes: usize,
}

impl Admissions {
    pub(super) fn new(instance: String) -> Self {
        Self {
            instance,
            next_epoch: 0,
            current: None,
            closed: VecDeque::new(),
            operations: BTreeMap::new(),
            retained_bytes: 0,
        }
    }

    /// Call under the same lock as `admit`. Fencing and waterline publication
    /// must also cover old requests decoded outside the lock.
    pub(super) fn acquire(&mut self) -> Result<u64, Failure> {
        let next = self
            .next_epoch
            .checked_add(1)
            .ok_or_else(|| Failure::new("epoch_exhausted", "owner must be maintained"))?;
        self.close_current();
        self.next_epoch = next;
        self.current = Some(Epoch {
            id: next,
            highest_admitted: 0,
        });
        Ok(next)
    }

    pub(super) fn close(&mut self, epoch: u64) -> bool {
        // An old connection's Drop cannot close its replacement.
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.id == epoch)
        {
            self.close_current();
            return true;
        }
        false
    }

    pub(super) fn require_current(&self, epoch: u64) -> Result<(), Failure> {
        self.current
            .as_ref()
            .filter(|current| current.id == epoch)
            .map(|_| ())
            .ok_or_else(|| Failure::new("stale_epoch", "control stream was closed or replaced"))
    }

    fn close_current(&mut self) {
        if let Some(epoch) = self.current.take() {
            self.closed.push_back(ClosedEpoch {
                epoch,
                closed_at: Instant::now(),
            });
        }
        while self.closed.len() > MAX_CLOSED_EPOCHS {
            self.closed.pop_front();
        }
        self.expire_closed();
    }

    fn expire_closed(&mut self) {
        while self
            .closed
            .front()
            .is_some_and(|epoch| epoch.closed_at.elapsed() > CLOSED_RETENTION)
        {
            self.closed.pop_front();
        }
        let current = self.current.as_ref().map(|epoch| epoch.id);
        self.operations.retain(|&(epoch, _), operation| {
            let expired = (operation.source.is_none() || Some(epoch) != current)
                && operation
                    .finished_at
                    .is_some_and(|finished| finished.elapsed() > CLOSED_RETENTION);
            if expired {
                self.retained_bytes = self.retained_bytes.saturating_sub(operation.reserved_bytes);
            }
            !expired
        });
    }

    pub(super) fn admit(
        &mut self,
        epoch: u64,
        sequence: u64,
        source: Option<OperationSource>,
        reserved_bytes: usize,
    ) -> Result<OperationId, Failure> {
        self.expire_closed();
        self.require_sequence(epoch, sequence)?;
        if self.operations.len() >= MAX_OPERATIONS
            || reserved_bytes > MAX_RETAINED_BYTES.saturating_sub(self.retained_bytes)
        {
            return Err(Failure::new(
                "owner_capacity",
                "unsettled operation capacity is full",
            ));
        }
        self.insert(epoch, sequence, source, reserved_bytes, None)
    }

    fn require_sequence(&self, epoch: u64, sequence: u64) -> Result<(), Failure> {
        let current = self
            .current
            .as_ref()
            .filter(|current| current.id == epoch)
            .ok_or_else(|| Failure::new("stale_epoch", "control stream was closed or replaced"))?;
        if sequence == 0 || sequence <= current.highest_admitted {
            return Err(Failure::new(
                "stale_sequence",
                "operation was already admitted; query its result",
            ));
        }
        Ok(())
    }

    /// Ack/Retire consume stream sequence authority without needing another
    /// execution slot. A full table must still allow it to be reclaimed.
    pub(super) fn administrative(
        &mut self,
        epoch: u64,
        sequence: u64,
    ) -> Result<OperationId, Failure> {
        self.require_sequence(epoch, sequence)?;
        self.current
            .as_mut()
            .expect("current epoch checked")
            .highest_admitted = sequence;
        Ok(OperationId {
            instance: self.instance.clone(),
            epoch,
            sequence,
        })
    }

    pub(super) fn admit_stop(
        &mut self,
        epoch: u64,
        sequence: u64,
        reserved_bytes: usize,
        all: bool,
    ) -> Result<OperationId, Failure> {
        self.expire_closed();
        self.require_sequence(epoch, sequence)?;
        let limit = if all { 1 } else { STOP_SLOTS };
        let slots = self
            .operations
            .values()
            .filter(|operation| operation.stop_scope == Some(all))
            .count();
        let bytes: usize = self
            .operations
            .values()
            .filter(|operation| operation.stop_scope.is_some())
            .map(|operation| operation.reserved_bytes)
            .sum();
        // Reclaim superseded Stop observations only when their reserved pool
        // is needed. Ordinary concurrent callers keep their completed result.
        if slots >= limit || reserved_bytes > STOP_BYTES.saturating_sub(bytes) {
            self.operations.retain(|_, operation| {
                let obsolete = operation.stop_scope.is_some() && operation.finished_at.is_some();
                if obsolete {
                    self.retained_bytes =
                        self.retained_bytes.saturating_sub(operation.reserved_bytes);
                }
                !obsolete
            });
        }
        let slots = self
            .operations
            .values()
            .filter(|operation| operation.stop_scope == Some(all))
            .count();
        let bytes: usize = self
            .operations
            .values()
            .filter(|operation| operation.stop_scope.is_some())
            .map(|operation| operation.reserved_bytes)
            .sum();
        if slots >= limit
            || reserved_bytes > 8192
            || reserved_bytes > STOP_BYTES.saturating_sub(bytes)
        {
            return Err(Failure::new(
                "stop_capacity",
                "confirmed stop capacity is busy; query the pending stop",
            ));
        }
        self.insert(epoch, sequence, None, reserved_bytes, Some(all))
    }

    fn insert(
        &mut self,
        epoch: u64,
        sequence: u64,
        source: Option<OperationSource>,
        reserved_bytes: usize,
        stop_scope: Option<bool>,
    ) -> Result<OperationId, Failure> {
        let id = self.administrative(epoch, sequence)?;
        self.operations.insert(
            (epoch, sequence),
            Operation {
                source,
                state: OperationState::Pending,
                reserved_bytes,
                finished_at: None,
                stop_scope,
            },
        );
        self.retained_bytes += reserved_bytes;
        Ok(id)
    }

    pub(super) fn finish(&mut self, id: &OperationId, result: Result<serde_json::Value, Failure>) {
        if id.instance != self.instance {
            return;
        }
        if let Some(operation) = self.operations.get_mut(&(id.epoch, id.sequence)) {
            operation.state = OperationState::Finished { result };
            let bytes = serde_json::to_vec(&operation.state)
                .map_or(operation.reserved_bytes, |bytes| bytes.len())
                + serde_json::to_vec(&operation.source).map_or(0, |bytes| bytes.len());
            self.retained_bytes = self
                .retained_bytes
                .saturating_sub(operation.reserved_bytes)
                .saturating_add(bytes);
            operation.reserved_bytes = bytes;
            operation.finished_at = Some(Instant::now());
        }
    }

    pub(super) fn query(&mut self, id: &OperationId) -> OperationState {
        if id.instance != self.instance {
            return OperationState::Unknown;
        }
        self.expire_closed();
        if let Some(operation) = self.operations.get(&(id.epoch, id.sequence)) {
            return operation.state.clone();
        }
        if id.sequence > 0
            && self.closed.iter().any(|closed| {
                closed.epoch.id == id.epoch && id.sequence > closed.epoch.highest_admitted
            })
        {
            OperationState::NotAdmitted
        } else {
            OperationState::Unknown
        }
    }

    /// Empty is not proof of non-admission: settled or expired facts are absent.
    pub(super) fn find(&mut self, source: &OperationSource) -> Vec<OperationId> {
        self.expire_closed();
        self.operations
            .iter()
            .filter(|(_, operation)| operation.source.as_ref() == Some(source))
            .map(|(&(epoch, sequence), _)| OperationId {
                instance: self.instance.clone(),
                epoch,
                sequence,
            })
            .collect()
    }

    pub(super) fn find_event(
        &mut self,
        group: &str,
        actor: &str,
        created: &str,
        event: &str,
    ) -> Vec<ExecutionFact> {
        self.expire_closed();
        self.operations
            .iter()
            .filter_map(|(&(epoch, sequence), operation)| {
                let source = operation.source.as_ref()?;
                (source.actor.group_id == group
                    && source.actor.actor_id == actor
                    && source.actor.actor_created_at == created
                    && source.event_ids.iter().any(|id| id == event))
                .then(|| ExecutionFact {
                    operation: OperationId {
                        instance: self.instance.clone(),
                        epoch,
                        sequence,
                    },
                    source: source.clone(),
                    state: operation.state.clone(),
                })
            })
            .collect()
    }

    /// Only the controller's successful final ledger append permits reclaim.
    /// The epoch's waterline still rejects any late duplicate after removal.
    pub(super) fn acknowledge(&mut self, id: &OperationId) -> Result<(), Failure> {
        if id.instance != self.instance {
            return Err(Failure::new(
                "owner_mismatch",
                "operation belongs to another owner instance",
            ));
        }
        let key = (id.epoch, id.sequence);
        if self
            .operations
            .get(&key)
            .is_some_and(|operation| matches!(operation.state, OperationState::Pending))
        {
            return Err(Failure::new(
                "operation_pending",
                "cannot reclaim unfinished execution",
            ));
        }
        if let Some(operation) = self.operations.remove(&key) {
            self.retained_bytes = self.retained_bytes.saturating_sub(operation.reserved_bytes);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> Admissions {
        Admissions::new("live-owner".into())
    }

    #[test]
    fn takeover_fences_decoded_old_requests_and_old_disconnect() {
        let mut state = owner();
        let first = state.acquire().expect("acquire control epoch");
        let operation = state.admit(first, 1, None, 32).expect("admit operation");
        let replacement = state.acquire().expect("acquire control epoch");
        assert_eq!(
            state
                .admit(first, 2, None, 32)
                .expect_err("admission must be rejected")
                .code,
            "stale_epoch"
        );
        state.close(first);
        assert!(state.admit(replacement, 1, None, 32).is_ok());
        assert_eq!(state.query(&operation), OperationState::Pending);
        state.finish(&operation, Ok(serde_json::json!({"accepted":true})));
        assert!(matches!(
            state.query(&operation),
            OperationState::Finished { .. }
        ));
    }

    #[test]
    fn reclaimed_submit_cannot_run_again_and_a_gap_is_not_proof_of_reclaim() {
        let mut state = owner();
        let epoch = state.acquire().expect("acquire control epoch");
        let operation = state.admit(epoch, 3, None, 32).expect("admit operation");
        assert!(state.acknowledge(&operation).is_err());
        state.finish(&operation, Ok(serde_json::Value::Null));
        state
            .acknowledge(&operation)
            .expect("acknowledge completed operation");
        assert_eq!(
            state
                .admit(epoch, 3, None, 32)
                .expect_err("admission must be rejected")
                .code,
            "stale_sequence"
        );
        state.close(epoch);
        assert_eq!(state.query(&operation), OperationState::Unknown);
        let mut gap = operation.clone();
        gap.sequence = 2;
        assert_eq!(state.query(&gap), OperationState::Unknown);
        gap.sequence = 4;
        assert_eq!(state.query(&gap), OperationState::NotAdmitted);
        gap.instance = "different-owner".into();
        assert_eq!(state.query(&gap), OperationState::Unknown);
    }

    #[test]
    fn expired_or_displaced_closed_epoch_is_unknown() {
        let mut state = owner();
        let epoch = state.acquire().expect("acquire control epoch");
        state.close(epoch);
        let id = OperationId {
            instance: "live-owner".into(),
            epoch,
            sequence: 1,
        };
        assert_eq!(state.query(&id), OperationState::NotAdmitted);
        state
            .closed
            .front_mut()
            .expect("closed epoch record")
            .closed_at = Instant::now() - CLOSED_RETENTION - Duration::from_secs(1);
        assert_eq!(state.query(&id), OperationState::Unknown);
    }

    #[test]
    fn capacity_rejects_new_work_without_erasing_existing_result() {
        let mut state = owner();
        let epoch = state.acquire().expect("acquire control epoch");
        let operation = state
            .admit(epoch, 1, None, MAX_RETAINED_BYTES)
            .expect("admit operation");
        assert_eq!(
            state
                .admit(epoch, 2, None, 1)
                .expect_err("admission must be rejected")
                .code,
            "owner_capacity"
        );
        state.finish(&operation, Ok(serde_json::json!("complete")));
        assert!(matches!(
            state.query(&operation),
            OperationState::Finished { .. }
        ));
        state
            .acknowledge(&operation)
            .expect("acknowledge completed operation");
        assert!(state.admit(epoch, 2, None, 1).is_ok());
    }

    #[test]
    fn a_full_execution_table_keeps_member_stop_and_global_stop_capacity() {
        let mut state = owner();
        let epoch = state.acquire().expect("acquire control epoch");
        for sequence in 1..=MAX_OPERATIONS as u64 {
            state
                .admit(epoch, sequence, None, 32)
                .expect("admit operation");
        }
        let mut next = MAX_OPERATIONS as u64 + 1;
        assert!(state.admit(epoch, next, None, 32).is_err());
        for _ in 0..STOP_SLOTS {
            state
                .admit_stop(epoch, next, 128, false)
                .expect("admit stop with reserved capacity");
            next += 1;
        }
        assert!(state.admit_stop(epoch, next, 128, false).is_err());
        // StopAll has its own slot even when every member-stop slot is busy.
        assert!(state.admit_stop(epoch, next, 128, true).is_ok());
        assert!(state.admit_stop(epoch, next + 1, 128, true).is_err());
    }

    #[test]
    fn a_new_epoch_can_acknowledge_old_results_even_with_a_full_table() {
        let mut state = owner();
        let old = state.acquire().expect("acquire control epoch");
        let operation = state.admit(old, 1, None, 32).expect("admit operation");
        state.finish(&operation, Ok(serde_json::Value::Null));
        for sequence in 2..=MAX_OPERATIONS as u64 {
            state
                .admit(old, sequence, None, 32)
                .expect("admit operation");
        }
        let new = state.acquire().expect("acquire control epoch");
        assert!(state.admit(new, 1, None, 32).is_err());
        state
            .administrative(new, 1)
            .expect("admit administrative operation");
        state
            .acknowledge(&operation)
            .expect("acknowledge completed operation");
        state
            .administrative(new, 2)
            .expect("admit administrative operation");
        state
            .acknowledge(&operation)
            .expect("acknowledge completed operation");
        assert!(state.admit(new, 3, None, 32).is_ok());
        assert_eq!(state.query(&operation), OperationState::Unknown);
        assert!(
            state
                .admit(old, MAX_OPERATIONS as u64 + 1, None, 32)
                .is_err()
        );
    }

    #[test]
    fn finished_payload_budget_shrinks_and_only_expired_finished_work_is_reclaimed() {
        let mut state = owner();
        let epoch = state.acquire().expect("acquire control epoch");
        let done = state
            .admit(epoch, 1, None, MAX_RETAINED_BYTES / 2)
            .expect("admit operation");
        let pending = state
            .admit(epoch, 2, None, MAX_RETAINED_BYTES / 2)
            .expect("admit operation");
        state.finish(&done, Ok(serde_json::json!("accepted")));
        assert!(state.retained_bytes < MAX_RETAINED_BYTES);
        state.close(epoch);
        state
            .operations
            .get_mut(&(done.epoch, done.sequence))
            .expect("registered mutable fixture entry")
            .finished_at = Some(Instant::now() - CLOSED_RETENTION - Duration::from_secs(1));
        assert_eq!(state.query(&done), OperationState::Unknown);
        assert_eq!(state.query(&pending), OperationState::Pending);
    }

    #[test]
    fn source_free_results_expire_even_when_their_control_epoch_stays_open() {
        let mut state = owner();
        let epoch = state.acquire().expect("acquire control epoch");
        let operation = state.admit(epoch, 1, None, 100).expect("admit operation");
        state.finish(&operation, Ok(serde_json::Value::Null));
        state
            .operations
            .get_mut(&(epoch, 1))
            .expect("registered mutable fixture entry")
            .finished_at = Some(Instant::now() - CLOSED_RETENTION - Duration::from_secs(1));
        assert_eq!(state.query(&operation), OperationState::Unknown);
        assert_eq!(
            state
                .admit(epoch, 1, None, 1)
                .expect_err("admission must be rejected")
                .code,
            "stale_sequence"
        );
    }
}
