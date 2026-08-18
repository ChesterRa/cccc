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
        return RuntimeStatus {
            running: status.is_some(),
            pid: status.and_then(|item| item.pid),
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
