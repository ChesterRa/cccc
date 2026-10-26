use super::deliveries::DeliveryState;
use super::notices::NoticeState;
use super::*;
use crate::ledger::snapshot::{Location, Snapshot, Source};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const PAGE_SIZE: usize = 1000;
const CACHE_GROUPS: usize = 32;

#[derive(Clone)]
struct MailRef {
    id: String,
    ts: String,
    location: Location,
}
#[derive(Default)]
struct Mailbox {
    page: VecDeque<MailRef>,
    count: usize,
}
struct Projection {
    group: GroupDoc,
    routing: Value,
    cursors: BTreeMap<String, String>,
    sources: Vec<Source>,
    offsets: Vec<u64>,
    boxes: BTreeMap<String, Mailbox>,
    notices: NoticeState,
    deliveries: DeliveryState,
}
#[derive(Default)]
struct Slot {
    projection: Mutex<Option<Projection>>,
    cursor_epoch: AtomicU64,
}
type Entry = Arc<Slot>;
#[derive(Default)]
struct Cache {
    entries: VecDeque<(PathBuf, Entry)>,
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}
fn entry(path: &Path) -> Entry {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(position) = cache.entries.iter().position(|(key, _)| key == path) {
        let pair = cache.entries.remove(position).expect("entry position");
        let entry = pair.1.clone();
        cache.entries.push_back(pair);
        return entry;
    }
    let entry = Arc::new(Slot::default());
    cache.entries.push_back((path.to_owned(), entry.clone()));
    while cache.entries.len() > CACHE_GROUPS {
        cache.entries.pop_front();
    }
    entry
}
fn cached(path: &Path) -> Option<Entry> {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entries
        .iter()
        .find(|(key, _)| key == path)
        .map(|(_, entry)| entry.clone())
}
fn routing(group: &GroupDoc) -> Value {
    json!((
        group
            .actors
            .iter()
            .map(|actor| (
                &actor.id,
                &actor.created_at,
                effective_role(group, &actor.id),
                &actor.internal_kind
            ))
            .collect::<Vec<_>>(),
        crate::automation::delivery_timing_value(group, "mail_notice_repeat_seconds", 0),
    ))
}
impl Projection {
    fn build(
        group: &GroupDoc,
        state: &InboxState,
        snapshot: &Snapshot,
        actor_ids: &[String],
    ) -> io::Result<Self> {
        let mut result = Self {
            group: group.clone(),
            routing: routing(group),
            cursors: state.cursors.clone(),
            sources: snapshot.sources.clone(),
            offsets: Vec::new(),
            boxes: BTreeMap::new(),
            notices: NoticeState::default(),
            deliveries: DeliveryState::default(),
        };
        for actor in &group.actors {
            result.boxes.entry(actor.id.clone()).or_default();
        }
        for actor in actor_ids {
            result.boxes.entry(actor.clone()).or_default();
        }
        for cursor in state.cursors.values() {
            result.notices.remember_cursor(cursor);
        }
        // Discover historical references before reducing their source records.
        // Both passes use the same identities and byte bounds, with no writer
        // lock. Only the rotating active file needs a pinned descriptor.
        snapshot.visit(&[], |event, _| {
            result.notices.remember_references(&event);
            result.deliveries.remember_references(&event);
            ControlFlow::Continue(())
        })?;
        result.offsets = snapshot.visit(&[], |event, location| {
            result.note(&event, location);
            ControlFlow::Continue(())
        })?;
        Ok(result)
    }
    fn note(&mut self, event: &Event, location: Location) {
        for (actor, mailbox) in &mut self.boxes {
            if actor_add_id(event) == Some(actor.as_str())
                || self.cursors.get(actor) == Some(&event.id)
            {
                mailbox.page.clear();
                mailbox.count = 0;
            } else if is_mail_for_actor(&self.group, event, actor) {
                // An overflowing page must not append a new tail ahead of its
                // omitted older Mail. Refill those references after consumption.
                if mailbox.count == mailbox.page.len() && mailbox.page.len() < PAGE_SIZE {
                    mailbox.page.push_back(MailRef {
                        id: event.id.clone(),
                        ts: event.ts.clone(),
                        location,
                    });
                }
                mailbox.count += 1;
            }
        }
        self.notices.note(&self.group, event);
        self.deliveries.note(&self.group, event, location);
    }
    fn move_cursors(&mut self, state: &InboxState, snapshot: &Snapshot) -> io::Result<bool> {
        for (actor, mailbox) in &mut self.boxes {
            let next = state.cursors.get(actor);
            if self.cursors.get(actor) == next {
                continue;
            }
            let Some(position) =
                next.and_then(|id| mailbox.page.iter().position(|mail| &mail.id == id))
            else {
                return Ok(false);
            };
            let refill_after = mailbox.page.back().map(|mail| mail.location);
            for _ in 0..=position {
                mailbox.page.pop_front();
            }
            mailbox.count -= position + 1;
            if mailbox.page.len() < mailbox.count
                && let Some(after) = refill_after
            {
                let mut offsets = vec![u64::MAX; snapshot.sources.len()];
                offsets[after.source] = after.end;
                for offset in &mut offsets[after.source + 1..] {
                    *offset = 0;
                }
                snapshot.visit(&offsets, |event, location| {
                    if is_mail_for_actor(&self.group, &event, actor) {
                        mailbox.page.push_back(MailRef {
                            id: event.id,
                            ts: event.ts,
                            location,
                        });
                    }
                    if mailbox.page.len() >= PAGE_SIZE.min(mailbox.count) {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                })?;
            }
        }
        self.cursors = state.cursors.clone();
        Ok(true)
    }
}

// The query mutex serializes cache publication, never append. Snapshot identity
// includes source order and inode; growth is decoded from committed offsets.
fn with_projection<T>(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_ids: &[String],
    state: &InboxState,
    mut inspect: impl FnMut(&mut Projection, &Snapshot) -> io::Result<T>,
) -> io::Result<T> {
    let path = GroupStore::new(home.clone())?.ledger_path(&group.group_id)?;
    let entry = entry(&path);
    let mut guard = entry
        .projection
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut state = state.clone();
    loop {
        let cursor_epoch = entry.cursor_epoch.load(Ordering::Acquire);
        let snapshot = Snapshot::open(&path)?;
        let result = (|| -> io::Result<T> {
            let rebuild = match guard.as_mut() {
                Some(projection)
                    if projection.routing == routing(group)
                        && actor_ids.iter().all(|id| projection.boxes.contains_key(id))
                        && snapshot.extends(&projection.sources) =>
                {
                    let mut unknown = false;
                    snapshot.visit(&projection.offsets, |event, _| {
                        unknown |= !projection.notices.can_append(&event);
                        unknown |= !projection.deliveries.can_append(&event);
                        ControlFlow::Continue(())
                    })?;
                    if unknown || !projection.move_cursors(&state, &snapshot)? {
                        true
                    } else {
                        let old_offsets = projection.offsets.clone();
                        projection.offsets = snapshot.visit(&old_offsets, |event, location| {
                            projection.note(&event, location);
                            ControlFlow::Continue(())
                        })?;
                        projection.sources = snapshot.sources.clone();
                        false
                    }
                }
                _ => true,
            };
            if rebuild {
                *guard = Some(Projection::build(group, &state, &snapshot, actor_ids)?);
            }
            inspect(guard.as_mut().expect("projection initialized"), &snapshot)
        })();
        if result.is_err() {
            // A failed suffix read/refill may have reduced only a prefix.
            // Never retry it into the same partially advanced cache.
            *guard = None;
        }
        // Validate after page decoding as well: archives open on demand and
        // may have been replaced since capture. Read-only inspection can retry
        // when a sealed source, the captured active prefix or a cursor changed;
        // stable read errors escape. Ordinary appends leave that prefix valid
        // and must not starve publication. A missed try_lock callback merges
        // by byte offset on the next query.
        let current = Snapshot::open(&path)?;
        let current_state = load(home, &group.group_id)?;
        if !current.extends(&snapshot.sources)
            || current_state.cursors != state.cursors
            || entry.cursor_epoch.load(Ordering::Acquire) != cursor_epoch
        {
            state = current_state;
            continue;
        }
        return result;
    }
}

pub(super) fn pages(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_ids: &[String],
    state: &InboxState,
    limit: usize,
) -> io::Result<BTreeMap<String, Vec<Event>>> {
    with_projection(home, group, actor_ids, state, |projection, snapshot| {
        let mut locations = Vec::new();
        let batches: Vec<_> = actor_ids
            .iter()
            .map(|actor| {
                let start = locations.len();
                locations.extend(
                    projection.boxes[actor]
                        .page
                        .iter()
                        .take(limit.min(PAGE_SIZE))
                        .map(|mail| mail.location),
                );
                (actor.clone(), locations.len() - start)
            })
            .collect();
        let mut messages = snapshot.events(&locations)?.into_iter();
        Ok(batches
            .into_iter()
            .map(|(actor, count)| (actor, messages.by_ref().take(count).collect()))
            .collect())
    })
}
pub(super) fn counts(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_ids: &[String],
    limit: usize,
) -> io::Result<BTreeMap<String, usize>> {
    let state = load(home, &group.group_id)?;
    with_projection(home, group, actor_ids, &state, |projection, _| {
        Ok(actor_ids
            .iter()
            .map(|actor| {
                (
                    actor.clone(),
                    projection.boxes[actor].count.min(limit.min(PAGE_SIZE)),
                )
            })
            .collect())
    })
}
pub(super) fn pending(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &str,
    state: &InboxState,
) -> io::Result<Option<(usize, String)>> {
    with_projection(home, group, &[actor.to_owned()], state, |projection, _| {
        let mailbox = &projection.boxes[actor];
        Ok(mailbox
            .page
            .front()
            .map(|mail| (mailbox.count, mail.ts.clone())))
    })
}
pub(crate) fn due(
    home: &HomeLayout,
    group: &GroupDoc,
    eligible: &[&cccc_contracts::Actor],
    mail_delays: crate::automation::MailDelays<'_>,
    reply_after: i64,
) -> io::Result<Vec<Event>> {
    let state = load(home, &group.group_id)?;
    with_projection(home, group, &[], &state, |projection, _| {
        Ok(projection.notices.due(
            group,
            eligible,
            &projection.cursors,
            &mail_delays,
            reply_after,
        ))
    })
}
pub(crate) fn still_awaited(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &str,
    ids: &[String],
) -> io::Result<std::collections::HashSet<String>> {
    let state = load(home, &group.group_id)?;
    with_projection(home, group, &[], &state, |projection, _| {
        Ok(projection
            .notices
            .still_awaited(actor, ids, projection.cursors.get(actor)))
    })
}

pub(super) fn pending_deliveries(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &str,
    limit: usize,
) -> io::Result<Vec<Event>> {
    let state = load(home, &group.group_id)?;
    with_projection(home, group, &[], &state, |projection, snapshot| {
        snapshot.events(&projection.deliveries.pending(group, actor, limit))
    })
}
pub(super) fn delivery_states(
    home: &HomeLayout,
    group: &GroupDoc,
    actors: &[String],
    source: &str,
) -> io::Result<HashMap<String, (String, String)>> {
    let state = load(home, &group.group_id)?;
    with_projection(home, group, &[], &state, |projection, _| {
        Ok(actors
            .iter()
            .filter_map(|actor| {
                projection
                    .deliveries
                    .latest(actor, source)
                    .map(|state| (actor.clone(), state))
            })
            .collect())
    })
}

// Called only after releasing the ledger writer lock. Never wait for a cold
// replay or page decoding: a missed callback is recovered from source offsets
// on the next query. Exact commit offsets make delayed callbacks idempotent.
pub(crate) fn note_append(path: &Path, event: &Event, source: Source, start: u64, end: u64) {
    let Some(entry) = cached(path) else {
        return;
    };
    let Ok(mut guard) = entry.projection.try_lock() else {
        return;
    };
    let Some(projection) = guard.as_mut() else {
        return;
    };
    let Some(index) = projection
        .sources
        .iter()
        .position(|token| token.path == path)
    else {
        return;
    };
    let previous = &projection.sources[index];
    if previous.identity != source.identity {
        return;
    }
    if projection.offsets[index] >= end {
        return;
    }
    if projection.offsets[index] != start
        || !projection.notices.can_append(event)
        || !projection.deliveries.can_append(event)
    {
        return;
    }
    projection.note(
        event,
        Location {
            source: index,
            offset: start,
            end,
        },
    );
    projection.sources[index] = source;
    projection.offsets[index] = end;
}

// Cursor writes notify only after persistence succeeded (including recovery).
// Readers reload the durable cursor before publication, then refill omitted
// Mail after the last retained byte offset rather than replaying the prefix.
pub(crate) fn note_cursor(path: &Path) {
    if let Some(entry) = cached(path) {
        entry.cursor_epoch.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
pub(crate) fn retained_mail_notice_claims(
    home: &HomeLayout,
    group: &GroupDoc,
) -> io::Result<usize> {
    let state = load(home, &group.group_id)?;
    with_projection(home, group, &[], &state, |projection, _| {
        Ok(projection.notices.retained_mail_claims())
    })
}

#[cfg(test)]
pub(crate) fn cached_actor_generation(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &str,
) -> io::Result<Option<String>> {
    let state = load(home, &group.group_id)?;
    with_projection(home, group, &[], &state, |projection, _| {
        Ok(projection
            .group
            .actors
            .iter()
            .find(|a| a.id == actor)
            .map(|a| a.created_at.clone()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    struct Fixture {
        _temp: tempfile::TempDir,
        home: HomeLayout,
        group: GroupDoc,
        ledger: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("fixture");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let mut group = store.create("projection boundary", "").expect("group");
            group.actors.push(cccc_contracts::Actor::new("reader"));
            store.save(&group).expect("save");
            let ledger = store.ledger_path(&group.group_id).expect("ledger");
            fs::write(&ledger, b"").expect("active ledger");
            Self {
                _temp: temp,
                home,
                group,
                ledger,
            }
        }

        fn mail(&self, text: &str) -> Event {
            let mut event = Event::new("chat.message", &self.group.group_id);
            event.by = "user".into();
            event.data = json!({"to":["reader"],"message_mode":"mail","text":text})
                .as_object()
                .expect("data")
                .clone();
            event
        }
    }

    fn page(projection: &Projection, snapshot: &Snapshot) -> io::Result<Vec<Event>> {
        snapshot.events(
            &projection.boxes["reader"]
                .page
                .iter()
                .map(|mail| mail.location)
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn appending_during_each_page_read_returns_the_captured_prefix_and_catches_up() {
        for native_writer in [true, false] {
            let f = Fixture::new();
            let first = f.mail("captured Mail");
            let next = f.mail("appended Mail");
            ledger::append(&f.ledger, &first).expect("initial Mail");
            let state = load(&f.home, &f.group.group_id).expect("state");
            let mut attempts = 0;
            let rows = with_projection(
                &f.home,
                &f.group,
                &["reader".into()],
                &state,
                |projection, snapshot| {
                    attempts += 1;
                    assert_eq!(
                        attempts, 1,
                        "append-only growth must not force another page decode"
                    );
                    let rows = page(projection, snapshot)?;
                    // Every inspection overlaps a write, as with a continuously
                    // active background writer. Fail on a retry instead of hanging.
                    if native_writer {
                        ledger::append(&f.ledger, &next)?;
                    } else {
                        let _lock = ledger::acquire_writer_lock(&f.ledger)?;
                        let mut stream = fs::OpenOptions::new().append(true).open(&f.ledger)?;
                        serde_json::to_writer(&mut stream, &next)?;
                        stream.write_all(b"\n")?;
                    }
                    Ok(rows)
                },
            )
            .expect("captured page");
            assert_eq!(
                rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
                [&first.id]
            );
            let caught_up = super::super::list_unread(&f.home, &f.group, "reader", 10)
                .expect("catch up skipped callback");
            assert_eq!(
                caught_up.iter().map(|row| &row.id).collect::<Vec<_>>(),
                [&first.id, &next.id]
            );
            let consumed = super::super::consume_unread(&f.home, &f.group, "reader", "reader", 1)
                .expect("consume prefix");
            assert_eq!(consumed.messages[0].id, first.id);
            assert_eq!(
                super::super::list_unread(&f.home, &f.group, "reader", 10).expect("remaining Mail")
                    [0]
                .id,
                next.id
            );
        }
    }

    #[test]
    fn cursor_advance_during_page_decode_still_reconciles() {
        let f = Fixture::new();
        let first = f.mail("first Mail");
        let next = f.mail("next Mail");
        ledger::append(&f.ledger, &first).expect("first");
        ledger::append(&f.ledger, &next).expect("next");
        let state = load(&f.home, &f.group.group_id).expect("state");
        let mut attempts = 0;
        let rows = with_projection(
            &f.home,
            &f.group,
            &["reader".into()],
            &state,
            |projection, snapshot| {
                attempts += 1;
                let rows = page(projection, snapshot)?;
                if attempts == 1 {
                    let mut advanced = state.clone();
                    advanced.cursors.insert("reader".into(), first.id.clone());
                    super::super::save(&f.home, &f.group.group_id, &advanced, "reader", &first.ts)?;
                }
                Ok(rows)
            },
        )
        .expect("cursor-consistent page");
        assert_eq!(attempts, 2);
        assert_eq!(
            rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
            [&next.id]
        );
    }

    #[test]
    fn active_truncation_during_page_decode_still_reconciles() {
        let f = Fixture::new();
        let old = f.mail(&"old Mail ".repeat(100));
        let new = f.mail("replacement Mail");
        ledger::append(&f.ledger, &old).expect("old Mail");
        let state = load(&f.home, &f.group.group_id).expect("state");
        let mut attempts = 0;
        let rows = with_projection(
            &f.home,
            &f.group,
            &["reader".into()],
            &state,
            |projection, snapshot| {
                attempts += 1;
                let rows = page(projection, snapshot)?;
                if attempts == 1 {
                    let _lock = ledger::acquire_writer_lock(&f.ledger)?;
                    fs::write(&f.ledger, format!("{}\n", serde_json::to_string(&new)?))?;
                }
                Ok(rows)
            },
        )
        .expect("reconciled truncation");
        assert_eq!(attempts, 2);
        assert_eq!(
            rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
            [&new.id]
        );
    }

    #[test]
    fn sealed_plain_archive_growth_during_page_decode_still_reconciles() {
        let f = Fixture::new();
        let first = f.mail("archived Mail");
        let next = f.mail("archive repair Mail");
        let segments = GroupStore::new(f.home.clone())
            .expect("store")
            .state_dir(&f.group.group_id)
            .expect("state")
            .join("ledger/segments");
        fs::create_dir_all(&segments).expect("segments");
        let archive = segments.join("ledger.000001.jsonl");
        fs::write(
            &archive,
            format!("{}\n", serde_json::to_string(&first).expect("event")),
        )
        .expect("archive");
        let state = load(&f.home, &f.group.group_id).expect("state");
        let mut attempts = 0;
        let rows = with_projection(
            &f.home,
            &f.group,
            &["reader".into()],
            &state,
            |projection, snapshot| {
                attempts += 1;
                let rows = page(projection, snapshot)?;
                if attempts == 1 {
                    // Only active-ledger growth is compatible. A repair to a
                    // sealed plain archive must still invalidate the page.
                    let _lock = ledger::acquire_writer_lock(&f.ledger)?;
                    let mut stream = fs::OpenOptions::new().append(true).open(&archive)?;
                    serde_json::to_writer(&mut stream, &next)?;
                    stream.write_all(b"\n")?;
                }
                Ok(rows)
            },
        )
        .expect("reconciled archive growth");
        assert_eq!(attempts, 2);
        assert_eq!(
            rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
            [&first.id, &next.id]
        );
    }

    #[test]
    fn archive_replacement_before_or_after_page_read_retries_publication() {
        for replace_before_read in [true, false] {
            let temp = tempfile::tempdir().expect("fixture");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let mut group = store.create("archive replacement", "").expect("group");
            group.actors.push(cccc_contracts::Actor::new("reader"));
            store.save(&group).expect("save");
            let ledger = store.ledger_path(&group.group_id).expect("ledger");
            fs::write(&ledger, b"").expect("active ledger");
            let segments = store
                .state_dir(&group.group_id)
                .expect("state")
                .join("ledger/segments");
            fs::create_dir_all(&segments).expect("segments");
            let archive = segments.join("ledger.000001.jsonl");
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data = json!({"to":["reader"],"message_mode":"mail","text":"fixed fixture"})
                .as_object()
                .expect("data")
                .clone();
            let bytes = format!("{}\n", serde_json::to_string(&event).expect("event"));
            fs::write(&archive, &bytes).expect("archive");
            let state = load(&home, &group.group_id).expect("state");
            let mut attempts = 0;
            let page = with_projection(
                &home,
                &group,
                &["reader".into()],
                &state,
                |projection, snapshot| {
                    attempts += 1;
                    let replace = || {
                        // A writer replaces a sealed source between capture and
                        // publication. Keep its old inode alive through rename.
                        let _lock = crate::ledger::acquire_writer_lock(&ledger).expect("writer");
                        let old = archive.with_extension("retired");
                        fs::rename(&archive, &old).expect("retire source");
                        fs::write(&archive, &bytes).expect("replacement");
                        fs::remove_file(old).expect("retired source");
                    };
                    if attempts == 1 && replace_before_read {
                        replace();
                    }
                    let locations: Vec<_> = projection.boxes["reader"]
                        .page
                        .iter()
                        .map(|mail| mail.location)
                        .collect();
                    let page = snapshot.events(&locations);
                    if attempts == 1 && !replace_before_read {
                        replace();
                    }
                    page
                },
            )
            .expect("reconciled page");
            assert_eq!(attempts, 2, "publication must retry the changed source");
            assert_eq!(
                page.iter().map(|row| &row.id).collect::<Vec<_>>(),
                [&event.id]
            );
            assert_eq!(
                super::super::list_unread(&home, &group, "reader", 1).expect("warm page")[0].id,
                event.id
            );
        }
    }
}
