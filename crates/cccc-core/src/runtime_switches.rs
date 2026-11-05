//! Records of explicit Actor runtime switches, kept in
//! `GroupDoc.extra.runtime_switches`.
//!
//! A record never carries message ids or content: `group_show` returns `extra`
//! verbatim. Two records matter after a switch:
//! - an unfinished switch (`pending`): its audit, handoff notice or private-env
//!   cleanup has not been written yet, and the member stays fenced until it is;
//! - a native source the member left: the old session is told it was replaced
//!   instead of being asked to reconnect.
use cccc_contracts::Actor;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{GroupDoc, actors};

pub const KEY: &str = "runtime_switches";

/// The provider's stored name, e.g. `codex`.
#[must_use]
pub fn runtime_name(runtime: cccc_contracts::ActorRuntime) -> String {
    serde_json::to_value(runtime)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    /// `native` or `managed`.
    pub kind: String,
    /// The provider, as `ActorRuntime` names it.
    pub runtime: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_id: String,
}

impl Endpoint {
    #[must_use]
    pub fn of(actor: &Actor) -> Self {
        Self {
            kind: if actor.external_session_id.is_empty() {
                "managed"
            } else {
                "native"
            }
            .into(),
            runtime: runtime_name(actor.runtime),
            source_id: actor.external_session_id.clone(),
        }
    }

    #[must_use]
    pub fn is_native(&self) -> bool {
        self.kind == "native"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwitchRecord {
    pub op_id: String,
    pub actor_id: String,
    pub actor_generation: String,
    pub from: Endpoint,
    pub to: Endpoint,
    pub at: String,
    #[serde(default)]
    pub pending: bool,
    /// The managed configuration was cleared; completion removes the member's
    /// own private env file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear_private_env: bool,
}

impl SwitchRecord {
    /// Whether this record names the given member generation.
    #[must_use]
    pub fn names(&self, actor: &Actor) -> bool {
        self.actor_id == actor.id && self.actor_generation == actors::generation_identity(actor)
    }

    /// A native source the member left; kept after completion.
    #[must_use]
    pub fn left_native(&self) -> bool {
        self.from.is_native() && !self.from.source_id.is_empty()
    }
}

/// Valid records in stored order. A malformed entry is ignored, never fatal.
#[must_use]
pub fn records(group: &GroupDoc) -> Vec<SwitchRecord> {
    group
        .extra
        .get(KEY)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

pub fn set_records(group: &mut GroupDoc, records: &[SwitchRecord]) {
    if records.is_empty() {
        group.extra.remove(KEY);
    } else {
        group.extra.insert(
            KEY.into(),
            serde_json::to_value(records).unwrap_or(Value::Array(Vec::new())),
        );
    }
}

/// The unfinished switch of this member generation, if any.
#[must_use]
pub fn pending_for(group: &GroupDoc, actor: &Actor) -> Option<SwitchRecord> {
    records(group)
        .into_iter()
        .find(|record| record.pending && record.names(actor))
}

#[must_use]
pub fn has_pending(group: &GroupDoc, actor_id: &str) -> bool {
    actors::find(group, actor_id).is_some_and(|actor| pending_for(group, actor).is_some())
}

#[must_use]
pub fn any_pending(group: &GroupDoc) -> bool {
    group
        .actors
        .iter()
        .any(|actor| pending_for(group, actor).is_some())
}

/// The record of a member that left this native source, while that member
/// generation still exists.
#[must_use]
pub fn left_source(group: &GroupDoc, source_id: &str) -> Option<SwitchRecord> {
    records(group).into_iter().rev().find(|record| {
        record.left_native()
            && record.from.source_id == source_id
            && actors::find(group, &record.actor_id).is_some_and(|actor| record.names(actor))
    })
}

/// Drop records whose member generation no longer exists.
pub fn prune(group: &mut GroupDoc) {
    let kept = records(group)
        .into_iter()
        .filter(|record| {
            actors::find(group, &record.actor_id).is_some_and(|actor| record.names(actor))
        })
        .collect::<Vec<_>>();
    set_records(group, &kept);
}

/// What a copied Group keeps: only completed records of native sources that a
/// member left. An unfinished switch belongs to the source Group's ledger.
#[must_use]
pub fn for_copy(group: &GroupDoc) -> Vec<SwitchRecord> {
    records(group)
        .into_iter()
        .filter(|record| record.left_native() && !record.pending)
        .map(|mut record| {
            record.clear_private_env = false;
            record
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::ActorRuntime;

    fn group_with(actor: &Actor) -> GroupDoc {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let mut group = crate::GroupStore::new(home)
            .expect("store")
            .create("switch records", "")
            .expect("group");
        group.actors.push(actor.clone());
        group
    }

    fn record(actor: &Actor, from: &str, pending: bool) -> SwitchRecord {
        SwitchRecord {
            op_id: format!("op-{from}"),
            actor_id: actor.id.clone(),
            actor_generation: actors::generation_identity(actor),
            from: Endpoint {
                kind: if from.is_empty() { "managed" } else { "native" }.into(),
                runtime: runtime_name(ActorRuntime::Codex),
                source_id: from.into(),
            },
            to: Endpoint {
                kind: "managed".into(),
                runtime: runtime_name(ActorRuntime::Claude),
                source_id: String::new(),
            },
            at: "2026-11-05T00:00:00Z".into(),
            pending,
            clear_private_env: false,
        }
    }

    #[test]
    fn pending_and_left_sources_follow_the_member_generation() {
        let actor = Actor::new("x");
        let mut group = group_with(&actor);
        set_records(&mut group, &[record(&actor, "codex:a", true)]);
        assert!(has_pending(&group, "x"));
        assert!(left_source(&group, "codex:a").is_some());

        group.actors[0].generation = "next".into();
        assert!(!has_pending(&group, "x"));
        assert!(left_source(&group, "codex:a").is_none());
        prune(&mut group);
        assert!(!group.extra.contains_key(KEY));
    }

    #[test]
    fn copies_keep_only_completed_native_departures() {
        let actor = Actor::new("x");
        let mut group = group_with(&actor);
        let mut finished = record(&actor, "codex:a", false);
        finished.clear_private_env = true;
        set_records(
            &mut group,
            &[
                finished.clone(),
                record(&actor, "codex:b", true),
                record(&actor, "", false),
            ],
        );
        finished.clear_private_env = false;
        assert_eq!(for_copy(&group), vec![finished]);
    }

    #[test]
    fn malformed_entries_are_ignored() {
        let actor = Actor::new("x");
        let mut group = group_with(&actor);
        group.extra.insert(
            KEY.into(),
            serde_json::json!([{"op_id": 1}, record(&actor, "codex:a", false)]),
        );
        assert_eq!(records(&group).len(), 1);
    }
}
