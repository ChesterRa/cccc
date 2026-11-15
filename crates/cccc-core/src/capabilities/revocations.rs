//! Process-local fences for slow capability work. Accepted revocations remain
//! visible even when they did not change a JSON document (for example, disabling
//! an already-disabled capability). No external work or file access runs under
//! this mutex, and no in-flight operation survives a daemon process restart.

use crate::HomeLayout;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    home: PathBuf,
    capability: String,
    group: String,
    actor: String,
}

fn revisions() -> &'static Mutex<BTreeMap<Key, u64>> {
    static REVISIONS: OnceLock<Mutex<BTreeMap<Key, u64>>> = OnceLock::new();
    REVISIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn key(home: &HomeLayout, capability: &str, group: &str, actor: &str) -> Key {
    Key {
        home: home.root().to_owned(),
        capability: capability.into(),
        group: group.into(),
        actor: actor.into(),
    }
}

pub struct Snapshot(Vec<(Key, u64)>);

pub fn snapshot(home: &HomeLayout, capability: &str, group: &str, actor: &str) -> Snapshot {
    let revisions = revisions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Snapshot(
        [
            key(home, capability, "", ""),
            key(home, capability, group, ""),
            key(home, capability, group, actor),
        ]
        .into_iter()
        .map(|key| {
            let revision = revisions.get(&key).copied().unwrap_or(0);
            (key, revision)
        })
        .collect(),
    )
}

impl Snapshot {
    pub fn is_current(&self) -> bool {
        let revisions = revisions()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.0
            .iter()
            .all(|(key, revision)| revisions.get(key).copied().unwrap_or(0) == *revision)
    }
}

/// Empty group/actor selects the whole capability; an empty actor selects all
/// members of the group. Call only after validating and authorizing the change.
pub fn invalidate(home: &HomeLayout, capability: &str, group: &str, actor: &str) {
    let mut revisions = revisions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *revisions
        .entry(key(home, capability, group, actor))
        .or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revocation_is_sticky_and_only_invalidates_its_scope() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let other = HomeLayout::from_path(temp.path().join("other")).expect("other");
        let first = snapshot(&home, "mcp:one", "g1", "a");
        let member = snapshot(&home, "mcp:one", "g1", "b");
        let group = snapshot(&home, "mcp:one", "g2", "a");
        let capability = snapshot(&home, "mcp:two", "g1", "a");
        let elsewhere = snapshot(&other, "mcp:one", "g1", "a");
        invalidate(&home, "mcp:one", "g1", "a");
        assert!(!first.is_current());
        assert!(
            member.is_current()
                && group.is_current()
                && capability.is_current()
                && elsewhere.is_current()
        );
        invalidate(&home, "mcp:one", "g1", "");
        assert!(!member.is_current());
        assert!(group.is_current());
        invalidate(&home, "mcp:one", "", "");
        assert!(!group.is_current());
        assert!(capability.is_current());
        invalidate(&home, "mcp:two", "", "");
        assert!(!capability.is_current());
        assert!(elsewhere.is_current());
        assert!(
            !first.is_current(),
            "later writes never restore an old snapshot"
        );
    }

    #[test]
    fn control_mutators_fence_noop_revocations_but_not_positive_bindings() {
        use crate::capabilities::CapabilityStore;
        use serde_json::json;
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = CapabilityStore::new(home.clone());
        let id = "skill:test:fences";
        let record = json!({"capability_id":id,"kind":"skill","name":"Fixture","capsule_text":"Fixture rules"});
        store.import_record(record.clone()).expect("record");
        let before = snapshot(&home, id, "g", "one");
        store
            .enable_and_unhide_for(id, "g", "one", "actor", 3600)
            .expect("enable");
        store.set_hidden_for(id, false, "g", "one").expect("unhide");
        store
            .set_blocked_for(id, false, "g", "", "user", 0)
            .expect("unblock");
        assert!(before.is_current());
        store
            .set_enabled_for(id, false, "g", "one", "actor", 3600)
            .expect("disable");
        let disabled = snapshot(&home, id, "g", "one");
        store
            .set_enabled_for(id, false, "g", "one", "actor", 3600)
            .expect("no-op disable");
        assert!(!disabled.is_current());
        let other = snapshot(&home, id, "g", "two");
        let hidden = snapshot(&home, id, "g", "one");
        store.set_hidden_for(id, true, "g", "one").expect("hide");
        store.set_hidden_for(id, false, "g", "one").expect("unhide");
        assert!(!hidden.is_current());
        assert!(other.is_current());
        let blocked = snapshot(&home, id, "g", "one");
        store
            .set_blocked_for(id, true, "g", "", "user", 0)
            .expect("block");
        store
            .set_blocked_for(id, false, "g", "", "user", 0)
            .expect("unblock");
        assert!(!blocked.is_current());
        let removed = snapshot(&home, id, "g", "one");
        store
            .uninstall_for_group(id, "g")
            .expect("remove group binding");
        store
            .set_removed_for_group(id, "g", false)
            .expect("restore marker");
        assert!(!removed.is_current());
        let replaced = snapshot(&home, id, "g", "one");
        let mut changed = record.clone();
        changed["capsule_text"] = json!("Changed rules");
        store.import_record(changed).expect("replace record");
        store.import_record(record).expect("restore record");
        assert!(!replaced.is_current());
        let deleted = snapshot(&home, id, "g", "one");
        store.remove_record(id).expect("remove record");
        assert!(!deleted.is_current());
    }
}
