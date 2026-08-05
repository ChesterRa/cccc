use cccc_contracts::Event;
use cccc_core::ledger::LedgerFollower;
use cccc_core::{GroupStore, HomeLayout};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

const EVENT_CHANNEL_CAPACITY: usize = 1024;
const WATCH_QUEUE_CAPACITY: usize = 1024;
const WATCH_COALESCE_DELAY: Duration = Duration::from_millis(20);
const ACTIVE_FEED_RESCAN_INTERVAL: Duration = Duration::from_secs(1);
const DELETED_GROUP_PRUNE_INTERVAL: Duration = Duration::from_secs(60);
const GLOBAL_DISCOVERY_REPLAY_LIMIT: usize = 256;

#[derive(Clone)]
pub(crate) struct LedgerEventHub {
    inner: Arc<HubInner>,
}

struct HubInner {
    home: HomeLayout,
    groups_root: PathBuf,
    feeds: Mutex<HashMap<String, Arc<GroupFeed>>>,
    global_sender: broadcast::Sender<Event>,
    global_followers: Mutex<HashMap<String, LedgerFollower>>,
    shutdown: watch::Sender<bool>,
    monitor_started: AtomicBool,
}

struct GroupFeed {
    path: PathBuf,
    follower: Mutex<LedgerFollower>,
    sender: broadcast::Sender<Event>,
    last_event_id: Mutex<Option<String>>,
}

impl LedgerEventHub {
    pub(crate) fn new(home: HomeLayout) -> Self {
        let (global_sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let groups_dir = home.groups_dir();
        let groups_root = groups_dir.canonicalize().unwrap_or(groups_dir);
        let (shutdown, _) = watch::channel(false);
        let inner = Arc::new(HubInner {
            groups_root,
            home,
            feeds: Mutex::new(HashMap::new()),
            global_sender,
            global_followers: Mutex::new(HashMap::new()),
            shutdown,
            monitor_started: AtomicBool::new(false),
        });
        Self { inner }
    }

    pub(crate) fn subscribe_group(&self, group_id: &str) -> io::Result<broadcast::Receiver<Event>> {
        self.subscribe_group_with_cursor(group_id)
            .map(|(receiver, _)| receiver)
    }

    pub(crate) fn subscribe_group_with_cursor(
        &self,
        group_id: &str,
    ) -> io::Result<(broadcast::Receiver<Event>, Option<String>)> {
        prune_deleted_groups(&self.inner);
        let (receiver, cursor) = {
            let mut feeds = self
                .inner
                .feeds
                .lock()
                .map_err(|_| io::Error::other("ledger event feeds lock poisoned"))?;
            feeds.retain(|_, feed| feed.sender.receiver_count() > 0);
            if let Some(feed) = feeds.get(group_id) {
                let cursor = feed
                    .last_event_id
                    .lock()
                    .map_err(|_| io::Error::other("ledger event cursor lock poisoned"))?;
                (feed.sender.subscribe(), cursor.clone())
            } else {
                let path = GroupStore::new(self.inner.home.clone())?.ledger_path(group_id)?;
                let (follower, cursor) = LedgerFollower::at_end(&path)?;
                let (sender, receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
                feeds.insert(
                    group_id.to_owned(),
                    Arc::new(GroupFeed {
                        path,
                        follower: Mutex::new(follower),
                        sender,
                        last_event_id: Mutex::new(cursor.clone()),
                    }),
                );
                (receiver, cursor)
            }
        };
        ensure_watcher(&self.inner);
        publish_group_changes(&self.inner, group_id);
        Ok((receiver, cursor))
    }

    pub(crate) fn subscribe_global(&self) -> broadcast::Receiver<Event> {
        if self.inner.global_sender.receiver_count() == 0 {
            initialize_global_followers(&self.inner);
        }
        let receiver = self.inner.global_sender.subscribe();
        ensure_watcher(&self.inner);
        publish_all_changes(&self.inner);
        receiver
    }

    pub(crate) fn replay_after(
        &self,
        group_id: &str,
        event_id: &str,
        limit: usize,
    ) -> io::Result<Vec<Event>> {
        let path = GroupStore::new(self.inner.home.clone())?.ledger_path(group_id)?;
        cccc_core::ledger::events_after(&path, event_id, limit)
    }
}

impl Drop for HubInner {
    fn drop(&mut self) {
        self.shutdown.send(true).ok();
    }
}

fn ensure_watcher(inner: &Arc<HubInner>) {
    if inner
        .monitor_started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if !start_watcher(
        Arc::downgrade(inner),
        inner.groups_root.clone(),
        inner.shutdown.subscribe(),
    ) {
        inner.monitor_started.store(false, Ordering::Release);
    }
}

fn start_watcher(
    inner: Weak<HubInner>,
    groups_root: PathBuf,
    shutdown: watch::Receiver<bool>,
) -> bool {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::error!("cannot start ledger event monitor without a Tokio runtime");
        return false;
    };
    let (sender, receiver) = mpsc::channel(WATCH_QUEUE_CAPACITY);
    let rescan_required = Arc::new(AtomicBool::new(false));
    let callback_rescan = Arc::clone(&rescan_required);
    let sender_guard = sender.clone();
    let (watcher_tx, watcher_rx) = oneshot::channel();
    let watcher_thread = std::thread::Builder::new()
        .name("cccc-ledger-watch".into())
        .spawn(move || {
            let result = create_watcher(groups_root, sender, callback_rescan);
            watcher_tx.send(result).ok();
        });
    if let Err(error) = watcher_thread {
        tracing::error!(%error, "failed to start ledger filesystem watcher thread");
        return false;
    }
    runtime.spawn(run_monitor(
        inner,
        receiver,
        sender_guard,
        watcher_rx,
        shutdown,
        rescan_required,
    ));
    true
}

fn create_watcher(
    groups_root: PathBuf,
    sender: mpsc::Sender<PathBuf>,
    rescan_required: Arc<AtomicBool>,
) -> Result<RecommendedWatcher, String> {
    let callback_root = groups_root.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Ok(event) = result {
            for path in event.paths {
                if is_ledger_path(&path) || (!path.exists() && path.starts_with(&callback_root)) {
                    enqueue_watch_path(&sender, &rescan_required, path);
                }
            }
        }
    })
    .map_err(|error| error.to_string())?;
    watcher
        .watch(&groups_root, RecursiveMode::Recursive)
        .map_err(|error| error.to_string())?;
    Ok(watcher)
}

async fn run_monitor(
    inner: Weak<HubInner>,
    mut receiver: mpsc::Receiver<PathBuf>,
    _sender_guard: mpsc::Sender<PathBuf>,
    watcher_rx: oneshot::Receiver<Result<RecommendedWatcher, String>>,
    mut shutdown: watch::Receiver<bool>,
    rescan_required: Arc<AtomicBool>,
) {
    let mut watcher_rx = watcher_rx;
    let mut watcher_setup_complete = false;
    let mut watcher = None;
    let mut rescan_interval = tokio::time::interval(ACTIVE_FEED_RESCAN_INTERVAL);
    rescan_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut prune_interval = tokio::time::interval(DELETED_GROUP_PRUNE_INTERVAL);
    prune_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let path = tokio::select! {
            setup = &mut watcher_rx, if !watcher_setup_complete => {
                watcher_setup_complete = true;
                match setup {
                    Ok(Ok(value)) => watcher = Some(value),
                    Ok(Err(error)) => {
                        tracing::error!(%error, "failed to create ledger filesystem watcher; polling remains active");
                    }
                    Err(_) => {
                        tracing::error!("ledger filesystem watcher setup stopped; polling remains active");
                    }
                }
                continue;
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            path = receiver.recv() => {
                let Some(path) = path else { break; };
                path
            }
            _ = rescan_interval.tick() => {
                let Some(inner) = inner.upgrade() else { break; };
                publish_active_feed_changes(&inner);
                continue;
            }
            _ = prune_interval.tick() => {
                let Some(inner) = inner.upgrade() else { break; };
                prune_deleted_groups(&inner);
                continue;
            }
        };
        let Some(inner) = inner.upgrade() else {
            break;
        };
        let mut group_ids = HashSet::new();
        collect_group_id(&inner.groups_root, &path, &mut group_ids);
        tokio::time::sleep(WATCH_COALESCE_DELAY).await;
        while let Ok(path) = receiver.try_recv() {
            collect_group_id(&inner.groups_root, &path, &mut group_ids);
        }
        for group_id in group_ids {
            publish_group_changes(&inner, &group_id);
        }
        prune_deleted_groups(&inner);
        if rescan_required.swap(false, Ordering::AcqRel) {
            publish_all_changes(&inner);
        }
    }
    if let Some(watcher) = watcher {
        let _ = std::thread::spawn(move || drop(watcher));
    }
}

fn enqueue_watch_path(sender: &mpsc::Sender<PathBuf>, rescan_required: &AtomicBool, path: PathBuf) {
    match sender.try_send(path) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            rescan_required.store(true, Ordering::Release);
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {}
    }
}

fn collect_group_id(groups_root: &Path, path: &Path, group_ids: &mut HashSet<String>) {
    if !is_ledger_path(path) {
        return;
    }
    let Ok(relative) = path.strip_prefix(groups_root) else {
        return;
    };
    let Some(group_id) = relative.components().next() else {
        return;
    };
    let group_id = group_id.as_os_str().to_string_lossy().trim().to_owned();
    if !group_id.is_empty() {
        group_ids.insert(group_id);
    }
}

fn is_ledger_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if name == "ledger.jsonl" {
        return true;
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized.contains("/state/ledger/segments/")
        && (name.ends_with(".jsonl") || name.ends_with(".jsonl.gz"))
}

fn publish_group_changes(inner: &HubInner, group_id: &str) {
    let feed = inner.feeds.lock().ok().and_then(|mut feeds| {
        feeds.retain(|_, feed| feed.sender.receiver_count() > 0);
        feeds.get(group_id).cloned()
    });

    if let Some(feed) = &feed {
        let events = feed
            .follower
            .lock()
            .ok()
            .and_then(|mut follower| follower.poll(&feed.path).ok())
            .unwrap_or_default();
        for event in events {
            if let Ok(mut cursor) = feed.last_event_id.lock() {
                cursor.replace(event.id.clone());
                feed.sender.send(event).ok();
            }
        }
    }
    for event in poll_global_events(inner, group_id) {
        inner.global_sender.send(event).ok();
    }
}

fn publish_all_changes(inner: &HubInner) {
    let Ok(store) = GroupStore::new(inner.home.clone()) else {
        return;
    };
    let Ok(groups) = store.list() else {
        return;
    };
    for group in groups {
        publish_group_changes(inner, &group.group_id);
    }
}

fn publish_active_feed_changes(inner: &HubInner) {
    let mut group_ids = inner
        .feeds
        .lock()
        .map(|feeds| feeds.keys().cloned().collect::<HashSet<_>>())
        .unwrap_or_default();
    if let Ok(followers) = inner.global_followers.lock() {
        group_ids.extend(followers.keys().cloned());
    }
    for group_id in group_ids {
        publish_group_changes(inner, &group_id);
    }
}

fn prune_deleted_groups(inner: &HubInner) {
    let Ok(existing) = GroupStore::new(inner.home.clone())
        .and_then(|store| store.list())
        .map(|groups| {
            groups
                .into_iter()
                .map(|group| group.group_id)
                .collect::<HashSet<_>>()
        })
    else {
        return;
    };
    if let Ok(mut feeds) = inner.feeds.lock() {
        feeds.retain(|group_id, feed| {
            existing.contains(group_id) && feed.sender.receiver_count() > 0
        });
    }
    if let Ok(mut followers) = inner.global_followers.lock() {
        let deleted = followers
            .keys()
            .filter(|group_id| !existing.contains(*group_id))
            .cloned()
            .collect::<Vec<_>>();
        followers.retain(|group_id, _| existing.contains(group_id));
        drop(followers);
        for group_id in deleted {
            let mut event = Event::new("group.deleted", &group_id);
            event.by = "system".into();
            inner.global_sender.send(event).ok();
        }
    }
}

fn initialize_global_followers(inner: &HubInner) {
    let Ok(store) = GroupStore::new(inner.home.clone()) else {
        return;
    };
    let Ok(groups) = store.list() else {
        return;
    };
    let Ok(mut followers) = inner.global_followers.lock() else {
        return;
    };
    for group in groups {
        if followers.contains_key(&group.group_id) {
            continue;
        }
        let Ok(path) = store.ledger_path(&group.group_id) else {
            continue;
        };
        let mut follower = LedgerFollower::default();
        if follower.poll(&path).is_ok() {
            followers.insert(group.group_id, follower);
        }
    }
}

fn poll_global_events(inner: &HubInner, group_id: &str) -> Vec<Event> {
    if inner.global_sender.receiver_count() == 0 {
        return Vec::new();
    }
    let Ok(store) = GroupStore::new(inner.home.clone()) else {
        return Vec::new();
    };
    let Ok(path) = store.ledger_path(group_id) else {
        return Vec::new();
    };
    let Ok(mut followers) = inner.global_followers.lock() else {
        return Vec::new();
    };
    if let Some(follower) = followers.get_mut(group_id) {
        return follower.poll(&path).unwrap_or_default();
    }
    // Preserve the first lifecycle events of a newly-created group, but keep
    // imported groups from flooding every browser with their full history.
    let Ok((follower, _)) = LedgerFollower::at_end(&path) else {
        return Vec::new();
    };
    let events = cccc_core::ledger::tail(&path, GLOBAL_DISCOVERY_REPLAY_LIMIT).unwrap_or_default();
    followers.insert(group_id.to_owned(), follower);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::ledger;

    #[test]
    fn extracts_group_id_from_nested_ledger_path() {
        let root = Path::new("/tmp/cccc/groups");
        let mut groups = HashSet::new();
        collect_group_id(
            root,
            Path::new("/tmp/cccc/groups/g_test/state/ledger/segments/ledger.1.jsonl.gz"),
            &mut groups,
        );
        assert_eq!(groups, HashSet::from(["g_test".to_owned()]));
    }

    #[test]
    fn ignores_non_ledger_runtime_files() {
        let root = Path::new("/tmp/cccc/groups");
        let mut groups = HashSet::new();
        collect_group_id(
            root,
            Path::new("/tmp/cccc/groups/g_test/state/runtime/output.log"),
            &mut groups,
        );
        assert!(groups.is_empty());
    }

    #[test]
    fn creating_hub_does_not_start_the_filesystem_watcher() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let hub = LedgerEventHub::new(home);
        assert!(!hub.inner.monitor_started.load(Ordering::Acquire));
        let inner = Arc::downgrade(&hub.inner);
        drop(hub);
        assert!(inner.upgrade().is_none());
    }

    #[test]
    fn full_watcher_queue_requests_bounded_rescan() {
        let (sender, _receiver) = mpsc::channel(1);
        let rescan = AtomicBool::new(false);
        enqueue_watch_path(&sender, &rescan, PathBuf::from("first"));
        enqueue_watch_path(&sender, &rescan, PathBuf::from("second"));
        assert!(rescan.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn deleted_groups_are_removed_from_feeds_and_global_cursors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("cleanup", "").expect("group");
        let hub = LedgerEventHub::new(home);
        let _receiver = hub.subscribe_group(&group.group_id).expect("subscribe");
        hub.inner
            .global_followers
            .lock()
            .expect("followers")
            .insert(group.group_id.clone(), LedgerFollower::default());
        store.delete(&group.group_id).expect("delete");

        prune_deleted_groups(&hub.inner);
        assert!(hub.inner.feeds.lock().expect("feeds").is_empty());
        assert!(
            hub.inner
                .global_followers
                .lock()
                .expect("followers")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn one_watcher_fans_out_external_appends_to_all_subscribers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("watcher", "").expect("group");
        let hub = LedgerEventHub::new(home);
        assert!(!hub.inner.monitor_started.load(Ordering::Acquire));
        let mut first = hub
            .subscribe_group(&group.group_id)
            .expect("first subscriber");
        assert!(hub.inner.monitor_started.load(Ordering::Acquire));
        let mut second = hub
            .subscribe_group(&group.group_id)
            .expect("second subscriber");
        let mut global = hub.subscribe_global();
        let mut event = Event::new("chat.message", &group.group_id);
        event.data.insert("text".into(), serde_json::json!("hello"));

        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger path"),
            &event,
        )
        .expect("append");

        for receiver in [&mut first, &mut second, &mut global] {
            let received = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
                .await
                .expect("watcher timeout")
                .expect("broadcast event");
            assert_eq!(received.id, event.id);
        }
    }

    #[tokio::test]
    async fn active_feed_rescan_recovers_an_unobserved_append() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("rescan", "").expect("group");
        let hub = LedgerEventHub::new(home);
        let mut receiver = hub
            .subscribe_group(&group.group_id)
            .expect("group subscriber");
        let mut event = Event::new("chat.message", &group.group_id);
        event.data.insert("text".into(), serde_json::json!("hello"));
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger path"),
            &event,
        )
        .expect("append");

        publish_active_feed_changes(&hub.inner);

        let received = receiver.recv().await.expect("rescanned event");
        assert_eq!(received.id, event.id);
    }

    #[tokio::test]
    async fn global_stream_publishes_every_coalesced_group_event() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("global", "").expect("group");
        let hub = LedgerEventHub::new(home);
        let mut global = hub.subscribe_global();
        let first = Event::new("actor.update", &group.group_id);
        let second = Event::new("chat.message", &group.group_id);
        let path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&path, &first).expect("first");
        ledger::append(&path, &second).expect("second");

        let received_first = tokio::time::timeout(Duration::from_secs(2), global.recv())
            .await
            .expect("first timeout")
            .expect("first event");
        let received_second = tokio::time::timeout(Duration::from_secs(2), global.recv())
            .await
            .expect("second timeout")
            .expect("second event");
        assert_eq!(received_first.id, first.id);
        assert_eq!(received_second.id, second.id);
    }

    #[tokio::test]
    async fn global_stream_keeps_first_events_from_a_newly_discovered_group() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let hub = LedgerEventHub::new(home);
        let _global = hub.subscribe_global();
        let group = store.create("late", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        let created = Event::new("group.create", &group.group_id);
        let live = Event::new("actor.start", &group.group_id);
        ledger::append(&path, &created).expect("created");
        ledger::append(&path, &live).expect("live");

        assert_eq!(
            poll_global_events(&hub.inner, &group.group_id),
            vec![created, live]
        );
    }

    #[tokio::test]
    async fn newly_discovered_import_replay_is_bounded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let hub = LedgerEventHub::new(home);
        let _global = hub.subscribe_global();
        let group = store.create("import", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        for _ in 0..(GLOBAL_DISCOVERY_REPLAY_LIMIT + 10) {
            ledger::append(&path, &Event::new("chat.message", &group.group_id)).expect("event");
        }

        assert_eq!(
            poll_global_events(&hub.inner, &group.group_id).len(),
            GLOBAL_DISCOVERY_REPLAY_LIMIT
        );
    }

    #[tokio::test]
    async fn global_stream_reports_deleted_groups_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("delete", "").expect("group");
        let hub = LedgerEventHub::new(home);
        let mut global = hub.subscribe_global();

        store.delete(&group.group_id).expect("delete");
        prune_deleted_groups(&hub.inner);

        let deleted = global.try_recv().expect("deleted event");
        assert_eq!(deleted.kind, "group.deleted");
        assert_eq!(deleted.group_id, group.group_id);
        prune_deleted_groups(&hub.inner);
        assert!(global.try_recv().is_err());
    }
}
