use cccc_contracts::{Actor, ActorRuntime, GroupState};
use cccc_core::GroupDoc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RuntimeStatus {
    pub running: bool,
    pub pid: Option<u32>,
}

pub(super) fn resolve(group: &GroupDoc, actor: &Actor) -> RuntimeStatus {
    if actor.runtime == ActorRuntime::Deepseek {
        return RuntimeStatus {
            running: super::deepseek_runtime::running(&group.group_id, &actor.id),
            pid: None,
        };
    }
    if super::local_headless::supports(actor) {
        let status = super::local_headless::status(&group.group_id, &actor.id);
        if let Some(status) = status {
            return RuntimeStatus {
                running: true,
                pid: status.pid,
            };
        }
        // A process started by an older daemon can still be present while the
        // actor is being projected during an in-place upgrade. Preserve that
        // observable session until its next explicit restart migrates it.
        if actor.runtime == ActorRuntime::Codex
            && actor.runner == cccc_contracts::RunnerKind::Pty
            && let Some(status) = super::actor_runtime::status(&group.group_id, &actor.id)
            && status.running
        {
            return RuntimeStatus {
                running: true,
                pid: status.pid,
            };
        }
        return RuntimeStatus {
            running: false,
            pid: None,
        };
    }
    let session = super::actor_runtime::status(&group.group_id, &actor.id);
    if super::actor_runtime::is_structured(actor) {
        return RuntimeStatus {
            running: actor.enabled && group.running && group.state != GroupState::Stopped,
            pid: None,
        };
    }
    RuntimeStatus {
        running: session.as_ref().is_some_and(|item| item.running),
        pid: session.and_then(|item| item.pid),
    }
}
