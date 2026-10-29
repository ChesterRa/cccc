//! The user's open reply requests in one Group: an incremental projection of
//! the ledger, kept from committed byte offsets like the Inbox projection.
//!
//! A request stays open for the user until the user replies to it or it is
//! cancelled, which is the per-event rule `ledger_statuses` reports. Counts
//! and pages come from the same projection. Unlike the lenient readers, it
//! fails on any stored bytes that are not a complete, valid event: a damaged
//! record could be the reply or cancellation that closed a request, and a
//! request must never look open, and accept a second answer, because of it.
//! It also fails on a history the per-event status would read differently: a
//! request id used twice, or a reply or cancellation stored before the
//! request it names.
//!
//! Like every incremental ledger reader, it relies on the ledger being
//! append-only: source identity, length and modification time detect a
//! replaced, truncated or rotated file, not an old byte edited in place
//! without the file system recording it.

use crate::ledger::snapshot::{Location, Snapshot, Source};
use crate::{GroupStore, HomeLayout};
use cccc_contracts::Event;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::io;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// Projections hold ids, not events: the user's requests and what the user's
/// replies name. Many Groups stay warm, so a summary across them does not
/// rebuild each time it is polled.
const CACHE_GROUPS: usize = 256;
pub const MAX_PAGE: usize = 200;
const CURSOR_VERSION: &str = "v1";

/// Whether a message gives the user a reply obligation, as `ledger_statuses`
/// reports it: `user` addressed explicitly, by someone else, and not the
/// source copy of a cross-group send (which tracks remote recipients).
pub fn targets_user(event: &Event) -> bool {
    event.kind == "chat.message" && addresses_user(&event.by, &event.data)
}

/// The same rule for a chat message's sender and data before it is stored.
pub fn addresses_user(by: &str, data: &Map<String, Value>) -> bool {
    by != "user"
        && data
            .get("dst_group_id")
            .and_then(Value::as_str)
            .is_none_or(|group| group.trim().is_empty())
        && data
            .get("to")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|recipient| matches!(recipient, "user" | "@user"))
            })
}

/// Whether a message is a reply request to the user: a message that
/// [`targets_user`] and asks for a reply.
pub fn is_request(event: &Event) -> bool {
    targets_user(event)
        && event.data.get("message_mode").and_then(Value::as_str) == Some("request_reply")
}

/// The request a ledger event closes for the user, if any. Only a reply the
/// user wrote, or a cancellation, closes it; the first one after the request
/// is terminal.
pub fn closes(event: &Event) -> Option<&str> {
    match event.kind.as_str() {
        "chat.message" if event.by == "user" => event.data.get("reply_to"),
        "chat.reply_request.cancelled" => event.data.get("source_event_id"),
        _ => None,
    }
    .and_then(Value::as_str)
    .filter(|id| !id.is_empty())
}

#[derive(Default)]
struct Projection {
    sources: Vec<Source>,
    offsets: Vec<u64>,
    /// Open requests in ledger order, with their timestamps.
    open: BTreeMap<Location, String>,
    locations: HashMap<String, Location>,
    /// Every request seen, open or closed.
    requests: HashSet<String>,
    /// Ids a reply or cancellation named before any request had them.
    closed_early: HashSet<String>,
}

impl Projection {
    fn build(snapshot: &Snapshot) -> io::Result<Self> {
        let mut projection = Self {
            sources: snapshot.sources.clone(),
            ..Self::default()
        };
        projection.offsets = projection.visit(snapshot, &[])?;
        Ok(projection)
    }

    fn visit(&mut self, snapshot: &Snapshot, offsets: &[u64]) -> io::Result<Vec<u64>> {
        #[cfg(test)]
        tests::READS.with(|reads| reads.set(reads.get() + 1));
        let mut failure = None;
        let offsets = snapshot.visit_validated(offsets, |event, location| {
            match self.note(&event, location) {
                Ok(()) => ControlFlow::Continue(()),
                Err(error) => {
                    failure = Some(error);
                    ControlFlow::Break(())
                }
            }
        })?;
        failure.map_or(Ok(offsets), Err)
    }

    /// The per-event status finds a request by its id and closes it with the
    /// first reply or cancellation that names it, wherever that is. Both
    /// readings agree only while ids are unique and closures follow their
    /// request, so a history that breaks either is unreadable here.
    fn note(&mut self, event: &Event, location: Location) -> io::Result<()> {
        let broken = |rule: &str| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("event {} {rule}", event.id),
            )
        };
        if self.requests.contains(&event.id) {
            return Err(broken("reuses the id of a request"));
        }
        if is_request(event) {
            if self.closed_early.contains(&event.id) {
                return Err(broken("follows a reply or cancellation that names it"));
            }
            self.requests.insert(event.id.clone());
            self.locations.insert(event.id.clone(), location);
            self.open.insert(location, event.ts.clone());
        } else if let Some(id) = closes(event) {
            if let Some(location) = self.locations.remove(id) {
                self.open.remove(&location);
            } else if !self.requests.contains(id) {
                self.closed_early.insert(id.to_owned());
            }
        }
        Ok(())
    }
}

/// A projection, or the damage found while building one. A damaged line that
/// ends with its newline stays in an append-only ledger, so that result holds
/// while the ledger only grows and polling does not reread it all to find the
/// damage again. An unfinished last line could still be completed by growth,
/// so that damage is kept only while the sources are unchanged.
enum Cached {
    Ready(Projection),
    Damaged {
        sources: Vec<Source>,
        error: String,
        ended: bool,
    },
}

type Entry = Arc<Mutex<Option<Cached>>>;

fn entry(path: &Path) -> Entry {
    static CACHE: OnceLock<Mutex<VecDeque<(PathBuf, Entry)>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(position) = cache.iter().position(|(key, _)| key == path) {
        let pair = cache.remove(position).expect("entry position");
        let entry = pair.1.clone();
        cache.push_back(pair);
        return entry;
    }
    let entry = Entry::default();
    cache.push_back((path.to_owned(), entry.clone()));
    while cache.len() > CACHE_GROUPS {
        cache.pop_front();
    }
    entry
}

// Growth of the active file is read from the committed offsets; any other
// change to the sources rebuilds. A source that changes while the result is
// read is detected afterwards and the read repeats.
fn with_projection<T>(
    home: &HomeLayout,
    group_id: &str,
    mut inspect: impl FnMut(&Projection, &Snapshot) -> io::Result<T>,
) -> io::Result<T> {
    let path = GroupStore::new(home.clone())?.ledger_path(group_id)?;
    let entry = entry(&path);
    let mut guard = entry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        let snapshot = Snapshot::open(&path)?;
        if snapshot.sources.is_empty() {
            // A Group always has a ledger; never report a missing one as empty.
            return Err(io::Error::new(io::ErrorKind::NotFound, "ledger not found"));
        }
        let result = (|| -> io::Result<T> {
            match guard.as_mut() {
                Some(Cached::Ready(projection)) if snapshot.extends(&projection.sources) => {
                    let previous = projection.offsets.clone();
                    projection.offsets = projection.visit(&snapshot, &previous)?;
                    projection.sources = snapshot.sources.clone();
                }
                Some(Cached::Damaged {
                    sources,
                    error,
                    ended,
                }) if snapshot.sources == *sources || *ended && snapshot.extends(sources) => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, error.clone()));
                }
                _ => *guard = Some(Cached::Ready(Projection::build(&snapshot)?)),
            }
            let Some(Cached::Ready(projection)) = guard.as_ref() else {
                unreachable!("projection built above");
            };
            inspect(projection, &snapshot)
        })();
        if let Err(error) = &result {
            // A failed suffix read may have applied only part of it.
            *guard = (error.kind() == io::ErrorKind::InvalidData).then(|| Cached::Damaged {
                sources: snapshot.sources.clone(),
                error: error.to_string(),
                ended: snapshot.ends_with_newline().unwrap_or(false),
            });
        }
        if !Snapshot::open(&path)?.extends(&snapshot.sources) {
            continue;
        }
        return result;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub open: usize,
    pub oldest_ts: Option<String>,
    /// Changes whenever the ledger's sources change.
    pub revision: String,
}

pub fn summary(home: &HomeLayout, group_id: &str) -> io::Result<Summary> {
    with_projection(home, group_id, |projection, snapshot| {
        Ok(Summary {
            open: projection.open.len(),
            oldest_ts: projection.open.values().next().cloned(),
            revision: revision(&snapshot.sources),
        })
    })
}

#[derive(Debug)]
pub struct Page {
    /// Oldest first, as the ledger orders them.
    pub events: Vec<Event>,
    pub total: usize,
    /// Continues with older requests; `None` when this page reached the oldest.
    pub next: Option<String>,
}

#[derive(Debug)]
pub enum PageError {
    /// The cursor came from a ledger that has since changed other than by
    /// growing (replacement, truncation, compaction). Read again from the top.
    CursorStale,
    InvalidCursor,
    Io(io::Error),
}

impl From<io::Error> for PageError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The newest open requests older than `cursor` (or the newest overall).
pub fn page(
    home: &HomeLayout,
    group_id: &str,
    cursor: Option<&str>,
    limit: usize,
) -> Result<Page, PageError> {
    let cursor = cursor
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
        .map(Cursor::parse)
        .transpose()?;
    let limit = limit.clamp(1, MAX_PAGE);
    let mut stale = false;
    let page = with_projection(home, group_id, |projection, snapshot| {
        stale = false;
        let before = match &cursor {
            Some(cursor) if !cursor.extended_by(snapshot) => {
                stale = true;
                return Ok(None);
            }
            Some(cursor) => Some(cursor.location),
            None => None,
        };
        let older = projection
            .open
            .keys()
            .copied()
            .filter(|location| before.is_none_or(|before| *location < before))
            .collect::<Vec<_>>();
        let start = older.len().saturating_sub(limit);
        let selected = &older[start..];
        let events = snapshot.events(selected)?;
        let next = (start > 0)
            .then(|| {
                selected
                    .first()
                    .map(|first| Cursor::encode(snapshot, *first))
            })
            .flatten();
        Ok(Some(Page {
            events,
            total: projection.open.len(),
            next,
        }))
    })?;
    if stale {
        return Err(PageError::CursorStale);
    }
    page.ok_or(PageError::CursorStale)
}

fn modified_nanos(source: &Source) -> u128 {
    source
        .modified
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn revision(sources: &[Source]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for source in sources {
        source.identity.hash(&mut hasher);
        source.len.hash(&mut hasher);
        modified_nanos(source).hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// A page position bound to the sources it was read from: every source's
/// identity, length and modification time, and the active file's index.
struct Cursor {
    location: Location,
    active: Option<usize>,
    sources: Vec<((u64, u64), u64, u128)>,
}

impl Cursor {
    fn encode(snapshot: &Snapshot, location: Location) -> String {
        let sources = snapshot
            .sources
            .iter()
            .map(|source| {
                format!(
                    "{:x}-{:x}-{:x}-{:x}",
                    source.identity.0,
                    source.identity.1,
                    source.len,
                    modified_nanos(source)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let active = snapshot
            .active_index()
            .map_or_else(|| "-".to_owned(), |index| index.to_string());
        format!(
            "{CURSOR_VERSION}.{}.{}.{}.{active}.{sources}",
            location.source, location.offset, location.end
        )
    }

    fn parse(value: &str) -> Result<Self, PageError> {
        let invalid = || PageError::InvalidCursor;
        let mut parts = value.splitn(6, '.');
        if parts.next() != Some(CURSOR_VERSION) {
            return Err(invalid());
        }
        let mut number = || -> Result<u64, PageError> {
            parts
                .next()
                .and_then(|part| part.parse().ok())
                .ok_or_else(invalid)
        };
        let location = Location {
            source: usize::try_from(number()?).map_err(|_| invalid())?,
            offset: number()?,
            end: number()?,
        };
        let active = match parts.next().ok_or_else(invalid)? {
            "-" => None,
            index => Some(index.parse().map_err(|_| invalid())?),
        };
        let sources = parts
            .next()
            .ok_or_else(invalid)?
            .split(',')
            .filter(|source| !source.is_empty())
            .map(|source| {
                let fields = source
                    .split('-')
                    .map(|field| u128::from_str_radix(field, 16).ok())
                    .collect::<Option<Vec<_>>>()
                    .filter(|fields| fields.len() == 4)
                    .ok_or_else(invalid)?;
                let narrow = |value: u128| u64::try_from(value).map_err(|_| invalid());
                Ok((
                    (narrow(fields[0])?, narrow(fields[1])?),
                    narrow(fields[2])?,
                    fields[3],
                ))
            })
            .collect::<Result<Vec<_>, PageError>>()?;
        Ok(Self {
            location,
            active,
            sources,
        })
    }

    /// The same rule as `Snapshot::extends`: only the active file may grow,
    /// and a source of the same length must be unchanged.
    fn extended_by(&self, snapshot: &Snapshot) -> bool {
        snapshot.active_index() == self.active
            && snapshot.sources.len() == self.sources.len()
            && snapshot.sources.iter().zip(&self.sources).enumerate().all(
                |(index, (next, &(identity, len, modified)))| {
                    next.identity == identity
                        && (Some(index) == self.active && next.len > len
                            || next.len == len && modified_nanos(next) == modified)
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger;
    use serde_json::json;
    use std::fs;

    struct Fixture {
        _temp: tempfile::TempDir,
        home: HomeLayout,
        group_id: String,
        ledger: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("fixture");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let group = store.create("user requests", "").expect("group");
            let ledger = store.ledger_path(&group.group_id).expect("ledger");
            Self {
                _temp: temp,
                home,
                group_id: group.group_id,
                ledger,
            }
        }

        fn append(&self, kind: &str, by: &str, data: Value) -> Event {
            let mut event = Event::new(kind, &self.group_id);
            event.by = by.into();
            event.data = data.as_object().cloned().expect("data");
            ledger::append(&self.ledger, &event).expect("append");
            event
        }

        fn request(&self, text: &str) -> Event {
            self.append(
                "chat.message",
                "peer",
                json!({"text": text, "to": ["user"], "message_mode": "request_reply"}),
            )
        }

        fn reply(&self, by: &str, to: &Event) -> Event {
            self.append(
                "chat.message",
                by,
                json!({"text": "ok", "to": [to.by], "message_mode": "send", "reply_to": to.id}),
            )
        }

        fn append_event(&self, event: &Event) {
            ledger::append(&self.ledger, event).expect("append");
        }

        fn summary(&self) -> Summary {
            summary(&self.home, &self.group_id).expect("summary")
        }

        fn page(&self, cursor: Option<&str>, limit: usize) -> Result<Page, PageError> {
            page(&self.home, &self.group_id, cursor, limit)
        }
    }

    thread_local! {
        /// Ledger reads by this thread's projections.
        pub(super) static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn ids(page: &Page) -> Vec<&str> {
        page.events.iter().map(|event| event.id.as_str()).collect()
    }

    #[test]
    fn only_requests_to_the_user_stay_open_until_the_user_replies_or_one_is_cancelled() {
        let f = Fixture::new();
        let answered = f.request("answered");
        let cancelled = f.request("cancelled");
        let open = f.request("open");
        f.append(
            "chat.message",
            "peer",
            json!({"text": "not a request", "to": ["user"], "message_mode": "send"}),
        );
        f.append(
            "chat.message",
            "peer",
            json!({"text": "to an agent", "to": ["lead"], "message_mode": "request_reply"}),
        );
        f.append(
            "chat.message",
            "user",
            json!({"text": "from the user", "to": ["user"], "message_mode": "request_reply"}),
        );
        f.append(
            "chat.message",
            "peer",
            json!({"text": "source copy", "to": ["user"], "message_mode": "request_reply", "dst_group_id": "g_other"}),
        );
        f.reply("lead", &open);
        f.reply("user", &answered);
        f.append(
            "chat.reply_request.cancelled",
            "user",
            json!({"source_event_id": cancelled.id}),
        );
        let summary = f.summary();
        assert_eq!(summary.open, 1);
        assert_eq!(summary.oldest_ts.as_deref(), Some(open.ts.as_str()));
        assert_eq!(ids(&f.page(None, 50).expect("page")), [open.id.as_str()]);
    }

    #[test]
    fn appended_requests_and_closures_update_the_warm_projection() {
        let f = Fixture::new();
        let first = f.request("first");
        let before = f.summary();
        assert_eq!(before.open, 1);
        let second = f.request("second");
        f.reply("user", &first);
        let after = f.summary();
        assert_eq!(after.open, 1);
        assert_ne!(after.revision, before.revision);
        assert_eq!(ids(&f.page(None, 50).expect("page")), [second.id.as_str()]);
    }

    #[test]
    fn pages_walk_back_from_the_newest_and_survive_closures_and_appends() {
        let f = Fixture::new();
        let requests = (0..5)
            .map(|index| f.request(&format!("request {index}")))
            .collect::<Vec<_>>();
        let first = f.page(None, 2).expect("first page");
        assert_eq!(first.total, 5);
        assert_eq!(
            ids(&first),
            [requests[3].id.as_str(), requests[4].id.as_str()]
        );
        // The cursor's own request closes and a new one arrives between pages.
        f.reply("user", &requests[3]);
        f.request("newer");
        let second = f.page(first.next.as_deref(), 2).expect("second page");
        assert_eq!(
            ids(&second),
            [requests[1].id.as_str(), requests[2].id.as_str()]
        );
        let third = f.page(second.next.as_deref(), 2).expect("third page");
        assert_eq!(ids(&third), [requests[0].id.as_str()]);
        assert!(third.next.is_none());
    }

    #[test]
    fn stored_bytes_that_are_not_a_complete_event_make_the_requests_unreadable() {
        let damaged: [&[u8]; 5] = [
            b"{\"kind\": \"chat.reply_request.cancelled\", broken\n",
            b"[1, 2]\n",
            b"{\"not\": \"an event\"}\n",
            b"{\"kind\": \"chat.reply_request.cancelled\", \"data\": {}}\n",
            // A cut tail could have been the closing record.
            b"{\"kind\": \"chat.message\", \"by\": \"us",
        ];
        for bytes in damaged {
            let f = Fixture::new();
            f.request("open");
            assert_eq!(f.summary().open, 1);
            {
                use std::io::Write;
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&f.ledger)
                    .expect("ledger");
                file.write_all(bytes).expect("damage");
            }
            let error = summary(&f.home, &f.group_id).expect_err("damaged ledger");
            assert_eq!(
                error.kind(),
                io::ErrorKind::InvalidData,
                "{}",
                String::from_utf8_lossy(bytes)
            );
            assert!(matches!(
                f.page(None, 10),
                Err(PageError::Io(error)) if error.kind() == io::ErrorKind::InvalidData
            ));
        }

        // A complete event that only lacks its final newline is still read,
        // and the next append keeps both records whole.
        let f = Fixture::new();
        let mut event = Event::new("chat.message", &f.group_id);
        event.by = "peer".into();
        event.data =
            json!({"text": "unterminated", "to": ["user"], "message_mode": "request_reply"})
                .as_object()
                .cloned()
                .expect("data");
        fs::write(&f.ledger, serde_json::to_vec(&event).expect("event")).expect("ledger");
        assert_eq!(f.summary().open, 1);
        f.request("next");
        assert_eq!(f.summary().open, 2);
    }

    #[test]
    fn an_unfinished_last_line_is_unreadable_until_growth_completes_it() {
        use std::io::Write;
        let f = Fixture::new();
        f.request("first");
        let mut second = Event::new("chat.message", &f.group_id);
        second.by = "peer".into();
        second.data = json!({"text": "second", "to": ["user"], "message_mode": "request_reply"})
            .as_object()
            .cloned()
            .expect("data");
        let mut line = serde_json::to_vec(&second).expect("encode");
        line.push(b'\n');
        let (head, tail) = line.split_at(line.len() / 2);
        let append = |bytes: &[u8]| {
            fs::OpenOptions::new()
                .append(true)
                .open(&f.ledger)
                .and_then(|mut file| file.write_all(bytes))
                .expect("append");
        };
        // Appends hold the writer lock, so a half line is never a write in
        // progress: it fails closed, but growth may still complete it.
        append(head);
        for _ in 0..2 {
            let error = summary(&f.home, &f.group_id).expect_err("unfinished");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }
        append(tail);
        assert_eq!(f.summary().open, 2);
    }

    #[test]
    fn found_damage_is_kept_while_the_ledger_grows_and_dropped_once_it_is_replaced() {
        use std::io::Write;
        let f = Fixture::new();
        f.request("first");
        let healthy = fs::read(&f.ledger).expect("ledger");
        fs::OpenOptions::new()
            .append(true)
            .open(&f.ledger)
            .and_then(|mut file| file.write_all(b"{\"kind\": broken\n"))
            .expect("damage");
        let damaged = |f: &Fixture| summary(&f.home, &f.group_id).expect_err("damaged").kind();
        assert_eq!(damaged(&f), io::ErrorKind::InvalidData);
        // Growth cannot repair an earlier record: polling reports the damage
        // without reading the ledger again.
        f.request("second");
        let reads = READS.with(std::cell::Cell::get);
        assert_eq!(damaged(&f), io::ErrorKind::InvalidData);
        assert_eq!(READS.with(std::cell::Cell::get), reads);
        // A repaired ledger is a replaced file, and is read again.
        let repaired = f.ledger.with_extension("repaired");
        fs::write(&repaired, &healthy).expect("repaired copy");
        fs::rename(&repaired, &f.ledger).expect("replace");
        assert_eq!(f.summary().open, 1);
    }

    #[test]
    fn a_reused_request_id_or_a_closure_before_its_request_makes_the_requests_unreadable() {
        // Each history breaks a rule the per-event status relies on: ids are
        // unique, and a reply or cancellation follows the request it closes.
        fn reused(f: &Fixture) {
            let request = f.request("asked");
            assert_eq!(f.summary().open, 1);
            f.reply("user", &request);
            f.append_event(&request);
        }
        fn shadowed(f: &Fixture) {
            let request = f.request("asked");
            assert_eq!(f.summary().open, 1);
            let mut other = Event::new("chat.message", &f.group_id);
            other.id = request.id.clone();
            other.by = "peer".into();
            other.data = json!({"text": "note", "to": ["lead"]})
                .as_object()
                .cloned()
                .expect("data");
            f.append_event(&other);
        }
        fn early(f: &Fixture) {
            let mut request = Event::new("chat.message", &f.group_id);
            request.by = "peer".into();
            request.data =
                json!({"text": "asked", "to": ["user"], "message_mode": "request_reply"})
                    .as_object()
                    .cloned()
                    .expect("data");
            f.reply("user", &request);
            assert_eq!(f.summary().open, 0);
            f.append_event(&request);
        }
        fn unreadable(name: &str, write: fn(&Fixture)) {
            let f = Fixture::new();
            write(&f);
            // The warm projection reads the suffix; the next call reuses the
            // damage it found.
            for _ in 0..2 {
                let error = summary(&f.home, &f.group_id).expect_err(name);
                assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{name}");
            }
            assert!(
                matches!(
                    f.page(None, 10),
                    Err(PageError::Io(error)) if error.kind() == io::ErrorKind::InvalidData
                ),
                "{name}"
            );
        }
        unreadable("reused", reused);
        unreadable("shadowed", shadowed);
        unreadable("early", early);
    }

    #[test]
    fn a_cursor_from_a_replaced_or_compacted_ledger_is_stale() {
        let f = Fixture::new();
        for index in 0..3 {
            f.request(&format!("request {index}"));
        }
        let first = f.page(None, 1).expect("first page");
        let cursor = first.next.clone().expect("cursor");
        // Rewriting the ledger with the same events keeps every id but not
        // the sources the cursor was read from.
        let bytes = fs::read(&f.ledger).expect("ledger");
        {
            let _lock = ledger::acquire_writer_lock(&f.ledger).expect("writer");
            let replacement = f.ledger.with_extension("replacement");
            fs::write(&replacement, &bytes).expect("replacement");
            fs::rename(&replacement, &f.ledger).expect("replace");
        }
        assert!(matches!(
            f.page(Some(&cursor), 1),
            Err(PageError::CursorStale)
        ));
        let fresh = f.page(None, 1).expect("fresh page");
        crate::ledger_archive::compact(&f.home, &f.group_id, "test").expect("compact");
        assert!(matches!(
            f.page(fresh.next.as_deref(), 1),
            Err(PageError::CursorStale)
        ));
        // Compaction keeps the history, so a fresh read still sees it all.
        assert_eq!(f.summary().open, 3);
        assert!(matches!(
            f.page(Some("v1.garbage"), 1),
            Err(PageError::InvalidCursor)
        ));
    }

    #[test]
    fn a_cursor_from_a_file_changed_in_place_without_growing_is_stale() {
        use std::io::Write;
        let f = Fixture::new();
        for index in 0..3 {
            f.request(&format!("request {index}"));
        }
        let cursor = f.page(None, 1).expect("first page").next.expect("cursor");
        // The same file and length with other bytes, as the file system
        // records it: only growth keeps a cursor, as it keeps a snapshot.
        let modified = fs::metadata(&f.ledger)
            .and_then(|metadata| metadata.modified())
            .expect("modified");
        let mut bytes = fs::read(&f.ledger).expect("ledger");
        let at = bytes
            .windows(9)
            .position(|window| window == b"request 0")
            .expect("text");
        bytes[at + 8] = b'X';
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&f.ledger)
            .expect("ledger");
        file.write_all(&bytes).expect("rewrite");
        file.set_modified(modified + std::time::Duration::from_secs(1))
            .expect("modified");
        drop(file);
        assert!(matches!(
            f.page(Some(&cursor), 1),
            Err(PageError::CursorStale)
        ));
        assert_eq!(f.summary().open, 3);
    }
}
