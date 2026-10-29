//! Managed workspace authority follows its registered process, otherwise config.
use cccc_contracts::Actor;
use cccc_core::group_scope::{self, EffectiveScope};
use cccc_core::{GroupDoc, HomeLayout, actors};
use std::path::{Path, PathBuf};

use crate::dispatch::OpError;

/// Kept inside the existing runtime owner, not in Group/Actor persistence.
#[derive(Clone, Default)]
pub(crate) struct LaunchWorkspace {
    pub scope_key: String,
    pub cwd: PathBuf,
    pub actor_generation: String,
}

impl LaunchWorkspace {
    pub(crate) fn capture(group: &GroupDoc, actor: &Actor, cwd: &Path) -> Self {
        Self {
            scope_key: group_scope::effective_scope(group, Some(actor))
                .map(|scope| scope.scope_key.clone())
                .unwrap_or_default(),
            cwd: cwd.to_path_buf(),
            actor_generation: actors::generation_identity(actor),
        }
    }

    pub(crate) fn for_actor(self, actor: &Actor) -> Option<EffectiveScope> {
        // A replaced/copied/imported Actor cannot inherit a previous owner's
        // workspace even if a stale registration has the same textual ID.
        (self.actor_generation == actors::generation_identity(actor) && !self.scope_key.is_empty())
            .then_some(EffectiveScope {
                scope_key: self.scope_key,
                path: self.cwd,
                source: "runtime_launch".into(),
            })
    }
}

pub(crate) fn effective_scope(group: &GroupDoc, actor: Option<&Actor>) -> Option<EffectiveScope> {
    // With no managed owner, including Native sessions, this is only the CCCC
    // tool configuration. It does not observe or control the original app's cwd
    // or lifetime; an explicit same-key attachment rebind changes that config.
    let configured = || group_scope::configured_scope(group, actor);
    let scope = if let Some(actor) = actor {
        if let Some((running, workspace)) =
            super::super::local_headless::registered_workspace(&group.group_id, &actor.id)
        {
            if running {
                workspace.for_actor(actor)
            } else {
                configured()
            }
        } else if let Some((running, workspace)) =
            super::super::deepseek_runtime::registered_workspace(&group.group_id, &actor.id)
        {
            if running {
                workspace.for_actor(actor)
            } else {
                configured()
            }
        } else if let Some(status) = super::status(&group.group_id, &actor.id).filter(|s| s.running)
        {
            LaunchWorkspace {
                scope_key: status.scope_key,
                cwd: status.cwd,
                actor_generation: status.actor_generation,
            }
            .for_actor(actor)
        } else {
            configured()
        }
    } else {
        configured()
    }?;
    // All consumers receive the same canonical, existing directory, including
    // prompts and bootstrap. A vanished launch root is unavailable, not a new pin.
    let path = scope
        .path
        .canonicalize()
        .ok()
        .filter(|path| path.is_dir())?;
    Some(EffectiveScope { path, ..scope })
}

pub(crate) fn caller_scope(group: &GroupDoc, by: &str) -> Result<Option<EffectiveScope>, OpError> {
    cccc_core::permissions::require_group_member(group, by)
        .map_err(|error| OpError::new("permission_denied", error.to_string()))?;
    Ok(effective_scope(group, actors::find(group, by.trim())))
}

pub(crate) fn session_prompt(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> String {
    let scope = effective_scope(group, Some(actor));
    cccc_core::system_prompt::render_session_with_scope(home, group, actor, scope.as_ref())
}
