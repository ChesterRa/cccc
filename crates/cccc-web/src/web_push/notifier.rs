//! Follows every Group's ledger while a browser is subscribed, and notifies
//! the subscriptions of the messages and reply requests addressed to the
//! user while this Web service runs.
//!
//! Each Group is followed at its own ledger position. The shared event feed
//! only says which ledger changed: it replays history a reader has not seen,
//! such as a newly imported Group's. A binding begins after the newest event
//! each Group had when it was made, and every batch is read before the
//! bindings it goes to, so an event added after a binding was saved reaches
//! it and nothing older does. A Group that appears while following is read
//! from after its import, or from its start while its whole history is
//! short: history is never announced.
//!
//! A message is sent once, best effort. A request a push service could not
//! take is offered again, with backoff, while it stays open and this process
//! runs. Nothing is replayed after a restart, and nothing runs until a
//! browser subscribes.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, Event, reply_question};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::ledger::{self, LedgerFollower};
use cccc_core::{GroupStore, HomeLayout, user_requests};
use futures_util::future::join_all;
use serde_json::{Map, Value, json};
use tokio::sync::{Notify, broadcast};
use tokio::time::Instant;

use super::crypto::VapidKey;
use super::sender::{Outcome, Sender};
use super::store::{Owner, PushStore, Subscription, is_page_origin};
use crate::ledger_event_hub::LedgerEventHub;

const TITLE_CHARS: usize = 80;
const BODY_CHARS: usize = 140;
const FIRST_RETRY: Duration = Duration::from_secs(60);
const LAST_RETRY: Duration = Duration::from_secs(30 * 60);
const SCAN_PAGE: u64 = 200;
/// How often every followed ledger is read again, for a change the feed did
/// not report, and the Groups are listed for new ones.
const SWEEP: Duration = Duration::from_secs(30);
/// How often a process that does not send tries to become the sender.
const CLAIM_RETRY: Duration = Duration::from_secs(60);
/// The newest events looked at when a Group starts being followed, and the
/// ids kept per Group after: a ledger read again from an earlier point is
/// cut after the last one already seen.
const SEEN: usize = 128;
/// A Group that appears without an import among its newest events is read
/// from its start only when its whole history is this short; a longer one
/// was not just created.
const NEW_GROUP_EVENTS: usize = 64;
/// Requests kept per subscription for another try; past this the oldest go.
const MAX_PENDING: usize = 10;
/// Events read at a time when a binding starts further back than the newest.
const CATCH_UP_PAGE: usize = 512;

/// Whether this Web process can send, shared with the routes.
#[derive(Default)]
pub(crate) struct Runtime {
    /// Wakes the notifier when a browser subscribes.
    pub(crate) wake: Notify,
    state: AtomicU8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SenderState {
    Starting,
    /// This process sends.
    Here,
    /// Another Web process for this CCCC_HOME sends; this one takes over if
    /// that one stops.
    Elsewhere,
    /// Nothing can send from here: no claim or no client could be made.
    Unavailable,
}

impl SenderState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Here => "here",
            Self::Elsewhere => "elsewhere",
            Self::Unavailable => "unavailable",
        }
    }
}

impl Runtime {
    pub(crate) fn state(&self) -> SenderState {
        match self.state.load(Ordering::Relaxed) {
            1 => SenderState::Here,
            2 => SenderState::Elsewhere,
            3 => SenderState::Unavailable,
            _ => SenderState::Starting,
        }
    }

    pub(crate) fn set(&self, state: SenderState) {
        let value = match state {
            SenderState::Starting => 0,
            SenderState::Here => 1,
            SenderState::Elsewhere => 2,
            SenderState::Unavailable => 3,
        };
        self.state.store(value, Ordering::Relaxed);
    }
}

pub(crate) fn spawn(
    home: HomeLayout,
    events: LedgerEventHub,
    runtime: Arc<Runtime>,
    shutdown: broadcast::Receiver<()>,
) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        runtime.set(SenderState::Unavailable);
        return;
    };
    handle.spawn(run(home, events, runtime, shutdown));
}

async fn run(
    home: HomeLayout,
    events: LedgerEventHub,
    runtime: Arc<Runtime>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let sender = match Sender::new() {
        Ok(sender) => sender,
        Err(error) => {
            tracing::warn!(%error, "Web Push notifications are unavailable");
            runtime.set(SenderState::Unavailable);
            return;
        }
    };
    let Some(_claim) = take_claim(&home, &runtime, &mut shutdown).await else {
        return;
    };
    let mut notifier = Notifier::new(home, sender);
    loop {
        match notifier.subscribed() {
            Ok(true) => {}
            // Asleep until a browser subscribes. Another Web process may
            // save the binding, so the store is looked at again from time
            // to time; what was written meanwhile follows its starting point.
            _ => {
                tokio::select! {
                    _ = shutdown.recv() => return,
                    () = runtime.wake.notified() => {}
                    () = tokio::time::sleep(SWEEP) => {}
                }
                continue;
            }
        }
        let mut feed = events.subscribe_global();
        notifier.activate().await;
        let mut sweep = tokio::time::interval_at(Instant::now() + SWEEP, SWEEP);
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        while notifier.active {
            let retry = notifier.retry;
            tokio::select! {
                _ = shutdown.recv() => return,
                // A new binding is taken up by the next batch it is owed.
                () = runtime.wake.notified() => {}
                () = until(retry) => notifier.retry_pending().await,
                _ = sweep.tick() => notifier.sweep().await,
                received = feed.recv() => match received {
                    Ok(event) => notifier.changed(&event).await,
                    Err(broadcast::error::RecvError::Lagged(_)) => notifier.sweep().await,
                    Err(broadcast::error::RecvError::Closed) => return,
                },
            }
        }
        notifier.sleep();
    }
}

/// Become the one sender for this CCCC_HOME. While another Web process
/// holds the claim this one waits, and takes over when that process stops.
async fn take_claim(
    home: &HomeLayout,
    runtime: &Runtime,
    shutdown: &mut broadcast::Receiver<()>,
) -> Option<File> {
    loop {
        match claim(home) {
            Ok(file) => {
                runtime.set(SenderState::Here);
                return Some(file);
            }
            Err(Claim::Held) => runtime.set(SenderState::Elsewhere),
            Err(Claim::Failed(error)) => {
                if runtime.state() != SenderState::Unavailable {
                    tracing::warn!(%error, "Web Push notifications cannot be sent from here");
                }
                runtime.set(SenderState::Unavailable);
            }
        }
        tokio::select! {
            _ = shutdown.recv() => return None,
            () = tokio::time::sleep(CLAIM_RETRY) => {}
        }
    }
}

async fn until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

enum Claim {
    /// Another process holds it.
    Held,
    Failed(io::Error),
}

fn claim(home: &HomeLayout) -> Result<File, Claim> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.root().join("web_push.sender.lock"))
        .map_err(Claim::Failed)?;
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(file),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Err(Claim::Held)
        }
        Err(error) => Err(Claim::Failed(error)),
    }
}

/// Where a binding made now starts: the newest event of each registered
/// Group in `visible` (every Group when `None`), or an empty id for one with
/// nothing written yet. A Group without an entry was not visible then.
///
/// A registered Group whose directory is gone (being deleted, or left in
/// the registry by an interrupted delete) gets no entry rather than failing
/// every subscription. If it comes back, a Group the notifier was following
/// resumes after the last event it handled, and one it never followed is
/// read as a Group that appeared: either way, as for every binding.
pub(crate) fn starting_points(
    home: &HomeLayout,
    visible: Option<&HashSet<String>>,
) -> io::Result<BTreeMap<String, String>> {
    let store = GroupStore::new(home.clone())?;
    let mut since = BTreeMap::new();
    for meta in store.list()? {
        if visible.is_some_and(|visible| !visible.contains(&meta.group_id)) {
            continue;
        }
        let path = store.ledger_path(&meta.group_id)?;
        let newest = match ledger::tail(&path, 1) {
            Ok(mut events) => events.pop().map(|event| event.id).unwrap_or_default(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if store.group_dir(&meta.group_id)?.exists() {
                    return Err(error);
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        since.insert(meta.group_id, newest);
    }
    Ok(since)
}

/// The user's open reply requests, read Group by Group and page by page, so
/// only one page of events is held at a time. It records which Groups were
/// read to the end.
struct Scan {
    groups: Vec<String>,
    /// The Group being read and the cursor of its next page.
    reading: Option<(String, Option<String>)>,
    complete: HashSet<String>,
}

impl Scan {
    /// Start reading `groups`, or every Group when `None`.
    async fn start(client: &DaemonClient, groups: Option<&[String]>) -> Option<Self> {
        let mut args = Map::new();
        if let Some(groups) = groups {
            args.insert("group_ids".into(), json!(groups));
        }
        let summary = call(client, "user_reply_requests_summary", args).await?;
        let mut scan = Self {
            groups: Vec::new(),
            reading: None,
            complete: HashSet::new(),
        };
        for group in summary.get("groups")?.as_array()? {
            let Some(group_id) = group.get("group_id").and_then(Value::as_str) else {
                continue;
            };
            match group.get("pending").and_then(Value::as_u64) {
                Some(0) => {
                    scan.complete.insert(group_id.to_owned());
                }
                Some(_) => scan.groups.push(group_id.to_owned()),
                // A deleted Group has no request left to answer.
                None if group.get("error").and_then(Value::as_str) == Some("group_not_found") => {
                    scan.complete.insert(group_id.to_owned());
                }
                None => {}
            }
        }
        scan.groups.reverse();
        Some(scan)
    }

    /// The next page of open requests, or `None` when every Group is read.
    async fn next(&mut self, client: &DaemonClient) -> Option<Vec<Event>> {
        loop {
            let (group_id, before) = match self.reading.take() {
                Some(reading) => reading,
                None => (self.groups.pop()?, None),
            };
            let mut args = Map::new();
            args.insert("group_id".into(), json!(group_id));
            args.insert("limit".into(), json!(SCAN_PAGE));
            if let Some(cursor) = &before {
                args.insert("before".into(), json!(cursor));
            }
            let page = call(client, "user_reply_requests", args)
                .await
                .and_then(|page| request_page(&page));
            let Some((events, more, next)) = page else {
                continue;
            };
            match (more, next) {
                (false, _) => {
                    self.complete.insert(group_id);
                }
                (true, Some(cursor)) if cursor_advances(before.as_deref(), &cursor) => {
                    self.reading = Some((group_id, Some(cursor)));
                }
                // More pages without an advancing cursor: not read to the end.
                (true, _) => {}
            }
            return Some(events);
        }
    }
}

/// A page of open requests: its events, whether more follow, and where. One
/// that does not say whether more follow is not read to the end.
fn request_page(page: &Map<String, Value>) -> Option<(Vec<Event>, bool, Option<String>)> {
    let events: Vec<Event> = serde_json::from_value(page.get("events")?.clone()).ok()?;
    let more = page.get("has_more")?.as_bool()?;
    let next = page
        .get("before")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some((events, more, next))
}

fn cursor_advances(before: Option<&str>, next: &str) -> bool {
    let next = next.trim();
    !next.is_empty() && before.map(str::trim) != Some(next)
}

async fn call(
    client: &DaemonClient,
    op: &str,
    args: Map<String, Value>,
) -> Option<Map<String, Value>> {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        })
        .await
        .ok()?;
    if !response.ok {
        return None;
    }
    Some(response.result)
}

/// One Group, followed at its own position in its ledger.
struct Followed {
    path: PathBuf,
    follower: LedgerFollower,
    seen: Seen,
}

/// The newest event ids read from one ledger.
#[derive(Default)]
struct Seen {
    order: VecDeque<String>,
    ids: HashSet<String>,
}

impl Seen {
    fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    fn newest(&self) -> Option<&String> {
        self.order.back()
    }

    fn record(&mut self, id: &str) {
        if !self.ids.insert(id.to_owned()) {
            return;
        }
        self.order.push_back(id.to_owned());
        if self.order.len() > SEEN
            && let Some(oldest) = self.order.pop_front()
        {
            self.ids.remove(&oldest);
        }
    }

    /// The events after the last one already read: a ledger read again from
    /// an earlier point repeats what came before it.
    fn after_known(&self, mut events: Vec<Event>) -> Vec<Event> {
        if let Some(last) = events.iter().rposition(|event| self.contains(&event.id)) {
            events.drain(..=last);
        }
        events
    }
}

/// Where announcements begin when a Group starts being followed.
#[derive(Clone, Debug)]
enum Resume {
    /// At its end: a Group that existed when the notifier woke.
    End,
    /// An initial end was read but the rest of startup failed. Existing
    /// bindings continue after this point; newer bindings keep their since.
    EndAfter(String),
    /// After the last event handled before its ledger could not be read.
    After(String),
    /// A Group that appeared while following.
    Start,
    /// A Group seen with nothing written: all of it is newer.
    Beginning,
}

/// One event a binding is owed.
type Owed = (String, Arc<Event>);

/// What happened when one binding was sent its notifications.
#[derive(Default)]
struct Report {
    binding: String,
    /// The push service no longer knows it, or its token is gone.
    gone: bool,
    /// Its push service failed; the rest waited for the next try.
    failed: bool,
    /// Its push service answered a try.
    answered: bool,
    /// Requests settled: accepted, refused for good, or no longer visible.
    done: Vec<String>,
    /// Requests to offer again.
    keep: Vec<String>,
}

struct Notifier {
    home: HomeLayout,
    store: PushStore,
    client: DaemonClient,
    sender: Sender,
    /// Bindings stored when this process started, or when it first could
    /// read them: they begin where it does. Unknown until then.
    earlier: Option<HashSet<String>>,
    groups: HashMap<String, Followed>,
    /// Groups not followed yet, with where to begin: an empty ledger, or one
    /// that could not be read. Tried again on every sweep.
    waiting: HashMap<String, Resume>,
    /// Whether the registered Groups were listed since waking. Until then a
    /// Group the feed names is not taken for a new one.
    listed: bool,
    /// Bindings that reached their starting point in a Group.
    passed: HashSet<(String, String)>,
    /// Requests per binding that a push service did not take yet.
    pending: HashMap<String, Vec<String>>,
    /// Bindings whose push service failed: nothing is sent to them until
    /// their next try, and their requests wait in `pending`.
    broken: HashSet<String>,
    /// When each binding with something to try again is tried: a broken one,
    /// or one with requests kept. Each has its own, so one push service that
    /// keeps failing does not hold back another.
    due: HashMap<String, Instant>,
    /// Tries in a row that a binding's push service failed; it waits longer
    /// after each, and a try that reaches the service starts it over.
    failures: HashMap<String, u32>,
    /// The earliest due try.
    retry: Option<Instant>,
    /// Cleared when no subscription is left, which stops the following.
    active: bool,
}

/// Whether a subscription may be sent a notification about a Group now.
enum Permit {
    Send,
    Skip,
    /// The binding's token is gone; the binding goes with it.
    Gone,
    /// Its state could not be read; a request is tried again later.
    Unknown,
}

impl Notifier {
    fn new(home: HomeLayout, sender: Sender) -> Self {
        let mut notifier = Self {
            client: DaemonClient::new(home.clone()),
            store: PushStore::new(home.clone()),
            home,
            sender,
            earlier: None,
            groups: HashMap::new(),
            waiting: HashMap::new(),
            listed: false,
            passed: HashSet::new(),
            pending: HashMap::new(),
            broken: HashSet::new(),
            due: HashMap::new(),
            failures: HashMap::new(),
            retry: None,
            active: false,
        };
        // Bindings that cannot be read now are not taken for none: they
        // would be sent what was written before this process started.
        if let Err(error) = notifier.snapshot() {
            tracing::warn!(%error, "could not read the Web Push subscriptions");
        }
        notifier
    }

    /// The key and the bindings stored now. The first read that succeeds
    /// tells which bindings this process found.
    fn snapshot(&mut self) -> io::Result<Option<(VapidKey, Vec<Subscription>)>> {
        let snapshot = self.store.snapshot()?;
        if self.earlier.is_none() {
            self.earlier = Some(
                snapshot
                    .iter()
                    .flat_map(|(_, items)| items)
                    .map(|item| item.binding.clone())
                    .collect(),
            );
        }
        Ok(snapshot)
    }

    fn stored_before(&self, binding: &str) -> bool {
        self.earlier
            .as_ref()
            .is_some_and(|earlier| earlier.contains(binding))
    }

    fn subscribed(&mut self) -> io::Result<bool> {
        Ok(self.snapshot()?.is_some_and(|(_, items)| !items.is_empty()))
    }

    /// The key and the bindings now. With none left the notifier sleeps.
    /// What is kept for a binding no longer stored is let go.
    fn bindings(&mut self) -> Option<(VapidKey, Vec<Subscription>)> {
        match self.snapshot() {
            Ok(Some((key, items))) if !items.is_empty() => {
                let current = items
                    .iter()
                    .map(|item| item.binding.as_str())
                    .collect::<HashSet<_>>();
                self.passed
                    .retain(|(binding, _)| current.contains(binding.as_str()));
                self.pending
                    .retain(|binding, _| current.contains(binding.as_str()));
                self.broken
                    .retain(|binding| current.contains(binding.as_str()));
                self.due
                    .retain(|binding, _| current.contains(binding.as_str()));
                self.failures
                    .retain(|binding, _| current.contains(binding.as_str()));
                self.reschedule();
                Some((key, items))
            }
            Ok(_) => {
                self.active = false;
                None
            }
            Err(error) => {
                tracing::warn!(%error, "could not read the Web Push subscriptions");
                None
            }
        }
    }

    /// Start following every registered Group at its end.
    async fn activate(&mut self) {
        self.active = true;
        let Some((key, subscriptions)) = self.bindings() else {
            return;
        };
        let mut owed = Vec::new();
        match GroupStore::new(self.home.clone()).and_then(|store| store.list()) {
            Ok(groups) => {
                self.listed = true;
                for meta in groups {
                    self.start(&meta.group_id, Resume::End, &subscriptions, &mut owed);
                }
            }
            Err(error) => tracing::warn!(%error, "could not list the Groups to follow"),
        }
        self.send(&key, &subscriptions, owed).await;
    }

    fn sleep(&mut self) {
        self.groups.clear();
        self.waiting.clear();
        self.passed.clear();
        self.pending.clear();
        self.broken.clear();
        self.due.clear();
        self.failures.clear();
        self.listed = false;
        self.retry = None;
    }

    /// A ledger the feed reported as changed.
    async fn changed(&mut self, event: &Event) {
        let group_id = event.group_id.clone();
        if event.kind == "group.deleted" {
            self.groups.remove(&group_id);
            self.waiting.remove(&group_id);
            self.passed.retain(|(_, group)| *group != group_id);
            // Its requests can no longer be answered.
            for requests in self.pending.values_mut() {
                requests.retain(|request| request_group(request) != Some(group_id.as_str()));
            }
            self.pending.retain(|_, requests| !requests.is_empty());
            self.reschedule();
            return;
        }
        if self.groups.contains_key(&group_id) {
            self.follow(&group_id).await;
        } else if self.listed {
            let resume = self
                .waiting
                .get(&group_id)
                .cloned()
                .unwrap_or(Resume::Start);
            self.begin(&group_id, resume).await;
        }
    }

    /// Read every followed ledger again, and list the Groups for any that
    /// appeared or could not be followed before.
    async fn sweep(&mut self) {
        if let Ok(false) = self.subscribed() {
            self.active = false;
            return;
        }
        let followed = self.groups.keys().cloned().collect::<Vec<_>>();
        for group_id in followed {
            self.follow(&group_id).await;
            if !self.active {
                return;
            }
        }
        let registered = match GroupStore::new(self.home.clone()).and_then(|store| store.list()) {
            Ok(groups) => groups
                .into_iter()
                .map(|meta| meta.group_id)
                .collect::<HashSet<_>>(),
            Err(error) => {
                tracing::warn!(%error, "could not list the Groups to follow");
                return;
            }
        };
        self.groups
            .retain(|group_id, _| registered.contains(group_id));
        self.waiting
            .retain(|group_id, _| registered.contains(group_id));
        // Before the first listing every Group already existed.
        let new = if self.listed {
            Resume::Start
        } else {
            Resume::End
        };
        self.listed = true;
        for group_id in registered {
            if self.groups.contains_key(&group_id) {
                continue;
            }
            let resume = self
                .waiting
                .get(&group_id)
                .cloned()
                .unwrap_or_else(|| new.clone());
            self.begin(&group_id, resume).await;
            if !self.active {
                return;
            }
        }
    }

    async fn begin(&mut self, group_id: &str, resume: Resume) {
        let Some((key, subscriptions)) = self.bindings() else {
            return;
        };
        let mut owed = Vec::new();
        self.start(group_id, resume, &subscriptions, &mut owed);
        self.send(&key, &subscriptions, owed).await;
    }

    /// Begin following a Group, and collect what each binding is owed from
    /// before that point: the events after where it starts. Only the given
    /// bindings are owed anything from before it; one saved after they were
    /// read misses what was written between its starting point and there.
    fn start(
        &mut self,
        group_id: &str,
        resume: Resume,
        subscriptions: &[Subscription],
        owed: &mut Vec<Owed>,
    ) {
        let mut initial_end = None;
        match self.read_start(group_id, &resume, subscriptions, &mut initial_end) {
            Ok(Some((followed, found))) => {
                self.groups.insert(group_id.to_owned(), followed);
                self.waiting.remove(group_id);
                for subscription in subscriptions {
                    self.passed
                        .insert((subscription.binding.clone(), group_id.to_owned()));
                }
                owed.extend(found);
            }
            // Nothing written yet: everything it gets is new.
            Ok(None) => {
                self.waiting.insert(group_id.to_owned(), Resume::Beginning);
            }
            Err(error) => {
                let resume = match (resume, initial_end) {
                    (Resume::End, Some(end)) => Resume::EndAfter(end),
                    (resume, _) => resume,
                };
                if self.waiting.insert(group_id.to_owned(), resume).is_none() {
                    tracing::warn!(group_id, %error, "could not follow a Group's ledger yet");
                }
            }
        }
    }

    fn read_start(
        &self,
        group_id: &str,
        resume: &Resume,
        subscriptions: &[Subscription],
        initial_end: &mut Option<String>,
    ) -> io::Result<Option<(Followed, Vec<Owed>)>> {
        let path = GroupStore::new(self.home.clone())?.ledger_path(group_id)?;
        let (follower, end) = LedgerFollower::at_end(&path)?;
        let Some(end) = end else {
            return Ok(None);
        };
        if matches!(resume, Resume::End) {
            *initial_end = Some(end.clone());
        }
        let mut window = ledger::tail(&path, SEEN)?;
        // What was added after the follower took its place comes from it.
        let Some(last) = window.iter().rposition(|event| event.id == end) else {
            return Err(io::Error::other("the ledger changed while it was read"));
        };
        window.truncate(last + 1);
        let window = window
            .into_iter()
            .map(|mut event| {
                event.group_id = group_id.to_owned();
                Arc::new(event)
            })
            .collect::<Vec<_>>();
        let len = window.len();
        let at = |id: &str| window.iter().position(|event| event.id == id);
        let closed = closed_in(window.iter().map(Arc::as_ref));
        let from = |from: usize| -> Vec<Arc<Event>> {
            window[from.min(len)..]
                .iter()
                .filter(|event| notable(event, &closed))
                .cloned()
                .collect()
        };
        // Where the notifier handled this ledger up to, after losing it.
        let handled = match resume {
            Resume::After(last) => at(last).map(|at| at + 1),
            _ => Some(0),
        };
        // What every binding that knew this Group is owed.
        let owed_here = match resume {
            Resume::End => Vec::new(),
            Resume::EndAfter(last) => match at(last) {
                Some(at) => from(at + 1),
                None => self.read_forward(&path, group_id, last, &end)?,
            },
            Resume::After(last) => match handled {
                Some(handled) => from(handled),
                None => self.read_forward(&path, group_id, last, &end)?,
            },
            // Seen for the first time: after an import among the newest
            // events, or all of a history this short. Anything else was
            // written before it was seen.
            Resume::Start => match window.iter().rposition(|event| is_import(event)) {
                Some(at) => from(at + 1),
                None if len <= NEW_GROUP_EVENTS => from(0),
                None => Vec::new(),
            },
            Resume::Beginning => self.read_beginning(&path, group_id, &end)?,
        };
        let mut owed = Vec::new();
        for subscription in subscriptions {
            let binding = &subscription.binding;
            let known = self.stored_before(binding)
                || self
                    .passed
                    .contains(&(binding.clone(), group_id.to_owned()));
            let events = match subscription.since.get(group_id).map(String::as_str) {
                _ if known => owed_here.clone(),
                // Not visible to it when it was bound.
                None => owed_here.clone(),
                // Nothing was written when it was bound: all of it is newer.
                Some("") => match resume {
                    Resume::Beginning => owed_here.clone(),
                    _ => self.read_beginning(&path, group_id, &end)?,
                },
                Some(since) => match at(since) {
                    Some(at) => from((at + 1).max(handled.unwrap_or(0))),
                    None => self.read_forward(&path, group_id, since, &end)?,
                },
            };
            owed.extend(events.into_iter().map(|event| (binding.clone(), event)));
        }
        let mut seen = Seen::default();
        for event in &window {
            seen.record(&event.id);
        }
        Ok(Some((
            Followed {
                path,
                follower,
                seen,
            },
            owed,
        )))
    }

    /// The messages owed by a Group the notifier saw empty: all it was
    /// written since, after the import that wrote its history, if any.
    fn read_beginning(
        &self,
        path: &Path,
        group_id: &str,
        end: &str,
    ) -> io::Result<Vec<Arc<Event>>> {
        let mut events = Vec::new();
        let mut closed = HashSet::new();
        let mut reached = false;
        ledger::visit_snapshot_oldest_first(path, |event| {
            if is_import(&event) {
                events.clear();
            }
            if let Some(request) = user_requests::closes(&event) {
                closed.insert(request.to_owned());
            }
            reached = event.id == end;
            if user_requests::targets_user(&event) {
                events.push(event);
            }
            if reached {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        })?;
        if !reached {
            return Err(io::Error::other("the ledger changed while it was read"));
        }
        Ok(events
            .into_iter()
            .filter(|event| notable(event, &closed))
            .map(|mut event| {
                event.group_id = group_id.to_owned();
                Arc::new(event)
            })
            .collect())
    }

    /// The messages owed after `after` when it is further back than the
    /// newest events: read forward from there up to `end`. A point no longer
    /// in the ledger owes nothing, and is reported.
    fn read_forward(
        &self,
        path: &Path,
        group_id: &str,
        after: &str,
        end: &str,
    ) -> io::Result<Vec<Arc<Event>>> {
        let mut events = Vec::new();
        let mut cursor = after.to_owned();
        let reached = 'read: loop {
            let page = ledger::events_after(path, &cursor, CATCH_UP_PAGE)?;
            let Some(last) = page.last() else {
                break false;
            };
            cursor.clone_from(&last.id);
            for event in page {
                let at_end = event.id == end;
                events.push(event);
                if at_end {
                    break 'read true;
                }
            }
        };
        if !reached {
            tracing::warn!(
                group_id,
                "a notification starting point is no longer in the ledger; what came after it is not announced"
            );
            return Ok(Vec::new());
        }
        let closed = closed_in(events.iter());
        Ok(events
            .into_iter()
            .filter(|event| notable(event, &closed))
            .map(|mut event| {
                event.group_id = group_id.to_owned();
                Arc::new(event)
            })
            .collect())
    }

    /// Read what a followed Group added and send it to the bindings owed it.
    async fn follow(&mut self, group_id: &str) {
        let Some(followed) = self.groups.get_mut(group_id) else {
            return;
        };
        let batch = match followed.follower.poll(&followed.path) {
            Ok(events) => followed.seen.after_known(events),
            Err(error) => {
                let resume = followed
                    .seen
                    .newest()
                    .cloned()
                    .map_or(Resume::Start, Resume::After);
                self.groups.remove(group_id);
                tracing::warn!(group_id, %error, "could not read a Group's ledger; trying again later");
                self.waiting.insert(group_id.to_owned(), resume);
                return;
            }
        };
        if batch.is_empty() {
            return;
        }
        let batch = batch
            .into_iter()
            .map(|mut event| {
                event.group_id = group_id.to_owned();
                event
            })
            .collect::<Vec<_>>();
        let closed = closed_in(batch.iter());
        if !batch.iter().any(|event| notable(event, &closed)) {
            // Nothing to announce. A binding whose starting point is here is
            // placed by the first batch that has something for it.
            if let Some(followed) = self.groups.get_mut(group_id) {
                for event in &batch {
                    followed.seen.record(&event.id);
                }
            }
            return;
        }
        // The bindings are read after the events, so one saved before these
        // were read is among them.
        let Some((key, subscriptions)) = self.bindings() else {
            // Unreadable for now: start again from before this batch.
            if self.active
                && let Some(followed) = self.groups.remove(group_id)
            {
                let resume = followed
                    .seen
                    .newest()
                    .cloned()
                    .map_or(Resume::Start, Resume::After);
                self.waiting.insert(group_id.to_owned(), resume);
            }
            return;
        };
        let owed = self.owed(group_id, &batch, &subscriptions);
        if let Some(followed) = self.groups.get_mut(group_id) {
            for event in &batch {
                followed.seen.record(&event.id);
            }
        }
        self.send(&key, &subscriptions, owed).await;
    }

    /// What each binding is owed of a new batch: all of it once past its
    /// starting point, the part after that point when it is in the batch,
    /// and none while the point is still ahead.
    fn owed(
        &mut self,
        group_id: &str,
        batch: &[Event],
        subscriptions: &[Subscription],
    ) -> Vec<Owed> {
        let closed = closed_in(batch.iter());
        let notable = batch
            .iter()
            .enumerate()
            .filter(|(_, event)| notable(event, &closed))
            .map(|(at, event)| (at, Arc::new(event.clone())))
            .collect::<Vec<_>>();
        let mut owed = Vec::new();
        for subscription in subscriptions {
            let Some(from) = self.reached(group_id, batch, subscription) else {
                continue;
            };
            owed.extend(
                notable
                    .iter()
                    .filter(|(at, _)| *at >= from)
                    .map(|(_, event)| (subscription.binding.clone(), Arc::clone(event))),
            );
        }
        owed
    }

    /// From which event of `batch` a binding is owed, or `None` while its
    /// starting point lies ahead.
    fn reached(
        &mut self,
        group_id: &str,
        batch: &[Event],
        subscription: &Subscription,
    ) -> Option<usize> {
        let key = (subscription.binding.clone(), group_id.to_owned());
        if self.stored_before(&subscription.binding) || self.passed.contains(&key) {
            return Some(0);
        }
        let from = match subscription.since.get(group_id).map(String::as_str) {
            None | Some("") => 0,
            Some(since) => {
                if let Some(at) = batch.iter().position(|event| event.id == since) {
                    at + 1
                } else if self
                    .groups
                    .get(group_id)
                    .is_some_and(|followed| followed.seen.contains(since))
                    || !self.ahead(group_id, since, &batch[0].id)
                {
                    0
                } else {
                    return None;
                }
            }
        };
        self.passed.insert(key);
        Some(from)
    }

    /// Whether `since` comes after `first` in a Group's ledger. A point no
    /// longer there, or a ledger that cannot be read, counts as passed: the
    /// worst that does is announce an event from just before a binding.
    fn ahead(&self, group_id: &str, since: &str, first: &str) -> bool {
        let Some(followed) = self.groups.get(group_id) else {
            return false;
        };
        ledger::inspect(&followed.path, |_, positions| {
            match (positions.get(since), positions.get(first)) {
                (Some(since), Some(first)) => since > first,
                (Some(_), None) => true,
                (None, _) => false,
            }
        })
        .unwrap_or(false)
    }

    /// Send each binding what it is owed, the bindings side by side. The
    /// next batch waits for the slowest, at most one request timeout for a
    /// push service that does not answer: it is then left out until the
    /// next try.
    async fn send(&mut self, key: &VapidKey, subscriptions: &[Subscription], owed: Vec<Owed>) {
        if owed.is_empty() {
            return;
        }
        let mut names = HashMap::new();
        let mut jobs: HashMap<String, Vec<Arc<Event>>> = HashMap::new();
        for (binding, event) in owed {
            if !names.contains_key(&event.group_id) {
                names.insert(event.group_id.clone(), self.group_names(&event.group_id));
            }
            jobs.entry(binding).or_default().push(event);
        }
        let reports = join_all(subscriptions.iter().filter_map(|subscription| {
            let events = jobs.remove(&subscription.binding)?;
            Some(self.send_to(key, subscription, events, &names))
        }))
        .await;
        self.apply(reports);
    }

    async fn send_to(
        &self,
        key: &VapidKey,
        subscription: &Subscription,
        events: Vec<Arc<Event>>,
        names: &HashMap<String, GroupNames>,
    ) -> Report {
        let mut report = Report {
            binding: subscription.binding.clone(),
            ..Report::default()
        };
        let mut failed = self.broken.contains(&subscription.binding);
        for event in events {
            let request = user_requests::is_request(&event);
            let request_key = request_key(&event);
            if failed {
                if request {
                    report.keep.push(request_key);
                }
                continue;
            }
            // Checked for every send: an earlier one can wait on the network
            // while a token is revoked or the endpoint is bound again.
            match self.permit(subscription, &event.group_id) {
                Permit::Send => {}
                Permit::Skip => {
                    if request {
                        report.done.push(request_key);
                    }
                    continue;
                }
                Permit::Gone => {
                    report.gone = true;
                    break;
                }
                Permit::Unknown => {
                    if request {
                        report.keep.push(request_key);
                    }
                    continue;
                }
            }
            let (title, body) = match names.get(&event.group_id) {
                Some(names) => summary(&event, names),
                None => summary(&event, &GroupNames::unknown(&event.group_id)),
            };
            let Some(payload) = payload(&event, request, &title, &body, &subscription.origin)
            else {
                // Only a damaged store holds such an origin, and no later try
                // repairs it; binding again in that browser does.
                tracing::warn!(
                    "a Web Push subscription has no valid page origin; enable notifications again in that browser"
                );
                if request {
                    report.done.push(request_key);
                }
                continue;
            };
            match self.sender.send(key, subscription, &payload).await {
                Outcome::Accepted | Outcome::Refused => {
                    report.answered = true;
                    if request {
                        report.done.push(request_key);
                    }
                }
                Outcome::Gone => {
                    report.gone = true;
                    break;
                }
                // The rest wait for the next try rather than for this service.
                Outcome::Retry => {
                    failed = true;
                    report.failed = true;
                    if request {
                        report.keep.push(request_key);
                    }
                }
            }
        }
        report
    }

    fn apply(&mut self, reports: Vec<Report>) {
        let mut gone = HashSet::new();
        let now = Instant::now();
        for report in reports {
            if report.gone {
                gone.insert(report.binding);
                continue;
            }
            if report.failed {
                let failures = self.failures.entry(report.binding.clone()).or_default();
                *failures = failures.saturating_add(1);
                let wait = backoff(*failures);
                self.broken.insert(report.binding.clone());
                self.due.insert(report.binding.clone(), now + wait);
            } else if report.answered {
                self.failures.remove(&report.binding);
            }
            let pending = self.pending.entry(report.binding.clone()).or_default();
            pending.retain(|request| !report.done.contains(request));
            for request in report.keep {
                if !pending.contains(&request) {
                    pending.push(request);
                }
            }
            if pending.len() > MAX_PENDING {
                let dropped = pending.len() - MAX_PENDING;
                pending.drain(..dropped);
                tracing::warn!(
                    dropped,
                    "Web Push requests waiting for another try were dropped"
                );
            }
            if pending.is_empty() {
                self.pending.remove(&report.binding);
            } else if !self.due.contains_key(&report.binding) {
                // Kept because its state could not be read: a later try.
                self.due.insert(report.binding.clone(), now + FIRST_RETRY);
            }
        }
        if !gone.is_empty() {
            if let Err(error) = self.store.forget(&gone) {
                tracing::warn!(%error, "could not remove expired Web Push subscriptions");
            }
            for binding in &gone {
                self.pending.remove(binding);
                self.broken.remove(binding);
                self.failures.remove(binding);
            }
            self.passed.retain(|(binding, _)| !gone.contains(binding));
        }
        self.reschedule();
    }

    /// Forget tries no longer needed, and wake for the earliest left.
    fn reschedule(&mut self) {
        let (broken, pending) = (&self.broken, &self.pending);
        self.due
            .retain(|binding, _| broken.contains(binding) || pending.contains_key(binding));
        self.retry = self.due.values().min().copied();
    }

    /// Try again the bindings whose time has come: one whose push service
    /// failed is sent to again, and its waiting requests still open are
    /// offered again.
    async fn retry_pending(&mut self) {
        let now = Instant::now();
        let ready = self
            .due
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(binding, _)| binding.clone())
            .collect::<HashSet<_>>();
        for binding in &ready {
            self.due.remove(binding);
            self.broken.remove(binding);
        }
        let Some((key, subscriptions)) = self.bindings() else {
            // Bindings that could not be read are tried again later.
            if self.active {
                self.delay(&ready, now + FIRST_RETRY);
            }
            return;
        };
        let wanted = self
            .pending
            .iter()
            .filter(|(binding, _)| ready.contains(*binding))
            .flat_map(|(_, requests)| requests.iter().cloned())
            .collect::<HashSet<_>>();
        if wanted.is_empty() {
            self.reschedule();
            return;
        }
        let groups = wanted
            .iter()
            .filter_map(|request| request_group(request))
            .collect::<HashSet<_>>()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let Some(mut scan) = Scan::start(&self.client, Some(&groups)).await else {
            self.delay(&ready, now + FIRST_RETRY);
            return;
        };
        let mut open = HashMap::new();
        while let Some(page) = scan.next(&self.client).await {
            for event in page {
                let request = request_key(&event);
                if wanted.contains(&request) {
                    open.insert(request, Arc::new(event));
                }
            }
        }
        // One no longer open in a Group read to the end is settled. Only
        // the requests looked for were checked: other bindings keep theirs.
        for (_, requests) in self
            .pending
            .iter_mut()
            .filter(|(binding, _)| ready.contains(*binding))
        {
            requests.retain(|request| {
                open.contains_key(request)
                    || !request_group(request).is_some_and(|group| scan.complete.contains(group))
            });
        }
        self.pending.retain(|_, requests| !requests.is_empty());
        let owed = self
            .pending
            .iter()
            .filter(|(binding, _)| ready.contains(*binding))
            .flat_map(|(binding, requests)| {
                requests
                    .iter()
                    .filter_map(|request| open.get(request))
                    .map(|event| (binding.clone(), Arc::clone(event)))
            })
            .collect::<Vec<_>>();
        self.send(&key, &subscriptions, owed).await;
        // A request still waiting whose Group could not be read is tried
        // again later.
        for binding in &ready {
            if self.pending.contains_key(binding) && !self.due.contains_key(binding) {
                self.due
                    .insert(binding.clone(), Instant::now() + FIRST_RETRY);
            }
        }
        self.reschedule();
    }

    /// Put off the tries of `bindings` that still have requests waiting.
    fn delay(&mut self, bindings: &HashSet<String>, until: Instant) {
        for binding in bindings {
            if self.pending.contains_key(binding) {
                self.due.entry(binding.clone()).or_insert(until);
            }
        }
        self.reschedule();
    }

    fn permit(&self, subscription: &Subscription, group_id: &str) -> Permit {
        match self.store.current(&subscription.binding) {
            Ok(Some(_)) => {}
            Ok(None) => return Permit::Skip,
            Err(_) => return Permit::Unknown,
        }
        let Owner::Token {
            user_id,
            token_id,
            created_at,
        } = &subscription.owner
        else {
            return Permit::Send;
        };
        let tokens = match AccessTokenStore::new(self.home.clone()).and_then(|store| store.list()) {
            Ok(tokens) => tokens,
            Err(_) => return Permit::Unknown,
        };
        let Some(token) = tokens.iter().find(|token| {
            token.token_id() == *token_id
                && token.created_at == *created_at
                && token.user_id == *user_id
        }) else {
            return Permit::Gone;
        };
        if token.is_admin || token.allowed_groups.iter().any(|group| group == group_id) {
            Permit::Send
        } else {
            Permit::Skip
        }
    }

    fn group_names(&self, group_id: &str) -> GroupNames {
        let group = GroupStore::new(self.home.clone()).and_then(|store| store.load(group_id));
        match group {
            Ok(group) => GroupNames {
                title: if group.title.trim().is_empty() {
                    group_id.to_owned()
                } else {
                    group.title
                },
                actors: group
                    .actors
                    .into_iter()
                    .filter(|actor| !actor.title.trim().is_empty())
                    .map(|actor| (actor.id, actor.title))
                    .collect(),
            },
            Err(_) => GroupNames::unknown(group_id),
        }
    }
}

struct GroupNames {
    title: String,
    actors: HashMap<String, String>,
}

impl GroupNames {
    fn unknown(group_id: &str) -> Self {
        Self {
            title: group_id.to_owned(),
            actors: HashMap::new(),
        }
    }
}

fn is_import(event: &Event) -> bool {
    event.kind == "group.create" && event.data.get("imported") == Some(&Value::Bool(true))
}

/// The requests that `events` close.
fn closed_in<'a>(events: impl Iterator<Item = &'a Event>) -> HashSet<String> {
    events
        .filter_map(user_requests::closes)
        .map(str::to_owned)
        .collect()
}

/// A message to the user, unless it is a request closed already.
fn notable(event: &Event, closed: &HashSet<String>) -> bool {
    user_requests::targets_user(event)
        && !(user_requests::is_request(event) && closed.contains(&event.id))
}

fn request_key(event: &Event) -> String {
    format!("{}:{}", event.group_id, event.id)
}

/// The Group a request key names.
fn request_group(request: &str) -> Option<&str> {
    request.split_once(':').map(|(group_id, _)| group_id)
}

/// How long a push service that failed `failures` tries in a row waits.
fn backoff(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    FIRST_RETRY.saturating_mul(1 << doublings).min(LAST_RETRY)
}

/// The notification's title and one-line body: Group and sender, then the
/// question a request asks, or the message text.
fn summary(event: &Event, names: &GroupNames) -> (String, String) {
    let sender = names
        .actors
        .get(&event.by)
        .map_or(event.by.as_str(), String::as_str);
    let title = shorten(&format!("{} · {sender}", names.title), TITLE_CHARS);
    let text = match reply_question::stored(&event.data) {
        reply_question::Stored::Valid(question) => question.prompt,
        _ => event
            .data
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    };
    (title, shorten(&text, BODY_CHARS))
}

fn shorten(text: &str, limit: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= limit {
        return line;
    }
    let mut short = line.chars().take(limit - 1).collect::<String>();
    short.push('…');
    short
}

/// One declarative push message (W3C Push API) for every browser. A browser
/// that supports them shows it itself and opens `navigate` when it is
/// clicked; any other hands it to the service worker, which reads the same
/// fields. `None` unless the origin is one a binding could have stored: a
/// browser drops a declarative message without a link, and opens the link
/// it is given.
fn payload(event: &Event, request: bool, title: &str, body: &str, origin: &str) -> Option<Vec<u8>> {
    if !is_page_origin(origin) {
        return None;
    }
    let mut navigate = url::Url::parse(origin).ok()?.join("/ui/").ok()?;
    navigate
        .query_pairs_mut()
        .append_pair("group", &event.group_id)
        .append_pair("event", &event.id);
    let envelope = json!({
        "web_push": 8030,
        "notification": {
            "title": title,
            "body": body,
            "navigate": navigate.as_str(),
            "tag": request_key(event),
            "data": {
                "v": 1,
                "kind": if request { "request" } else { "message" },
                "group_id": event.group_id,
                "event_id": event.id,
            },
        },
    });
    Some(envelope.to_string().into_bytes())
}

#[cfg(test)]
mod tests;
