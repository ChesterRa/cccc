//! Incremental, payload-free reduction of unconfirmed delivery claims.
use cccc_contracts::Event;
use cccc_core::ledger::{SnapshotCursor, SnapshotUpdate};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const CACHE_GROUPS: usize = 32;
type Claims = HashMap<(String, String), String>;
type Entry = Arc<Mutex<Projection>>;

#[derive(Default)]
struct Projection {
    cursor: SnapshotCursor,
    pending: Claims,
}

struct Cache {
    entries: VecDeque<(PathBuf, Entry)>,
    limit: usize,
}

impl Cache {
    fn entry(&mut self, path: &Path) -> (Entry, Option<(PathBuf, Entry)>) {
        let entry = if let Some(index) = self.entries.iter().position(|(key, _)| key == path) {
            self.entries.remove(index).expect("entry position").1
        } else {
            Arc::new(Mutex::new(Projection::default()))
        };
        self.entries.push_back((path.to_owned(), entry.clone()));
        let retired = (self.entries.len() > self.limit)
            .then(|| self.entries.pop_front())
            .flatten();
        (entry, retired)
    }
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            entries: VecDeque::new(),
            limit: CACHE_GROUPS,
        })
    })
}

fn entry(path: &Path) -> Entry {
    let (entry, retired) = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(path);
    // A group with many unresolved claims can take time to free. Avoid doing
    // that while holding the cache lock for unrelated groups.
    drop(retired);
    entry
}

#[cfg(test)]
thread_local! {
    static REDUCED_EVENTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn reduced_events() -> usize {
    REDUCED_EVENTS.with(std::cell::Cell::get)
}

/// The caller holds the group's runtime_delivery lock throughout this read.
/// Only outstanding claims are retained; Event payloads are not cached.
pub(super) fn read_stranded_claims(path: &Path) -> io::Result<Claims> {
    let entry = entry(path);
    let mut projection = match entry.lock() {
        Ok(projection) => projection,
        Err(poisoned) => {
            // A panic could leave derived state partially reduced.
            let mut projection = poisoned.into_inner();
            *projection = Projection::default();
            entry.clear_poison();
            projection
        }
    };
    let Projection { cursor, pending } = &mut *projection;
    cursor.visit(path, |update| match update {
        SnapshotUpdate::Reset => pending.clear(),
        SnapshotUpdate::Event(event) => {
            #[cfg(test)]
            REDUCED_EVENTS.with(|count| count.set(count.get() + 1));
            reduce(pending, event);
        }
    })?;
    Ok(pending.clone())
}

fn reduce(pending: &mut Claims, event: Event) {
    if event.kind == "actor.add" {
        let actor = event
            .data
            .get("actor")
            .and_then(Value::as_object)
            .and_then(|actor| actor.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !actor.is_empty() {
            pending.retain(|(id, _), _| id != actor);
        }
    } else if event.kind == "runtime.delivery" {
        let actor = event
            .data
            .get("actor_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let source = event
            .data
            .get("source_event_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !actor.is_empty() && !source.is_empty() {
            let key = (actor.to_owned(), source.to_owned());
            if event.data.get("state").and_then(Value::as_str) == Some("claimed") {
                pending.insert(
                    key,
                    event
                        .data
                        .get("transport")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                );
            } else {
                pending.remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::{GroupStore, HomeLayout, ledger, ledger_archive};
    use serde_json::json;
    use std::io::Write;

    fn append(path: &Path, group: &str, kind: &str, data: Value) {
        let mut event = Event::new(kind, group);
        event.data = data.as_object().expect("event data").clone();
        ledger::append(path, &event).expect("append fixture");
    }

    fn full_scan(path: &Path) -> Claims {
        let mut pending = Claims::new();
        ledger::visit_snapshot_oldest_first(path, |event| {
            reduce(&mut pending, event);
            std::ops::ControlFlow::Continue(())
        })
        .expect("full reference scan");
        pending
    }

    #[test]
    fn incremental_claims_match_full_scan_and_only_reduce_appended_events() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("claims fixture", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        let receipt = |actor: &str, source: &str, state: &str, transport: &str| json!({"actor_id":actor,"source_event_id":source,"state":state,"transport":transport});
        append(
            &path,
            &group.group_id,
            "runtime.delivery",
            receipt("a", "one", "claimed", "pty"),
        );
        append(
            &path,
            &group.group_id,
            "runtime.delivery",
            receipt("b", "two", "claimed", "pty"),
        );
        append(
            &path,
            &group.group_id,
            "runtime.delivery",
            receipt("a", "one", "accepted", "pty"),
        );
        assert_eq!(
            read_stranded_claims(&path).expect("cached claims"),
            full_scan(&path)
        );
        let before = reduced_events();
        assert_eq!(
            read_stranded_claims(&path).expect("cached claims"),
            full_scan(&path)
        );
        assert_eq!(
            reduced_events(),
            before,
            "an unchanged ledger reduces no events"
        );

        append(
            &path,
            &group.group_id,
            "runtime.delivery",
            receipt("a", "old", "claimed", "pty"),
        );
        append(
            &path,
            &group.group_id,
            "actor.add",
            json!({"actor":{"id":"a"}}),
        );
        append(
            &path,
            &group.group_id,
            "runtime.delivery",
            receipt("a", "new", "claimed", "pty"),
        );
        append(
            &path,
            &group.group_id,
            "runtime.delivery",
            receipt("a", "orphan", "claimed", "web_model_browser"),
        );
        append(
            &path,
            &group.group_id,
            "runtime.delivery",
            json!({"actor_id":"b","source_event_id":"two"}),
        );
        let actual = read_stranded_claims(&path).expect("incremental claims");
        assert_eq!(actual, full_scan(&path));
        assert_eq!(
            reduced_events() - before,
            5,
            "only the appended events were reduced"
        );
        assert_eq!(actual.len(), 2);
        assert_eq!(actual.get(&("a".into(), "new".into())), Some(&"pty".into()));
        assert_eq!(
            actual.get(&("a".into(), "orphan".into())),
            Some(&"web_model_browser".into())
        );

        ledger_archive::compact(&home, &group.group_id, "fixture").expect("rotate ledger");
        assert_eq!(
            read_stranded_claims(&path).expect("cached claims"),
            full_scan(&path)
        );
        let after_rotation = reduced_events();
        assert_eq!(
            read_stranded_claims(&path).expect("cached claims"),
            full_scan(&path)
        );
        assert_eq!(
            reduced_events(),
            after_rotation,
            "a rebuilt snapshot is reused"
        );
    }

    #[test]
    fn failed_read_does_not_publish_a_partial_claim_fold() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("claim read failure", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        append(
            &path,
            &group.group_id,
            "runtime.delivery",
            json!({
                "actor_id":"a","source_event_id":"one","state":"claimed","transport":"pty"
            }),
        );
        assert_eq!(
            read_stranded_claims(&path).expect("cached claims"),
            full_scan(&path)
        );
        let segments = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("ledger/segments");
        std::fs::create_dir_all(&segments).expect("segments");
        let mut archived = Event::new("runtime.delivery", &group.group_id);
        archived.data = json!({
            "actor_id":"b","source_event_id":"two","state":"claimed","transport":"pty"
        })
        .as_object()
        .expect("archive data")
        .clone();
        let mut plain =
            std::fs::File::create(segments.join("ledger.000001.jsonl")).expect("plain archive");
        serde_json::to_writer(&mut plain, &archived).expect("archive event");
        plain.write_all(b"\n").expect("newline");
        drop(plain);
        let broken = segments.join("ledger.000002.jsonl.gz");
        std::fs::write(&broken, b"not a gzip stream").expect("broken archive");
        assert!(read_stranded_claims(&path).is_err());
        std::fs::remove_file(broken).expect("repair archive");
        assert_eq!(
            read_stranded_claims(&path).expect("cached claims"),
            full_scan(&path)
        );
    }

    #[test]
    fn cache_evicts_the_least_recently_used_group() {
        let mut cache = Cache {
            entries: VecDeque::new(),
            limit: 2,
        };
        let first = cache.entry(Path::new("first")).0;
        let second = cache.entry(Path::new("second")).0;
        assert!(Arc::ptr_eq(&first, &cache.entry(Path::new("first")).0));
        cache.entry(Path::new("third"));
        assert!(Arc::ptr_eq(&first, &cache.entry(Path::new("first")).0));
        assert!(!Arc::ptr_eq(&second, &cache.entry(Path::new("second")).0));
    }
}
