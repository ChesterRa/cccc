use super::super::crypto::{MAX_PLAINTEXT, ReceiverKeys, decrypt};
use super::super::store::NewSubscription;
use super::*;
use aws_lc_rs::agreement::{ECDH_P256, PrivateKey};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::sync::atomic::{AtomicU16, Ordering as AtomicOrdering};

fn message(text: &str, data: Value) -> Event {
    let mut event = Event::new("chat.message", "g_cccc");
    event.by = "claude-1".into();
    event.data = data.as_object().cloned().unwrap_or_default();
    event.data.insert("text".into(), json!(text));
    event
}

fn names() -> GroupNames {
    GroupNames {
        title: "cccc TUI 设计优化".into(),
        actors: HashMap::from([("claude-1".to_owned(), "Claude".to_owned())]),
    }
}

#[test]
fn a_page_of_requests_that_does_not_say_whether_more_follow_is_unread() {
    let page = |more: Value| {
        let mut page = Map::new();
        page.insert("events".into(), json!([]));
        if !more.is_null() {
            page.insert("has_more".into(), more);
        }
        page
    };
    assert_eq!(
        request_page(&page(json!(false))).map(|(_, more, _)| more),
        Some(false)
    );
    assert!(request_page(&page(Value::Null)).is_none());
    assert!(request_page(&page(json!("false"))).is_none());
}

#[test]
fn an_empty_or_unchanged_paging_cursor_does_not_advance() {
    assert!(!cursor_advances(None, ""));
    assert!(!cursor_advances(None, "   "));
    assert!(!cursor_advances(Some("cursor"), "cursor"));
    assert!(!cursor_advances(Some("cursor"), " cursor "));
    assert!(cursor_advances(None, "cursor"));
    assert!(cursor_advances(Some("cursor"), "older"));
}

#[test]
fn a_notification_names_the_group_and_sender_on_one_short_line() {
    let event = message("First line\n\n   second\tline", json!({"to": ["user"]}));
    let (title, body) = summary(&event, &names());
    assert_eq!(title, "cccc TUI 设计优化 · Claude");
    assert_eq!(body, "First line second line");

    let long = "长".repeat(BODY_CHARS + 10);
    let (_, body) = summary(&message(&long, json!({})), &names());
    assert_eq!(body.chars().count(), BODY_CHARS);
    assert!(body.ends_with('…'));

    let unknown = GroupNames {
        title: "g_cccc".into(),
        actors: HashMap::new(),
    };
    let (title, _) = summary(&event, &unknown);
    assert_eq!(title, "g_cccc · claude-1");
}

#[test]
fn a_question_is_announced_by_its_prompt() {
    let question = reply_question::Question {
        v: 1,
        context: String::new(),
        prompt: "Which option should ship?".into(),
        options: Vec::new(),
        recommended: None,
    };
    let event = message(
        &question.text(),
        json!({"to": ["user"], "message_mode": "request_reply", "question": question}),
    );
    let (_, body) = summary(&event, &names());
    assert_eq!(body, "Which option should ship?");
}

#[test]
fn the_payload_links_back_to_the_page_that_subscribed() {
    let event = message("hello", json!({"to": ["user"]}));
    let bytes =
        payload(&event, true, "title", "body", "https://cccc.example:8443").expect("payload");
    let value: Value = serde_json::from_slice(&bytes).expect("json");
    let notification = &value["notification"];
    assert_eq!(
        notification["navigate"],
        json!(format!(
            "https://cccc.example:8443/ui/?group=g_cccc&event={}",
            event.id
        ))
    );
    assert_eq!(notification["tag"], json!(format!("g_cccc:{}", event.id)));
    assert_eq!(
        notification["data"],
        json!({"v": 1, "kind": "request", "group_id": "g_cccc", "event_id": event.id})
    );
    assert_eq!(value["web_push"], json!(8030));
    assert!(value.get("mutable").is_none());
    // Only an origin a binding could have stored gives a link.
    for damaged in [
        "",
        "not an origin",
        "file:///tmp",
        "https://user@cccc.example",
        "https://cccc.example/ui/",
        "HTTPS://CCCC.example",
    ] {
        assert!(
            payload(&event, true, "title", "body", damaged).is_none(),
            "{damaged}"
        );
    }

    // The longest title and body still fit one encrypted record.
    let title = "長".repeat(TITLE_CHARS);
    let body = "長".repeat(BODY_CHARS);
    let bytes = payload(&event, false, &title, &body, "https://cccc.example").expect("payload");
    assert!(bytes.len() < MAX_PLAINTEXT / 2, "{}", bytes.len());
}

fn notifier(home: &HomeLayout) -> Notifier {
    Notifier::new(home.clone(), Sender::new().expect("sender"))
}

fn bind(store: &PushStore, endpoint: &str, owner: Owner) -> Subscription {
    let key = store.ensure_key().expect("key");
    let browser = PrivateKey::generate(&ECDH_P256).expect("browser");
    store
        .bind(
            NewSubscription {
                endpoint: endpoint.into(),
                p256dh: URL_SAFE_NO_PAD
                    .encode(browser.compute_public_key().expect("public").as_ref()),
                auth: URL_SAFE_NO_PAD.encode([1_u8; 16]),
                application_server_key: key,
                origin: "https://cccc.example".into(),
            },
            owner,
            BTreeMap::new(),
        )
        .expect("bind");
    store
        .snapshot()
        .expect("snapshot")
        .expect("key")
        .1
        .into_iter()
        .find(|item| item.endpoint == endpoint)
        .expect("bound")
}

#[test]
fn each_send_is_checked_against_the_current_token_and_binding() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let notifier = notifier(&home);
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    let member = tokens
        .create("ana", vec!["g_allowed".into()], false, None)
        .expect("token");
    let owner = Owner::Token {
        user_id: member.user_id.clone(),
        token_id: member.token_id(),
        created_at: member.created_at.clone(),
    };
    let local = bind(
        &notifier.store,
        "https://fcm.googleapis.com/fcm/send/local",
        Owner::Local,
    );
    let ana = bind(
        &notifier.store,
        "https://fcm.googleapis.com/fcm/send/ana",
        owner.clone(),
    );

    assert!(matches!(notifier.permit(&local, "g_any"), Permit::Send));
    assert!(matches!(notifier.permit(&ana, "g_allowed"), Permit::Send));
    assert!(matches!(notifier.permit(&ana, "g_other"), Permit::Skip));

    // Rebound meanwhile: the old binding is not sent to.
    let rebound = bind(
        &notifier.store,
        "https://fcm.googleapis.com/fcm/send/ana",
        owner,
    );
    assert!(matches!(notifier.permit(&ana, "g_allowed"), Permit::Skip));
    assert!(matches!(
        notifier.permit(&rebound, "g_allowed"),
        Permit::Send
    ));

    // A revoked token takes its bindings with it.
    tokens.delete(&member.token_id()).expect("revoke");
    assert!(matches!(
        notifier.permit(&rebound, "g_allowed"),
        Permit::Gone
    ));
    // The same secret added again is a different owner.
    tokens
        .create("ana", vec!["g_allowed".into()], false, Some(&member.token))
        .expect("recreate");
    let recreated = tokens
        .lookup(&member.token)
        .expect("lookup")
        .expect("token");
    assert_ne!(recreated.created_at, member.created_at);
    assert!(matches!(
        notifier.permit(&rebound, "g_allowed"),
        Permit::Gone
    ));
}

#[test]
fn messages_and_requests_follow_the_inbox_rules() {
    let to_user = message("hi", json!({"to": ["user"]}));
    assert!(user_requests::targets_user(&to_user));
    assert!(!user_requests::is_request(&to_user));
    let request = message(
        "?",
        json!({"to": ["@user"], "message_mode": "request_reply"}),
    );
    assert!(user_requests::is_request(&request));
    let broadcast = message("all", json!({"to": ["@all"]}));
    assert!(!user_requests::targets_user(&broadcast));
    let mut from_user = message("mine", json!({"to": ["user"]}));
    from_user.by = "user".into();
    assert!(!user_requests::targets_user(&from_user));
    let copy = message("copy", json!({"to": ["user"], "dst_group_id": "g_remote"}));
    assert!(!user_requests::targets_user(&copy));
}

/// A push service on loopback for one device. It keeps every body it is
/// sent and answers `status`. While that is 0 it holds each request until
/// `release`, then answers 503; unreleased, the sender's timeout ends it.
#[derive(Clone)]
struct Device {
    url: url::Url,
    bodies: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    status: Arc<AtomicU16>,
    /// Told when a request arrives.
    arrived: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl Device {
    async fn start(status: u16) -> Self {
        let bodies: Arc<std::sync::Mutex<Vec<Vec<u8>>>> = Arc::default();
        let status = Arc::new(AtomicU16::new(status));
        let arrived = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let (record, answer) = (Arc::clone(&bodies), Arc::clone(&status));
        let (tell, held) = (Arc::clone(&arrived), Arc::clone(&release));
        let app = axum::Router::new().route(
            "/push",
            axum::routing::post(move |body: axum::body::Bytes| {
                let (record, answer) = (Arc::clone(&record), Arc::clone(&answer));
                let (tell, held) = (Arc::clone(&tell), Arc::clone(&held));
                async move {
                    record.lock().expect("bodies").push(body.to_vec());
                    tell.notify_one();
                    let status = answer.load(AtomicOrdering::SeqCst);
                    if status == 0 {
                        held.notified().await;
                        return axum::http::StatusCode::SERVICE_UNAVAILABLE;
                    }
                    axum::http::StatusCode::from_u16(status).expect("status")
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let url = url::Url::parse(&format!(
            "http://{}/push",
            listener.local_addr().expect("address")
        ))
        .expect("url");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("device");
        });
        Self {
            url,
            bodies,
            status,
            arrived,
            release,
        }
    }

    fn tries(&self) -> usize {
        self.bodies.lock().expect("bodies").len()
    }
}

/// A browser subscribed to the notifier, with what it needs to read what it
/// was sent.
struct Browser {
    endpoint: String,
    key: PrivateKey,
    receiver: ReceiverKeys,
    device: Device,
}

impl Browser {
    /// The events it was sent, in order.
    fn received(&self) -> Vec<String> {
        self.device
            .bodies
            .lock()
            .expect("bodies")
            .iter()
            .map(|body| {
                let plain = decrypt(body, &self.key, &self.receiver).expect("decrypt");
                let value: Value = serde_json::from_slice(&plain).expect("json");
                value["notification"]["data"]["event_id"]
                    .as_str()
                    .expect("event")
                    .to_owned()
            })
            .collect()
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    groups: GroupStore,
    notifier: Notifier,
}

impl Fixture {
    /// A notifier started before any browser subscribed.
    fn new() -> Self {
        Self::with_sender(Sender::new().expect("sender"))
    }

    fn with_sender(sender: Sender) -> Self {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let notifier = Notifier::new(home.clone(), sender);
        Self {
            _temp: temp,
            home,
            groups,
            notifier,
        }
    }

    fn group(&self) -> String {
        self.groups.create("Group", "").expect("group").group_id
    }

    /// Append `event` to a Group's ledger, as the daemon does.
    fn write(&self, group_id: &str, mut event: Event) -> Event {
        event.group_id = group_id.to_owned();
        ledger::append(&self.groups.ledger_path(group_id).expect("path"), &event).expect("append");
        event
    }

    /// Subscribe a browser now, starting where the ledgers stand, as PUT does.
    async fn subscribe(&mut self, name: &str, status: u16) -> Browser {
        let device = Device::start(status).await;
        let endpoint = format!("https://fcm.googleapis.com/fcm/send/{name}");
        self.notifier
            .sender
            .stand_ins
            .insert(endpoint.clone(), device.url.clone());
        let store = &self.notifier.store;
        let server_key = store.ensure_key().expect("key");
        let key = PrivateKey::generate(&ECDH_P256).expect("browser");
        let p256dh = URL_SAFE_NO_PAD.encode(key.compute_public_key().expect("public").as_ref());
        let auth = URL_SAFE_NO_PAD.encode([5_u8; 16]);
        store
            .bind(
                NewSubscription {
                    endpoint: endpoint.clone(),
                    p256dh: p256dh.clone(),
                    auth: auth.clone(),
                    application_server_key: server_key,
                    origin: "https://cccc.example".into(),
                },
                Owner::Local,
                starting_points(&self.home, None).expect("starting points"),
            )
            .expect("bind");
        let receiver = ReceiverKeys::parse(&p256dh, &auth).expect("keys");
        Browser {
            endpoint,
            key,
            receiver,
            device,
        }
    }

    fn binding(&self, browser: &Browser) -> String {
        self.subscription(browser).binding
    }

    fn subscription(&self, browser: &Browser) -> Subscription {
        self.notifier
            .store
            .snapshot()
            .expect("read")
            .expect("key")
            .1
            .into_iter()
            .find(|item| item.endpoint == browser.endpoint)
            .expect("bound")
    }

    /// Bring a browser's next try forward to now.
    fn due_now(&mut self, browser: &Browser) {
        let binding = self.binding(browser);
        self.notifier.due.insert(binding, Instant::now());
    }

    /// Run an isolated daemon for this home, which reports open requests.
    async fn daemon(&self) -> tokio::task::JoinHandle<()> {
        let home = self.home.clone();
        let daemon = tokio::spawn(async move {
            cccc_daemon::run(home).await.expect("isolated daemon");
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            while self
                .notifier
                .client
                .call(&DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Map::new(),
                })
                .await
                .is_err()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("daemon ready");
        daemon
    }
}

fn to_user(text: &str) -> Event {
    message(text, json!({"to": ["user"]}))
}

fn request(text: &str) -> Event {
    message(
        text,
        json!({"to": ["user"], "message_mode": "request_reply"}),
    )
}

fn answer(to: &Event) -> Event {
    let mut answer = message("done", json!({"to": ["claude-1"], "reply_to": to.id}));
    answer.by = "user".into();
    answer
}

#[tokio::test]
async fn failed_initial_scan_keeps_old_and_new_binding_start_points() {
    let mut f = Fixture::new();
    let group = f.group();
    f.write(&group, to_user("old one"));
    f.write(&group, to_user("old two"));
    let old = f.subscribe("old-initial-scan", 201).await;
    let new = f.subscribe("new-initial-scan", 201).await;
    let old_binding = f.binding(&old);
    let new_binding = f.binding(&new);
    f.notifier.earlier = Some(HashSet::from([old_binding.clone()]));
    let old_sub = f.subscription(&old);
    let new_sub = f.subscription(&new);
    let before_failure = f.write(&group, to_user("new binding is owed this"));

    // at_end needs only the two newest active events and succeeds; the
    // longer startup read reaches this damaged old segment and fails.
    let segments = f
        .groups
        .group_dir(&group)
        .expect("group")
        .join("state/ledger/segments");
    std::fs::create_dir_all(&segments).expect("segments");
    let damaged = segments.join("ledger.0001.jsonl.gz");
    std::fs::write(&damaged, b"not a gzip stream").expect("damaged segment");
    let subscriptions = [old_sub, new_sub];
    let mut owed = Vec::new();
    f.notifier
        .start(&group, Resume::End, &subscriptions, &mut owed);
    assert!(owed.is_empty());
    assert!(matches!(
        f.notifier.waiting.get(&group),
        Some(Resume::EndAfter(end)) if *end == before_failure.id
    ));

    let after_failure = f.write(&group, to_user("both bindings are owed this"));
    std::fs::remove_file(damaged).expect("repair segment");
    let resume = f.notifier.waiting.get(&group).expect("waiting").clone();
    f.notifier.start(&group, resume, &subscriptions, &mut owed);
    let received = |binding: &str| {
        owed.iter()
            .filter(|(id, _)| id == binding)
            .map(|(_, event)| event.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(received(&old_binding), [after_failure.id.clone()]);
    assert_eq!(
        received(&new_binding),
        [before_failure.id, after_failure.id]
    );
}

fn ids(events: &[Event]) -> Vec<String> {
    events.iter().map(|event| event.id.clone()).collect()
}

#[tokio::test]
async fn a_binding_is_sent_what_follows_it_from_the_moment_it_is_saved() {
    let mut f = Fixture::new();
    let g = f.group();
    f.write(&g, to_user("before"));
    let a = f.subscribe("a", 201).await;
    // Written after the binding was saved and before the notifier woke.
    let early = f.write(&g, to_user("early"));
    f.notifier.activate().await;
    assert_eq!(a.received(), [early.id.clone()]);

    let later = f.write(&g, request("later?"));
    f.notifier.changed(&later).await;
    assert_eq!(a.received(), ids(&[early.clone(), later.clone()]));

    // A browser saved while events wait to be read gets none of them.
    let backlog = f.write(&g, to_user("backlog"));
    let b = f.subscribe("b", 201).await;
    f.notifier.changed(&backlog).await;
    assert_eq!(a.received().last(), Some(&backlog.id));
    assert!(b.received().is_empty());
    let both = f.write(&g, to_user("both"));
    f.notifier.changed(&both).await;
    assert_eq!(a.received().last(), Some(&both.id));
    assert_eq!(b.received(), [both.id.clone()]);
}

#[tokio::test]
async fn a_binding_far_behind_the_newest_events_is_read_forward_from_its_start() {
    let mut f = Fixture::new();
    let g = f.group();
    f.write(&g, to_user("before"));
    let a = f.subscribe("a", 201).await;
    let owed = f.write(&g, to_user("owed"));
    for n in 0..SEEN + 10 {
        f.write(
            &g,
            message(&format!("busy {n}"), json!({"to": ["codex-1"]})),
        );
    }
    f.notifier.activate().await;
    assert_eq!(a.received(), [owed.id.clone()]);
}

#[tokio::test]
async fn after_a_restart_only_what_comes_next_is_announced() {
    let mut f = Fixture::new();
    let g = f.group();
    let a = f.subscribe("a", 201).await;
    f.write(&g, to_user("while stopped"));
    let mut sender = Sender::new().expect("sender");
    sender.stand_ins = f.notifier.sender.stand_ins.clone();
    f.notifier = Notifier::new(f.home.clone(), sender);
    f.notifier.activate().await;
    assert!(a.received().is_empty());
    let next = f.write(&g, to_user("next"));
    f.notifier.changed(&next).await;
    assert_eq!(a.received(), [next.id.clone()]);
}

#[tokio::test]
async fn bindings_unreadable_at_a_restart_still_begin_with_what_comes_next() {
    let mut f = Fixture::new();
    let g = f.group();
    let a = f.subscribe("a", 201).await;
    f.write(&g, to_user("while stopped"));
    let store = f.home.root().join("web_push.json");
    let saved = std::fs::read(&store).expect("read");
    std::fs::write(&store, b"{").expect("damage");
    let mut sender = Sender::new().expect("sender");
    sender.stand_ins = f.notifier.sender.stand_ins.clone();
    f.notifier = Notifier::new(f.home.clone(), sender);
    assert!(f.notifier.subscribed().is_err());
    std::fs::write(&store, saved).expect("restore");
    assert!(f.notifier.subscribed().expect("read"));
    f.notifier.activate().await;
    assert!(a.received().is_empty());
    let next = f.write(&g, to_user("next"));
    f.notifier.changed(&next).await;
    assert_eq!(a.received(), [next.id.clone()]);
}

/// A copy of a Group with `events` of history, imported into `f`'s home.
fn import_history(f: &Fixture, events: usize) -> String {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home).expect("groups");
    let source = store.create("Copied", "").expect("group").group_id;
    let path = store.ledger_path(&source).expect("path");
    let write = |mut event: Event| {
        event.group_id.clone_from(&source);
        ledger::append(&path, &event).expect("append");
        event
    };
    let asked = write(request("old question"));
    write(answer(&asked));
    write(request("still open"));
    for n in 3..events {
        write(to_user(&format!("old {n}")));
    }
    let (bytes, _, _) = cccc_core::group_copy::export(&store, &source).expect("export");
    cccc_core::group_copy::import(&f.groups, &bytes, "", "")
        .expect("import")
        .group_id
}

#[tokio::test]
async fn the_history_of_an_imported_group_is_never_announced() {
    let mut f = Fixture::new();
    f.group();
    let a = f.subscribe("a", 201).await;
    f.notifier.activate().await;
    for size in [6, SEEN * 2] {
        let imported = import_history(&f, size);
        // Something said right after the import, before it was seen.
        let fresh = f.write(&imported, to_user("fresh"));
        f.notifier.changed(&fresh).await;
        f.notifier.sweep().await;
        assert_eq!(a.received().last(), Some(&fresh.id), "{size}");
    }
    assert_eq!(a.received().len(), 2);
}

#[tokio::test]
async fn a_new_group_is_announced_from_its_start_and_a_long_one_from_its_end() {
    let mut f = Fixture::new();
    f.group();
    let a = f.subscribe("a", 201).await;
    f.notifier.activate().await;

    let created = f.group();
    let mut create = Event::new("group.create", &created);
    create.by = "user".into();
    let create = f.write(&created, create);
    let first = f.write(&created, to_user("first"));
    f.notifier.changed(&create).await;
    assert_eq!(a.received(), [first.id.clone()]);

    // One that turns up with a long history was not just created.
    let restored = f.group();
    for n in 0..NEW_GROUP_EVENTS + 1 {
        f.write(&restored, to_user(&format!("restored {n}")));
    }
    f.notifier.sweep().await;
    assert_eq!(a.received().len(), 1);
    let next = f.write(&restored, to_user("next"));
    f.notifier.changed(&next).await;
    assert_eq!(a.received(), ids(&[first, next]));
}

#[tokio::test]
async fn a_request_closed_before_it_is_read_is_not_announced() {
    let mut f = Fixture::new();
    let g = f.group();
    let a = f.subscribe("a", 201).await;
    f.notifier.activate().await;
    let answered = f.write(&g, request("quick?"));
    f.write(&g, answer(&answered));
    let withdrawn = f.write(&g, request("never mind?"));
    let mut cancel = Event::new("chat.reply_request.cancelled", &g);
    cancel.by = "claude-1".into();
    cancel
        .data
        .insert("source_event_id".into(), json!(withdrawn.id));
    f.write(&g, cancel);
    let note = f.write(&g, to_user("note"));
    f.notifier.changed(&note).await;
    assert_eq!(a.received(), [note.id.clone()]);
}

#[tokio::test]
async fn a_push_service_that_does_not_answer_holds_the_others_back_one_try_at_most() {
    let mut f = Fixture::new();
    let g = f.group();
    let healthy = f.subscribe("healthy", 201).await;
    let silent = f.subscribe("silent", 0).await;
    f.notifier.activate().await;
    let asked = (0..3)
        .map(|n| f.write(&g, request(&format!("q{n}"))))
        .collect::<Vec<_>>();
    // The healthy browser is sent its batch while the silent one still
    // holds the first request: the bindings are sent to side by side.
    let (arrived, release) = (
        Arc::clone(&silent.device.arrived),
        Arc::clone(&silent.device.release),
    );
    let watch = async {
        arrived.notified().await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while healthy.received().len() < 3 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the healthy browser is not held back");
        release.notify_one();
    };
    tokio::join!(f.notifier.changed(&asked[2]), watch);
    assert_eq!(healthy.received(), ids(&asked));
    // After that one try it is left out: the next batch is not held back by
    // it, and its requests wait for its next try without the network.
    let next = f.write(&g, request("while waiting"));
    f.notifier.changed(&next).await;
    let mut sent = asked.clone();
    sent.push(next);
    assert_eq!(healthy.received(), ids(&sent));
    assert_eq!(silent.device.tries(), 1);
    let binding = f.binding(&silent);
    assert_eq!(f.notifier.pending[&binding].len(), 4);
    assert_eq!(f.notifier.retry, f.notifier.due.get(&binding).copied());
}

#[tokio::test]
async fn one_failing_push_service_does_not_hold_back_another() {
    let mut f = Fixture::new();
    let g = f.group();
    let failing = f.subscribe("failing", 503).await;
    let healthy = f.subscribe("healthy", 201).await;
    f.notifier.activate().await;
    let (x, y) = (f.binding(&failing), f.binding(&healthy));
    // The failing one fails three tries in a row and waits longer each time.
    for n in 0..3 {
        let note = f.write(&g, to_user(&format!("note {n}")));
        f.notifier.changed(&note).await;
        assert_eq!(f.notifier.failures[&x], n + 1);
        if n < 2 {
            f.due_now(&failing);
            f.notifier.retry_pending().await;
            assert!(!f.notifier.broken.contains(&x));
        }
    }
    assert_eq!(failing.device.tries(), 3);
    assert_eq!(healthy.device.tries(), 3);
    assert!(!f.notifier.failures.contains_key(&y));
    // Another push service failing once waits only the first delay.
    healthy.device.status.store(503, AtomicOrdering::SeqCst);
    let note = f.write(&g, to_user("both fail"));
    f.notifier.changed(&note).await;
    let now = Instant::now();
    assert_eq!(failing.device.tries(), 3, "still left out");
    assert!(f.notifier.due[&y] <= now + FIRST_RETRY);
    assert!(f.notifier.due[&x] > now + FIRST_RETRY * 3);
    assert_eq!(f.notifier.retry, Some(f.notifier.due[&y]));
    // Its next try reaches the service and starts it over; the other is
    // still left out until its own time.
    healthy.device.status.store(201, AtomicOrdering::SeqCst);
    f.due_now(&healthy);
    f.notifier.retry_pending().await;
    let back = f.write(&g, to_user("back"));
    f.notifier.changed(&back).await;
    assert_eq!(healthy.received().last(), Some(&back.id));
    assert!(!f.notifier.failures.contains_key(&y));
    assert_eq!(failing.device.tries(), 3);
    assert_eq!(f.notifier.retry, Some(f.notifier.due[&x]));
}

#[tokio::test]
async fn a_group_missing_while_a_browser_subscribes_neither_blocks_it_nor_replays_history() {
    let mut f = Fixture::new();
    let g = f.group();
    let other = f.group();
    let a = f.subscribe("a", 201).await;
    f.notifier.activate().await;
    let old = f.write(&g, to_user("old"));
    f.notifier.changed(&old).await;
    // Its directory is away, as while the Group is being deleted.
    let dir = f.groups.group_dir(&g).expect("dir");
    let away = dir.with_file_name(format!(".{g}.away"));
    std::fs::rename(&dir, &away).expect("move away");
    f.notifier.sweep().await;
    let b = f.subscribe("b", 201).await;
    let since = f.subscription(&b).since;
    assert!(since.contains_key(&other));
    assert!(!since.contains_key(&g));
    // Back again: only what is written now is announced, to both.
    std::fs::rename(&away, &dir).expect("restore");
    let new = f.write(&g, to_user("new"));
    f.notifier.changed(&new).await;
    assert_eq!(b.received(), [new.id.clone()]);
    assert_eq!(a.received(), ids(&[old, new]));
}

#[tokio::test]
async fn a_request_waiting_in_a_deleted_group_is_let_go() {
    let mut f = Fixture::new();
    let daemon = f.daemon().await;
    let g = f.group();
    let h = f.group();
    let a = f.subscribe("a", 503).await;
    f.notifier.activate().await;
    let binding = f.binding(&a);
    // Deleted unseen: its next try finds the Group gone and lets it go.
    let asked = f.write(&g, request("deleted?"));
    f.notifier.changed(&asked).await;
    assert_eq!(f.notifier.pending[&binding].len(), 1);
    f.groups.delete(&g).expect("delete");
    f.due_now(&a);
    f.notifier.retry_pending().await;
    assert!(f.notifier.pending.is_empty());
    assert_eq!(a.device.tries(), 1);
    // Seen deleted: it is let go at once.
    let asked = f.write(&h, request("also deleted?"));
    f.notifier.changed(&asked).await;
    assert_eq!(f.notifier.pending[&binding].len(), 1);
    let mut deleted = Event::new("group.deleted", &h);
    deleted.by = "user".into();
    f.notifier.changed(&deleted).await;
    assert!(f.notifier.pending.is_empty());
    daemon.abort();
}

#[tokio::test]
async fn nothing_is_kept_for_a_browser_no_longer_bound() {
    let mut f = Fixture::new();
    let g = f.group();
    let a = f.subscribe("a", 201).await;
    let b = f.subscribe("b", 503).await;
    f.notifier.activate().await;
    let asked = f.write(&g, request("q?"));
    f.notifier.changed(&asked).await;
    let gone = f.binding(&b);
    assert!(f.notifier.pending.contains_key(&gone));
    assert!(
        f.notifier
            .passed
            .iter()
            .any(|(binding, _)| *binding == gone)
    );
    f.notifier
        .store
        .unbind(&b.endpoint, &Owner::Local)
        .expect("unbind");
    let next = f.write(&g, to_user("next"));
    f.notifier.changed(&next).await;
    assert!(!f.notifier.pending.contains_key(&gone));
    assert!(
        !f.notifier
            .passed
            .iter()
            .any(|(binding, _)| *binding == gone)
    );
    assert!(!f.notifier.due.contains_key(&gone));
    assert!(!f.notifier.failures.contains_key(&gone));
    assert!(f.notifier.retry.is_none());
    assert_eq!(a.received(), ids(&[asked, next]));
}

#[tokio::test]
async fn a_binding_whose_origin_gives_no_link_is_sent_nothing() {
    let mut f = Fixture::new();
    let g = f.group();
    let a = f.subscribe("a", 201).await;
    let b = f.subscribe("b", 201).await;
    let store = f.home.root().join("web_push.json");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&store).expect("read")).expect("json");
    for item in document["subscriptions"]
        .as_array_mut()
        .expect("subscriptions")
    {
        if item["endpoint"] == json!(a.endpoint) {
            item["origin"] = json!("not an origin");
        }
    }
    std::fs::write(&store, serde_json::to_vec(&document).expect("json")).expect("damage");
    f.notifier.activate().await;
    let asked = f.write(&g, request("which?"));
    f.notifier.changed(&asked).await;
    assert_eq!(a.device.tries(), 0);
    assert_eq!(b.received(), [asked.id.clone()]);
    // No later try repairs the origin, so none is kept.
    assert!(f.notifier.pending.is_empty());
}

#[tokio::test]
async fn a_batch_read_while_the_bindings_cannot_be_is_sent_once_they_can() {
    let mut f = Fixture::new();
    let g = f.group();
    let a = f.subscribe("a", 201).await;
    f.notifier.activate().await;
    let store = f.home.root().join("web_push.json");
    let saved = std::fs::read(&store).expect("read");
    let asked = f.write(&g, request("still there?"));
    std::fs::write(&store, b"{").expect("damage");
    f.notifier.changed(&asked).await;
    assert!(a.received().is_empty());
    // Busy meanwhile: more than the newest events looked at on resuming.
    let later = f.write(&g, request("and this?"));
    for n in 0..SEEN + 10 {
        f.write(
            &g,
            message(&format!("busy {n}"), json!({"to": ["codex-1"]})),
        );
    }
    std::fs::write(&store, saved).expect("restore");
    f.notifier.sweep().await;
    assert_eq!(a.received(), ids(&[asked, later]));
}

#[tokio::test]
async fn a_request_is_offered_again_while_it_is_open() {
    let mut f = Fixture::new();
    let daemon = f.daemon().await;
    let g = f.group();
    let a = f.subscribe("a", 503).await;
    let binding = f.binding(&a);
    f.notifier.activate().await;

    let asked = f.write(&g, request("again?"));
    f.notifier.changed(&asked).await;
    assert_eq!(a.device.tries(), 1);
    // Not before its time.
    f.notifier.retry_pending().await;
    assert_eq!(a.device.tries(), 1);
    // A try that cannot read the bindings is made again later.
    let store = f.home.root().join("web_push.json");
    let saved = std::fs::read(&store).expect("read");
    std::fs::write(&store, b"{").expect("damage");
    f.notifier.due.insert(binding.clone(), Instant::now());
    f.notifier.retry_pending().await;
    assert!(f.notifier.retry.is_some_and(|at| at > Instant::now()));
    assert_eq!(a.device.tries(), 1);
    std::fs::write(&store, saved).expect("restore");
    a.device.status.store(201, AtomicOrdering::SeqCst);
    f.due_now(&a);
    f.notifier.retry_pending().await;
    assert_eq!(a.received(), [asked.id.clone(), asked.id.clone()]);
    assert!(f.notifier.pending.is_empty());
    assert!(f.notifier.retry.is_none());
    assert!(!f.notifier.failures.contains_key(&binding));

    // One answered before its next try is not sent again.
    a.device.status.store(503, AtomicOrdering::SeqCst);
    let closed = f.write(&g, request("closed?"));
    f.notifier.changed(&closed).await;
    f.write(&g, answer(&closed));
    a.device.status.store(201, AtomicOrdering::SeqCst);
    let tries = a.device.tries();
    f.due_now(&a);
    f.notifier.retry_pending().await;
    assert_eq!(a.device.tries(), tries);
    assert!(f.notifier.pending.is_empty());
    daemon.abort();
}

#[tokio::test]
async fn a_try_for_one_browser_keeps_what_another_waits_with_in_the_same_group() {
    let mut f = Fixture::new();
    let daemon = f.daemon().await;
    let g = f.group();
    let x = f.subscribe("x", 201).await;
    let y = f.subscribe("y", 503).await;
    let yb = f.binding(&y);
    f.notifier.activate().await;
    let for_y = f.write(&g, request("for y"));
    f.notifier.changed(&for_y).await;
    x.device.status.store(503, AtomicOrdering::SeqCst);
    let for_x = f.write(&g, request("for x"));
    f.notifier.changed(&for_x).await;
    assert_eq!(f.notifier.pending[&yb].len(), 2);

    // Only x is due: reading the Group to the end checks x's requests alone.
    x.device.status.store(201, AtomicOrdering::SeqCst);
    f.due_now(&x);
    f.notifier.retry_pending().await;
    assert_eq!(
        x.received(),
        ids(&[for_y.clone(), for_x.clone(), for_x.clone()])
    );
    assert!(f.notifier.pending[&yb].contains(&request_key(&for_y)));

    y.device.status.store(201, AtomicOrdering::SeqCst);
    f.due_now(&y);
    f.notifier.retry_pending().await;
    let mut retried = y.received()[1..].to_vec();
    retried.sort();
    let mut open = ids(&[for_y, for_x]);
    open.sort();
    assert_eq!(retried, open);
    assert!(f.notifier.pending.is_empty());
    daemon.abort();
}

#[tokio::test]
async fn the_sender_state_says_whether_this_process_sends() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let runtime = Runtime::default();
    let (stop, _) = broadcast::channel(1);

    let held = take_claim(&home, &runtime, &mut stop.subscribe())
        .await
        .expect("claim");
    assert_eq!(runtime.state(), SenderState::Here);
    // Another process for the same home waits for it.
    let mut stopped = stop.subscribe();
    stop.send(()).expect("stop");
    assert!(take_claim(&home, &runtime, &mut stopped).await.is_none());
    assert_eq!(runtime.state(), SenderState::Elsewhere);
    drop(held);

    // A claim that cannot be made at all.
    let blocked = HomeLayout::from_path(temp.path().join("blocked")).expect("home");
    blocked.initialize().expect("initialize");
    std::fs::create_dir(blocked.root().join("web_push.sender.lock")).expect("block");
    let mut stopped = stop.subscribe();
    stop.send(()).expect("stop");
    assert!(take_claim(&blocked, &runtime, &mut stopped).await.is_none());
    assert_eq!(runtime.state(), SenderState::Unavailable);
}
