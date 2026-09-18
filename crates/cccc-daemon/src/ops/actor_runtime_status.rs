use cccc_contracts::{Actor, ActorRuntime, GroupState};
use cccc_core::GroupDoc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RuntimeStatus {
    pub running: bool,
    pub pid: Option<u32>,
}

pub(super) fn resolve(group: &GroupDoc, actor: &Actor) -> RuntimeStatus {
    if super::deepseek_runtime::running(&group.group_id, &actor.id) {
        return RuntimeStatus {
            running: true,
            pid: None,
        };
    }
    if let Some(status) = super::local_headless::status(&group.group_id, &actor.id) {
        return RuntimeStatus {
            running: true,
            pid: status.pid,
        };
    }
    let session = super::actor_runtime::status(&group.group_id, &actor.id);
    if let Some(status) = &session
        && status.running
    {
        return RuntimeStatus {
            running: true,
            pid: status.pid,
        };
    }
    if actor.runtime == ActorRuntime::Deepseek || super::local_headless::supports(actor) {
        return RuntimeStatus {
            running: false,
            pid: None,
        };
    }
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn retained_pty_remains_visible_after_saving_managed_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("transition", "")
            .expect("group");
        let mut actor = Actor::new("transition-agent");
        cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: cccc_contracts::RunnerKind::Pty,
            command: vec!["sleep".into(), "60".into()],
            cwd: temp.path().to_owned(),
            env: Default::default(),
            cols: 80,
            rows: 24,
        })
        .expect("fixture PTY");
        struct Cleanup(String, String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = cccc_runtime::stop(&self.0, &self.1);
            }
        }
        let _cleanup = Cleanup(group.group_id.clone(), actor.id.clone());
        for runtime in [
            ActorRuntime::Codex,
            ActorRuntime::Grok,
            ActorRuntime::Deepseek,
            ActorRuntime::WebModel,
        ] {
            actor.runtime = runtime;
            actor.normalize_runtime_constraints();
            assert!(resolve(&group, &actor).running);
        }
    }
}
