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
    append_epoch: AtomicU64,
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
    json!(
        group
            .actors
            .iter()
            .map(|actor| (
                &actor.id,
                effective_role(group, &actor.id),
                &actor.internal_kind
            ))
            .collect::<Vec<_>>()
    )
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
        // Both passes use the same pinned FDs and bounds, with no ledger lock.
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
    inspect: impl FnOnce(&Projection, &Snapshot) -> io::Result<T>,
) -> io::Result<T> {
    let path = GroupStore::new(home.clone())?.ledger_path(&group.group_id)?;
    let entry = entry(&path);
    let mut guard = entry
        .projection
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut state = state.clone();
    loop {
        let append_epoch = entry.append_epoch.load(Ordering::Acquire);
        let cursor_epoch = entry.cursor_epoch.load(Ordering::Acquire);
        let snapshot = Snapshot::open(&path)?;
        let refresh = (|| -> io::Result<()> {
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
            Ok(())
        })();
        if let Err(error) = refresh {
            // A failed suffix read/refill may have reduced only a prefix.
            // Never retry it into the same partially advanced cache.
            *guard = None;
            return Err(error);
        }
        // Validate publication after decoding, not only before it. Appends
        // skipped by try_lock are merged by byte offset on the next iteration.
        // Rotation/replacement causes a new inode/order token and a rebuild.
        let current = Snapshot::open(&path)?;
        let current_state = load(home, &group.group_id)?;
        if current.sources != snapshot.sources
            || current_state.cursors != state.cursors
            || entry.append_epoch.load(Ordering::Acquire) != append_epoch
            || entry.cursor_epoch.load(Ordering::Acquire) != cursor_epoch
        {
            state = current_state;
            continue;
        }
        return inspect(guard.as_ref().expect("projection initialized"), &snapshot);
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
        actor_ids
            .iter()
            .map(|actor| {
                let messages = projection.boxes[actor]
                    .page
                    .iter()
                    .take(limit.min(PAGE_SIZE))
                    .map(|mail| snapshot.event(mail.location))
                    .collect::<io::Result<Vec<_>>>()?;
                Ok((actor.clone(), messages))
            })
            .collect()
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
            mail_delays,
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
        projection
            .deliveries
            .pending(actor, limit)
            .into_iter()
            .map(|location| snapshot.event(location))
            .collect()
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
    entry.append_epoch.fetch_add(1, Ordering::Release);
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
