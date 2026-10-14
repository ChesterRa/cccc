use super::*;
use cccc_contracts::{DaemonResponse, Event};
use cccc_core::{integration_state, ledger};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
    connector_id: String,
    subscription_token: String,
}

impl Fixture {
    fn default_enabled() -> Self {
        let temp = tempfile::tempdir().expect("isolated default Dots home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home layout");
        home.initialize().expect("initialize owned home");
        let groups = GroupStore::new(home.clone()).expect("group store");
        let mut group = groups.create("Dots", "").expect("create group");
        group.running = true;
        group.state = GroupState::Active;
        groups.save(&group).expect("save active group");
        let configured =
            store::configure_provider(&home, "chatgpt_web").expect("configure isolated connector");
        let connector_id = configured["connector"]["connector_id"]
            .as_str()
            .expect("connector ID")
            .to_owned();
        let mut f = Self {
            _temp: temp,
            home,
            group,
            connector_id,
            subscription_token: String::new(),
        };
        let added = f.call("actor_add", json!({"runtime":"chatgpt_dots"}));
        assert_eq!(added["actor"]["enabled"], true);
        f.group = groups.load(&f.group.group_id).expect("load added Actor");
        f
    }

    fn new() -> Self {
        let temp = tempfile::tempdir().expect("isolated Dots home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home layout");
        home.initialize().expect("initialize owned home");
        let groups = GroupStore::new(home.clone()).expect("group store");
        let mut group = groups.create("Dots", "").expect("create group");
        let mut actor = Actor::new("dot");
        actor.runtime = ActorRuntime::ChatgptDots;
        actor.enabled = false;
        cccc_core::actors::add(&mut group, actor).expect("add stopped Dots Actor");
        group.running = true;
        group.state = GroupState::Active;
        groups.save(&group).expect("save group");
        let configured =
            store::configure_provider(&home, "chatgpt_web").expect("configure isolated connector");
        let connector_id = configured["connector"]["connector_id"]
            .as_str()
            .expect("connector ID")
            .to_owned();
        let mut f = Self {
            _temp: temp,
            home,
            group,
            connector_id,
            subscription_token: String::new(),
        };
        let setup = f.call("dots_events_connect", json!({}));
        f.subscription_token = setup["subscription_token"]
            .as_str()
            .expect("subscription setup token")
            .to_owned();
        let key = store::session_key(
            &f.private(),
            &json!({"openai/session":"synthetic-dot-session"}),
        )
        .expect("host session hash");
        let paired = f.call(
            "web_model_pairing_accept",
            json!({"connector_id":f.connector_id,"code":setup["pairing_code"],"session_key":key}),
        );
        assert_eq!(paired["state"], "bound");
        f
    }

    fn response(&self, op: &str, mut args: Value) -> DaemonResponse {
        let args = args.as_object_mut().expect("object args");
        args.entry("group_id").or_insert(json!(self.group.group_id));
        args.entry("actor_id").or_insert(json!("dot"));
        args.entry("by").or_insert(json!("user"));
        crate::dispatch::dispatch(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.clone(),
            },
        )
    }

    fn call(&self, op: &str, args: Value) -> Value {
        let response = self.response(op, args);
        assert!(response.ok, "{op}: {:?}", response.error);
        Value::Object(response.result)
    }

    fn private(&self) -> Value {
        store::load(&self.home)
            .expect("read isolated private connector")
            .into_iter()
            .find(|c| c["connector_id"] == self.connector_id)
            .expect("connector exists")
    }

    fn subscription_args(&self, secret: &str) -> Value {
        let c = self.private();
        json!({"connector_id":self.connector_id,"subscription_token":self.subscription_token,
            "expected_revision":c["dots"]["revision"],"expected_epoch":c["dots"]["epoch"],
            "callback_url":"https://example.com/owned-dots-callback","subscription_id":"dots_sub_fixture",
            "signing_secret":secret})
    }

    fn start(&self) {
        self.call(
            "dots_events_subscribe",
            self.subscription_args("synthetic-signing-secret"),
        );
        self.call("actor_start", json!({}));
    }

    fn source(&self, text: &str) -> String {
        let groups = GroupStore::new(self.home.clone()).expect("group store");
        let mut event = Event::new("chat.message", &self.group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["dot"],"text":text,"message_mode":"send"})
            .as_object()
            .expect("message data")
            .clone();
        ledger::append(
            &groups
                .ledger_path(&self.group.group_id)
                .expect("ledger path"),
            &event,
        )
        .expect("append source");
        event.id
    }

    fn delivery(&self) -> Value {
        self.call("dots_events_next_delivery", json!({}))["delivery"].clone()
    }

    fn received(&self, delivery: &Value, status: u16) -> Value {
        self.call("dots_events_delivery_record", json!({"connector_id":self.connector_id,
            "event_id":delivery["event_id"],"epoch":delivery["epoch"],"attempt":delivery["attempt"],"http_status":status}))
    }

    fn wake_token(&self, delivery: &Value) -> String {
        let connector = self.private();
        let route = store::dots_subscription_binding(&connector, &self.subscription_token)
            .expect("fixed Actor grant");
        store::dots_wake_token(
            &connector,
            &route,
            delivery["event_id"].as_str().expect("wake ID"),
        )
        .expect("owned execution credential")
    }

    fn claim_capture(&self) -> Value {
        let key = store::session_key(
            &self.private(),
            &json!({"openai/session":"synthetic-dot-session"}),
        )
        .expect("host session hash");
        store::dots_claim_binding(&self.private(), &key).expect("paired Dot claim")
    }

    fn captured(&self, delivery: &Value, tool: &str) -> Value {
        if tool == "cccc_runtime_wait_next_turn" {
            return self.claim_capture();
        }
        store::dots_binding_for_token(&self.private(), &self.wake_token(delivery), tool)
            .expect("valid execution credential")
    }

    fn claim(&self, captured: &Value) -> Value {
        self.call(
            "dots_work_claim",
            json!({"by":"dot","_cccc_web_binding":captured}),
        )
    }

    fn complete(&self, turn: &Value, captured: &Value) -> Value {
        self.call(
            "runtime_complete_turn",
            json!({"by":"dot","turn_id":turn["turn_id"],"event_ids":turn["event_ids"],
            "status":"done","_cccc_web_binding":captured}),
        )
    }
}

#[test]
fn dots_waits_for_explicit_pairing_and_duplicate_setup_does_not_interrupt_a_turn() {
    let mut f = Fixture::default_enabled();
    let setup = f.call("dots_events_connect", json!({}));
    f.subscription_token = setup["subscription_token"]
        .as_str()
        .expect("subscription credential")
        .to_owned();
    f.start();
    f.source("synthetic queued work before pairing");
    let status = f.call("dots_events_status", json!({}));
    assert_eq!(status["phase"], "waiting_pairing");
    assert_eq!(status["paired"], false);
    assert_eq!(status["subscribed"], true);
    assert!(
        f.delivery().is_null(),
        "subscription alone must not send work to an unpaired conversation"
    );
    let key = store::session_key(
        &f.private(),
        &json!({"openai/session":"synthetic-dot-session"}),
    )
    .expect("host key");
    let args =
        json!({"connector_id":f.connector_id,"code":setup["pairing_code"],"session_key":key});
    let before = super::super::runtime_state::actor_state(&f.home, &f.group.group_id, "dot")
        .expect("runtime state");
    let pair = f.call("web_model_pairing_accept", args.clone());
    assert_eq!(pair["state"], "bound");
    assert_eq!(pair["routing_mode"], "mcp_events");
    assert_eq!(
        super::super::runtime_state::actor_state(&f.home, &f.group.group_id, "dot")
            .expect("after pairing"),
        before
    );
    assert_eq!(f.call("dots_events_status", json!({}))["paired"], true);
    f.delivery();
    let packet = f.claim(&f.claim_capture());
    assert_eq!(packet["status"], "work_available");
    let state_before = f.private();
    assert_eq!(f.call("web_model_pairing_accept", args)["state"], "bound");
    assert!(
        f.private() == state_before,
        "duplicate pairing must not alter active work"
    );
    let wrong = f.response("web_model_pairing_accept", json!({"connector_id":f.connector_id,"code":setup["pairing_code"],"session_key":"other-session"}));
    assert!(
        !wrong.ok,
        "a used setup code cannot pair a different conversation"
    );
    assert!(f.private() == state_before);
}

#[test]
fn dots_connect_accepts_default_enabled_actor_after_observational_status() {
    let f = Fixture::default_enabled();
    let groups = GroupStore::new(f.home.clone()).expect("group store");
    let private_path = f.home.root().join("web_model_connectors.yaml");
    let group_path = groups
        .group_dir(&f.group.group_id)
        .expect("owned group directory")
        .join("group.yaml");
    let ledger_path = groups
        .ledger_path(&f.group.group_id)
        .expect("owned ledger path");
    let private_before = std::fs::read(&private_path).expect("private config bytes");
    let group_before = std::fs::read(&group_path).expect("group bytes");
    let ledger_before = std::fs::read(&ledger_path).expect("ledger bytes");
    for _ in 0..3 {
        let status = f.call("dots_events_status", json!({}));
        assert_eq!(status["enabled"], true);
        assert_eq!(status["connected"], false);
        assert_eq!(status["phase"], "unconfigured");
    }
    assert!(
        std::fs::read(&private_path).expect("private config unchanged") == private_before,
        "status must not change private connection state"
    );
    assert_eq!(
        std::fs::read(&group_path).expect("group unchanged"),
        group_before
    );
    assert_eq!(
        std::fs::read(&ledger_path).expect("ledger unchanged"),
        ledger_before
    );
    let denied = f.response("dots_events_connect", json!({"by":"dot"}));
    assert_eq!(
        denied.error.expect("user authority required").code,
        "permission_denied"
    );
    let connected = f.call("dots_events_connect", json!({}));
    assert_eq!(connected["connected"], true);
    assert!(
        connected["subscription_token"]
            .as_str()
            .is_some_and(|token| !token.is_empty())
    );
    assert_eq!(
        f.call("dots_events_status", json!({}))["phase"],
        "waiting_pairing"
    );
    assert!(
        groups
            .load(&f.group.group_id)
            .expect("same enabled Actor")
            .actors[0]
            .enabled
    );
}

#[test]
#[cfg(unix)]
fn dots_connect_rejects_actual_native_owner_after_saved_runtime_changes() {
    struct OwnedTerminal {
        group_id: String,
        started_at: String,
    }
    impl Drop for OwnedTerminal {
        fn drop(&mut self) {
            let _ = cccc_runtime::stop_if_started_at(&self.group_id, "dot", &self.started_at);
        }
    }

    let f = Fixture::default_enabled();
    f.call("actor_update", json!({"runtime":"custom"}));
    let terminal = cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: f.group.group_id.clone(),
        actor_id: "dot".into(),
        runner: RunnerKind::Pty,
        command: vec!["/bin/sh".into(), "-c".into(), "exec sleep 60".into()],
        cwd: f._temp.path().to_path_buf(),
        env: std::collections::BTreeMap::from([(
            "HOME".into(),
            f._temp.path().to_string_lossy().into_owned(),
        )]),
        cols: 120,
        rows: 40,
    })
    .expect("start owned native fixture");
    let owned = OwnedTerminal {
        group_id: f.group.group_id.clone(),
        started_at: terminal.started_at,
    };
    f.call("actor_update", json!({"runtime":"chatgpt_dots"}));
    let path = f.home.root().join("web_model_connectors.yaml");
    let before = std::fs::read(&path).expect("unbound connector bytes");
    let refused = f.response("dots_events_connect", json!({}));
    assert_eq!(
        refused
            .error
            .expect("actual prior owner requires stop")
            .code,
        "actor_must_be_stopped"
    );
    assert_eq!(
        cccc_runtime::status(&f.group.group_id, "dot")
            .expect("native owner retained")
            .pid,
        terminal.pid
    );
    assert!(
        std::fs::read(&path).expect("binding remains absent") == before,
        "saved Runtime must not replace the native owner's authority"
    );
    drop(owned);
    assert!(!cccc_runtime::status(&f.group.group_id, "dot").is_ok_and(|state| state.running));
    assert_eq!(f.call("dots_events_connect", json!({}))["connected"], true);
}

#[test]
fn dots_connect_reuses_enabled_binding_without_retiring_subscription_or_pending_wake() {
    let f = Fixture::new();
    f.start();
    f.source("pending input before setup instructions are retrieved again");
    let delivery = f.delivery();
    assert!(delivery.is_object());
    let path = f.home.root().join("web_model_connectors.yaml");
    let before = std::fs::read(&path).expect("private subscription and wake bytes");
    for _ in 0..2 {
        let repeated = f.call("dots_events_connect", json!({}));
        assert!(
            repeated["subscription_token"].as_str() == Some(f.subscription_token.as_str()),
            "same binding must return the same setup grant"
        );
        assert!(
            std::fs::read(&path).expect("unchanged private state") == before,
            "retrieving instructions must preserve epoch, subscription and pending wake"
        );
    }
    let another = f.call(
        "actor_add",
        json!({"actor_id":"second-dot","runtime":"chatgpt_dots"}),
    );
    assert_eq!(another["actor"]["enabled"], true);
    let conflict = f.response("dots_events_connect", json!({"actor_id":"second-dot"}));
    assert_eq!(
        conflict.error.expect("single Dot binding conflict").code,
        "dots_binding_conflict"
    );
    assert!(
        std::fs::read(&path).expect("original binding retained") == before,
        "a second Actor must not alter the connected Actor's authority"
    );
}

#[test]
fn dots_connect_requires_unfinished_turn_resolution_and_disconnect_still_requires_stop() {
    let f = Fixture::new();
    f.start();
    f.source("accepted work remains owned while connection controls are used");
    let delivery = f.delivery();
    let turn = f.claim(&f.claim_capture())["turn"].clone();
    let live = f.captured(&delivery, "cccc_runtime_complete_turn");
    let before = f.private();
    let refused = f.response("dots_events_connect", json!({}));
    assert_eq!(
        refused.error.expect("unfinished turn retained").code,
        "delivery_unresolved"
    );
    let running = f.response("dots_events_disconnect", json!({}));
    assert_eq!(
        running.error.expect("disconnect requires stop").code,
        "actor_must_be_stopped"
    );
    assert!(
        f.private() == before,
        "rejected controls must preserve exact execution"
    );
    f.call("actor_stop", json!({}));
    for op in ["dots_events_connect", "dots_events_disconnect"] {
        let refused = f.response(op, json!({}));
        assert_eq!(
            refused
                .error
                .expect("stopped unfinished turn retained")
                .code,
            "delivery_unresolved"
        );
    }
    assert!(!f.response("runtime_complete_turn", json!({"by":"dot","turn_id":turn["turn_id"],"event_ids":turn["event_ids"],"_cccc_web_binding":live})).ok);
    f.call(
        "dots_work_control",
        json!({"action":"resolve","turn_id":turn["turn_id"],"event_ids":turn["event_ids"]}),
    );
    let connected = f.call("dots_events_connect", json!({}));
    assert!(
        connected["subscription_token"].as_str() == Some(f.subscription_token.as_str()),
        "explicit resolution must not replace the binding grant"
    );
    assert_eq!(f.received(&delivery, 200)["recorded"], false);
    assert_eq!(
        f.call("dots_events_disconnect", json!({}))["connected"],
        false
    );
}

#[test]
fn dots_status_is_observational_and_fixed_grant_cannot_run_business() {
    let f = Fixture::new();
    let path = f.home.root().join("web_model_connectors.yaml");
    let before = std::fs::read(&path).expect("private config bytes");
    for _ in 0..3 {
        let status = f.call("dots_events_status", json!({}));
        assert_eq!(status["phase"], "waiting_subscription");
        let public = status.to_string();
        assert!(!public.contains(&f.subscription_token));
        assert!(!public.contains("routing_key"));
        assert!(!public.contains("signing_secret"));
    }
    assert_eq!(
        std::fs::read(path).expect("unchanged private config"),
        before
    );
    let groups = GroupStore::new(f.home.clone()).expect("groups");
    groups
        .mutate(&f.group.group_id, |group| {
            let mut another = Actor::new("second-dot");
            another.runtime = ActorRuntime::ChatgptDots;
            another.enabled = false;
            cccc_core::actors::add(group, another)?;
            Ok(())
        })
        .expect("second stopped Dots Actor");
    let conflict = f.response("dots_events_connect", json!({"actor_id":"second-dot"}));
    assert_eq!(
        conflict
            .error
            .expect("explicit one-Dot binding conflict")
            .code,
        "dots_binding_conflict"
    );
    assert_eq!(
        std::fs::read(f.home.root().join("web_model_connectors.yaml"))
            .expect("original binding retained"),
        before
    );
    assert!(
        store::dots_binding_for_token(&f.private(), &f.subscription_token, "cccc_message_send")
            .is_none()
    );
    f.start();
    f.source("owned input");
    assert!(!f.response("runtime_wait_next_turn", json!({"by":"dot"})).ok);
    assert!(
        !f.response(
            "web_model_pairing_begin",
            json!({"connector_id":f.connector_id})
        )
        .ok
    );
    assert!(
        !f.response(
            "headless_set_status",
            json!({"by":"dot","status":"working"})
        )
        .ok
    );
}

#[test]
fn dots_claim_handshake_returns_execution_credential_only_to_the_first_owner() {
    let f = Fixture::new();
    f.start();
    let source = f.source("content is fetched only by the winning claim");
    let delivery = f.delivery();
    let data = &delivery["event"]["data"];
    assert!(
        data.get("actor_token").is_none(),
        "wake must carry no credential"
    );
    assert_eq!(data.as_object().expect("event data").len(), 1);
    assert_eq!(data["event_id"], delivery["event_id"]);
    let captured = f.claim_capture();
    assert_eq!(captured["dots_stage"], "claim");
    assert!(!f.response("message_send", json!({"by":"dot","to":["user"],"text":"forbidden fixed-grant business","_cccc_web_binding":captured})).ok);

    let claimed = f.claim(&captured);
    assert_eq!(claimed["status"], "work_available");
    assert_eq!(claimed["turn"]["event_ids"], json!([source]));
    let token = claimed["actor_token"]
        .as_str()
        .expect("first claim capability");
    assert_eq!(token.len(), 43);
    assert!(
        token != f.subscription_token,
        "execution and claim credentials differ"
    );
    let live = store::dots_binding_for_token(&f.private(), token, "cccc_message_send")
        .expect("claimed task authority");
    assert_eq!(live["dots_stage"], "execution");
    assert_eq!(live["dots_turn_id"], claimed["turn"]["turn_id"]);
    for duplicate in [f.claim(&captured), f.claim(&f.claim_capture())] {
        assert_eq!(duplicate["status"], "turn_in_progress");
        assert_eq!(duplicate["turn"], Value::Null);
        assert!(duplicate.get("actor_token").is_none());
    }
    assert!(
        !f.call("dots_events_status", json!({}))
            .to_string()
            .contains(token)
    );
    let ledger_path = GroupStore::new(f.home.clone())
        .expect("groups")
        .ledger_path(&f.group.group_id)
        .expect("ledger path");
    assert!(
        !std::fs::read_to_string(ledger_path)
            .expect("ledger")
            .contains(token)
    );
    f.complete(&claimed["turn"], &live);
    assert!(store::dots_binding_for_token(&f.private(), token, "cccc_message_send").is_none());
}

#[test]
fn dots_claim_without_a_wake_is_idle_and_cannot_borrow_a_later_wake() {
    let f = Fixture::new();
    f.start();
    let idle_capture = f.claim_capture();
    assert_eq!(idle_capture["dots_wake_id"], Value::Null);
    let idle = f.claim(&idle_capture);
    assert_eq!(idle["status"], "idle");
    assert_eq!(idle["turn"], Value::Null);
    assert!(idle.get("actor_token").is_none());

    f.source("arrived after the old claim snapshot");
    f.delivery();
    assert!(
        !f.response(
            "dots_work_claim",
            json!({"by":"dot","_cccc_web_binding":idle_capture})
        )
        .ok
    );
    let fresh = f.claim(&f.claim_capture());
    assert_eq!(fresh["status"], "work_available");
    assert!(fresh["actor_token"].is_string());
}

#[test]
fn dots_callback_receipt_first_claim_duplicates_completion_and_backlog_are_distinct() {
    let f = Fixture::new();
    f.start();
    let sources = (0..22)
        .map(|i| f.source(&format!("source {i}")))
        .collect::<Vec<_>>();
    let delivery = f.delivery();
    assert!(delivery.is_object());
    assert_eq!(delivery["event"]["eventId"], delivery["event_id"]);
    assert!(!delivery["event"].to_string().contains("source 0"));
    assert!(
        super::super::runtime_delivery::latest_state(
            &f.home,
            &f.group.group_id,
            "dot",
            &sources[0]
        )
        .expect("delivery lookup")
        .is_none()
    );
    assert_eq!(f.received(&delivery, 200)["recorded"], true);
    assert_eq!(
        f.call("dots_events_status", json!({}))["phase"],
        "delivered"
    );
    assert!(f.delivery().is_null());
    let captured = f.captured(&delivery, "cccc_runtime_wait_next_turn");
    let turn = f.claim(&captured)["turn"].clone();
    assert_eq!(turn["event_ids"].as_array().expect("batch").len(), 20);
    assert_eq!(f.claim(&captured)["turn"], Value::Null);
    let live = f.captured(&delivery, "cccc_message_send");
    assert!(
        !f.response(
            "dots_work_claim",
            json!({"by":"dot","_cccc_web_binding":live})
        )
        .ok
    );
    assert!(!f.response("message_send", json!({"by":"dot","to":["user"],"text":"forbidden wake business","_cccc_web_binding":captured})).ok);
    f.complete(&turn, &live);
    let done = f.captured(&delivery, "cccc_runtime_complete_turn");
    let retry = f.complete(&turn, &done);
    assert_eq!(retry["processed_event_ids"], turn["event_ids"]);
    let next = f.delivery();
    assert!(next.is_object());
    assert_ne!(next["event_id"], delivery["event_id"]);
    let turn = f.claim(&f.captured(&next, "cccc_runtime_wait_next_turn"))["turn"].clone();
    assert_eq!(turn["event_ids"], json!(&sources[20..]));
    f.complete(&turn, &f.captured(&next, "cccc_runtime_complete_turn"));
    assert!(f.delivery().is_null());
    assert_eq!(f.call("dots_events_status", json!({}))["phase"], "ready");
}

#[test]
fn dots_stop_start_fences_late_work_and_requires_explicit_resolution_without_replay() {
    let f = Fixture::new();
    f.start();
    f.source("accepted original input");
    let delivery = f.delivery();
    let original_claim = f.claim_capture();
    let turn = f.claim(&original_claim)["turn"].clone();
    let live = f.captured(&delivery, "cccc_runtime_complete_turn");
    let subscription = f.private()["dots"]["subscription"].clone();
    f.call("actor_stop", json!({}));
    assert!(!f.response("runtime_complete_turn", json!({"by":"dot","turn_id":turn["turn_id"],"event_ids":turn["event_ids"],"_cccc_web_binding":live})).ok);
    assert_eq!(f.private()["dots"]["subscription"], subscription);
    assert_eq!(f.received(&delivery, 200)["recorded"], false);
    f.call("actor_start", json!({}));
    assert!(
        !f.response(
            "dots_work_claim",
            json!({"by":"dot","_cccc_web_binding":original_claim})
        )
        .ok
    );
    let busy = f.claim(&f.claim_capture());
    assert_eq!(busy["status"], "turn_in_progress");
    assert_eq!(busy["turn"], Value::Null);
    assert!(busy.get("actor_token").is_none());
    let epoch = f.private()["dots"]["epoch"].clone();
    f.call("actor_start", json!({}));
    assert_eq!(f.private()["dots"]["epoch"], epoch);
    assert_eq!(
        f.call("dots_events_status", json!({}))["phase"],
        "interrupted"
    );
    assert!(f.delivery().is_null());
    assert!(
        !f.response(
            "web_model_runtime_recover_turn",
            json!({"by":"dot","event_ids":turn["event_ids"]})
        )
        .ok
    );
    f.call(
        "dots_work_control",
        json!({"action":"resolve","turn_id":turn["turn_id"],"event_ids":turn["event_ids"]}),
    );
    assert!(f.delivery().is_null());
    let current = f.call("dots_events_status", json!({}));
    assert_eq!(current["last_execution"]["status"], "cancelled");
    assert_eq!(current["active_turn_id"], "");
    f.source("new independent input");
    assert!(f.delivery().is_object());
}

#[test]
fn dots_bounded_same_event_retries_ignore_late_callback_and_private_error_text() {
    let f = Fixture::new();
    f.start();
    f.source("one input");
    let first = f.delivery();
    assert!(f.delivery().is_null());
    for attempt in 1..=3 {
        let delivery = if attempt == 1 {
            first.clone()
        } else {
            f.delivery()
        };
        assert_eq!(delivery["event_id"], first["event_id"]);
        assert_eq!(delivery["attempt"], attempt);
        f.call("dots_events_delivery_record", json!({"connector_id":f.connector_id,"event_id":delivery["event_id"],
            "epoch":delivery["epoch"],"attempt":attempt,"error_code":"synthetic-private-network-value"}));
        let wake = f.private()["dots"]["wake"].clone();
        if attempt < 3 {
            assert_eq!(
                wake["retry_at_ms"].as_i64().expect("retry deadline")
                    - wake["updated_at_ms"]
                        .as_i64()
                        .expect("recorded callback time"),
                if attempt == 1 { 2_000 } else { 4_000 }
            );
            assert!(
                f.delivery().is_null(),
                "backoff must prevent an immediate retry"
            );
        } else {
            assert!(
                wake["retry_at_ms"].is_null(),
                "exhausted attempts do not schedule work"
            );
        }
        store::dots_update(&f.home, &f.connector_id, |c| {
            c["dots"]["wake"]["retry_at_ms"] = json!(0);
            Ok(())
        })
        .expect("advance owned retry fixture");
    }
    assert!(f.delivery().is_null());
    assert_eq!(f.received(&first, 200)["recorded"], false);
    let status = f.call("dots_events_status", json!({}));
    assert_eq!(status["phase"], "failed");
    assert_eq!(status["error_code"], "dots_callback_failed");
    assert!(
        !f.private()
            .to_string()
            .contains("synthetic-private-network-value")
    );
}

#[test]
fn dots_subscription_refresh_rotation_expiry_and_unsubscribe_preserve_exact_authority() {
    let f = Fixture::new();
    f.start();
    f.source("queued input");
    let delivery = f.delivery();
    let old_execution_token = f.wake_token(&delivery);
    let epoch = f.private()["dots"]["epoch"].clone();
    let verified_at = chrono::Utc::now().timestamp_millis() - 5000;
    let mut refreshed = f.subscription_args("rotated-synthetic-signing-secret");
    refreshed["verified_at_ms"] = json!(verified_at);
    f.call("dots_events_subscribe", refreshed);
    assert_eq!(
        f.private()["dots"]["subscription"]["verified_at_ms"],
        verified_at
    );
    assert_eq!(f.private()["dots"]["epoch"], epoch);
    assert_eq!(
        f.private()["dots"]["subscription"]["previous_secret"],
        "synthetic-signing-secret"
    );
    assert_eq!(
        f.private()["dots"]["subscription"]["expires_at_ms"],
        Value::Null
    );
    store::dots_update(&f.home, &f.connector_id, |c| {
        c["dots"]["subscription"]["expires_at_ms"] = json!(1);
        Ok(())
    })
    .expect("expire owned subscription");
    assert!(f.delivery().is_null());
    let expired = f.claim(&f.captured(&delivery, "cccc_runtime_wait_next_turn"));
    assert_eq!(expired["status"], "idle");
    assert_eq!(expired["turn"], Value::Null);
    assert!(expired.get("actor_token").is_none());
    f.call(
        "dots_events_subscribe",
        f.subscription_args("rotated-synthetic-signing-secret"),
    );
    f.call("dots_events_unsubscribe", f.subscription_args("unused"));
    assert_ne!(f.private()["dots"]["epoch"], epoch);
    assert_eq!(f.call("dots_events_status", json!({}))["subscribed"], false);
    assert!(
        store::dots_binding_for_token(&f.private(), &old_execution_token, "cccc_message_send")
            .is_none()
    );
}

#[test]
fn dots_restart_with_missing_execution_projection_keeps_turn_and_user_can_resolve() {
    let f = Fixture::new();
    f.start();
    f.source("accepted before private projection commit");
    let delivery = f.delivery();
    let turn = f.claim(&f.captured(&delivery, "cccc_runtime_wait_next_turn"))["turn"].clone();
    store::dots_update(&f.home, &f.connector_id, |c| {
        c["dots"]["execution"] = Value::Null;
        Ok(())
    })
    .expect("simulate crash between durable stores");
    assert!(f.delivery().is_null());
    let blocked = f.claim(&f.claim_capture());
    assert_eq!(blocked["status"], "turn_in_progress");
    assert_eq!(blocked["turn"], Value::Null);
    assert!(blocked.get("actor_token").is_none());
    assert_eq!(
        f.call("dots_events_status", json!({}))["phase"],
        "interrupted"
    );
    f.call(
        "dots_work_control",
        json!({"action":"resolve","turn_id":turn["turn_id"],"event_ids":turn["event_ids"]}),
    );
    let states = integration_state::group_get(
        &GroupStore::new(f.home.clone()).expect("groups"),
        &f.group.group_id,
        "runtime_states",
    )
    .expect("canonical states");
    assert_eq!(states["dot"]["active_turn_id"], "");
    assert!(f.delivery().is_null());
}

#[test]
fn dots_permanent_callback_failures_do_not_retry_and_stop_start_only_rewakes_unaccepted_work() {
    for http in [410, 413] {
        let f = Fixture::new();
        f.start();
        let source = f.source("unaccepted input survives callback failure");
        let first = f.delivery();
        f.received(&first, http);
        store::dots_update(&f.home, &f.connector_id, |c| {
            c["dots"]["wake"]["retry_at_ms"] = json!(0);
            Ok(())
        })
        .expect("advance owned failure fixture");
        assert!(f.delivery().is_null());
        let failed_wake = f.claim(&f.claim_capture());
        assert_eq!(failed_wake["status"], "idle");
        assert_eq!(failed_wake["turn"], Value::Null);
        assert!(failed_wake.get("actor_token").is_none());
        assert_eq!(f.private()["dots"]["wake"]["attempt"], 1);
        assert!(
            super::super::runtime_delivery::latest_state(
                &f.home,
                &f.group.group_id,
                "dot",
                &source
            )
            .expect("source retained")
            .is_none()
        );
        f.call("actor_stop", json!({}));
        f.call("actor_start", json!({}));
        if http == 410 {
            assert!(f.delivery().is_null());
            f.call(
                "dots_events_subscribe",
                f.subscription_args("renewed-synthetic-secret"),
            );
        }
        let next = f.delivery();
        assert!(next.is_object());
        assert_ne!(next["event_id"], first["event_id"]);
        let turn = f.claim(&f.captured(&next, "cccc_runtime_wait_next_turn"))["turn"].clone();
        assert_eq!(turn["event_ids"], json!([source]));
    }
}

#[test]
fn dots_late_unsubscribe_cannot_retire_a_new_callback_subscription() {
    let f = Fixture::new();
    f.start();
    let old = f.subscription_args("synthetic-signing-secret");
    let mut replacement = old.clone();
    replacement["callback_url"] = json!("https://example.com/new-owned-callback");
    replacement["subscription_id"] = json!("dots_sub_replacement");
    f.call("dots_events_subscribe", replacement);
    let epoch = f.private()["dots"]["epoch"].clone();
    let mut late = old;
    late["expected_epoch"] = epoch.clone();
    let result = f.call("dots_events_unsubscribe", late);
    assert_eq!(result["removed"], false);
    assert_eq!(f.private()["dots"]["epoch"], epoch);
    assert_eq!(f.call("dots_events_status", json!({}))["subscribed"], true);
}

#[test]
fn dots_user_resolution_preserves_committed_done_receipt_after_projection_crash() {
    let f = Fixture::new();
    f.start();
    f.source("completed before projection cleanup");
    let delivery = f.delivery();
    let turn = f.claim(&f.captured(&delivery, "cccc_runtime_wait_next_turn"))["turn"].clone();
    let completion = super::super::runtime_completion::Completion {
        turn_id: turn["turn_id"].as_str().expect("turn ID").into(),
        event_ids: turn["event_ids"]
            .as_array()
            .expect("event IDs")
            .iter()
            .map(|id| id.as_str().expect("source ID").to_owned())
            .collect(),
        status: "done".into(),
        delivery_id: "durable-cloud-completion".into(),
    };
    let receipt =
        super::super::runtime_completion::append(&f.home, &f.group.group_id, "dot", &completion)
            .expect("durable receipt before simulated crash");
    f.call("actor_stop", json!({}));
    let result = f.call(
        "dots_work_control",
        json!({"action":"resolve","turn_id":turn["turn_id"],"event_ids":turn["event_ids"]}),
    );
    assert_eq!(result["status"], "done");
    assert_eq!(result["delivery_id"], "durable-cloud-completion");
    assert_eq!(result["completion_event"], json!(receipt));
    assert_eq!(
        f.call("dots_events_status", json!({}))["active_turn_id"],
        ""
    );
    assert!(f.delivery().is_null());
}

#[test]
fn dots_revoked_binding_does_not_strand_explicit_user_resolution() {
    let f = Fixture::new();
    f.start();
    f.source("accepted before revocation");
    let wake = f.delivery();
    let turn = f.claim(&f.captured(&wake, "cccc_runtime_wait_next_turn"))["turn"].clone();
    let captured = f.captured(&wake, "cccc_runtime_complete_turn");
    store::revoke(&f.home, &f.connector_id).expect("revoke original connector");
    let path = f.home.root().join("web_model_connectors.yaml");
    let revoked_bytes = std::fs::read(&path).expect("revoked configuration");
    assert!(!f.response("runtime_complete_turn", json!({"by":"dot","turn_id":turn["turn_id"],"event_ids":turn["event_ids"],"_cccc_web_binding":captured})).ok);
    f.call(
        "dots_work_control",
        json!({"action":"resolve","turn_id":turn["turn_id"],"event_ids":turn["event_ids"]}),
    );
    assert_eq!(
        f.call("dots_events_status", json!({}))["active_turn_id"],
        ""
    );
    assert_eq!(
        std::fs::read(path).expect("revocation retained"),
        revoked_bytes
    );
    assert!(f.delivery().is_null());
}

#[test]
fn dots_partial_admission_crash_resolution_does_not_replay_claimed_source() {
    let f = Fixture::new();
    f.start();
    let source = f.source("claimed before accepted commit");
    f.delivery();
    let groups = GroupStore::new(f.home.clone()).expect("groups");
    let group = groups.load(&f.group.group_id).expect("active group");
    let actor = &group.actors[0];
    super::super::runtime_delivery::claim(&f.home, &group, actor, &source, "web_model_pull", false)
        .expect("source claim");
    integration_state::group_update(&groups, &group.group_id, "runtime_states", |states| {
        *states = json!({"dot":{"status":"working","active_turn_id":"partial-admission-crash",
            "active_event_ids":[source],"latest_event_id":source}});
        Ok(())
    })
    .expect("simulate crash immediately before accepted delivery commit");
    let blocked = f.claim(&f.claim_capture());
    assert_eq!(blocked["status"], "turn_in_progress");
    assert_eq!(blocked["turn"], Value::Null);
    assert!(blocked.get("actor_token").is_none());
    assert_eq!(
        super::super::runtime_delivery::latest_state(&f.home, &group.group_id, "dot", &source)
            .expect("delivery")
            .expect("claimed source")
            .0,
        "claimed"
    );
    f.call(
        "dots_work_control",
        json!({"action":"resolve","turn_id":"partial-admission-crash","event_ids":[source]}),
    );
    assert!(
        f.delivery().is_null(),
        "explicitly resolved old source must never be admitted again"
    );
    assert_eq!(
        super::super::runtime_delivery::latest_state(&f.home, &group.group_id, "dot", &source)
            .expect("terminal delivery")
            .expect("source retained")
            .0,
        "ambiguous"
    );
}
