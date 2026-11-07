//! Runtime switches through real dispatch on an isolated home. Native sources
//! are Codex registrations that can receive but name no program, so a delivery
//! never launches a provider; managed targets are never started.
use super::*;
use crate::ops::actor_delivery_worker::{self, BatchOutcome};
use crate::ops::actor_secrets;
use crate::ops::runtime_delivery::DeliveryOutcome;
use cccc_contracts::local_session::LocalSessionRuntime;
use cccc_contracts::{ActorRuntime, DaemonResponse};
use cccc_core::local_sessions::{self as native, NativeHost, NativeSession};
use cccc_core::profiles::ProfileStore;
use cccc_core::{ledger, scope};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

mod receiver;
mod web_model;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group_id: String,
    workspace: PathBuf,
}

impl Fixture {
    /// Group with one managed custom Codex member `x` and an attached workspace.
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let created = store.create("Runtime switch", "").expect("group");
        let mut group = group_scope::attach(
            &store,
            &created.group_id,
            scope::detect(&workspace).expect("scope"),
        )
        .expect("attach scope");
        let mut actor = Actor::new("x");
        actor.runtime = ActorRuntime::Codex;
        actor.command = vec!["codex".into()];
        actor.env = BTreeMap::from([("PUBLIC".into(), "1".into())]);
        actors::add(&mut group, actor).expect("member");
        store.save(&group).expect("save");
        Self {
            _temp: temp,
            home,
            group_id: created.group_id,
            workspace,
        }
    }

    /// A native Codex session in the workspace. It names no program, so a
    /// delivery attempt never launches one.
    fn source(&self, key: &str, connected: bool) -> NativeSession {
        native::write(
            &self.home,
            NativeSession {
                source_id: LocalSessionRuntime::Codex.source_id(key),
                runtime: LocalSessionRuntime::Codex,
                native_session_id: format!("thread-{key}"),
                host: NativeHost::capture(std::process::id()).expect("host"),
                cwd: self.workspace.to_string_lossy().into_owned(),
                title: format!("Codex {key}"),
                origin: None,
                revision: format!("revision-{key}"),
                context_epoch: 0,
                codex_home: None,
                codex_can_receive: connected,
                codex_program: None,
                claude_config_dir: None,
                pi_agent_dir: None,
                opencode_config_dir: None,
                opencode_server: None,
                retired: None,
            },
        )
        .expect("native source")
    }

    /// A native Claude session in the workspace; it receives through the
    /// receiver queue once attached, without any provider process.
    fn claude_source(&self) -> NativeSession {
        let host = NativeHost::capture(std::process::id()).expect("host");
        let config = self.workspace.join("claude-config");
        std::fs::create_dir_all(&config).expect("isolated Claude config");
        native::write(
            &self.home,
            NativeSession {
                source_id: LocalSessionRuntime::Claude.source_id(&host.key()),
                runtime: LocalSessionRuntime::Claude,
                native_session_id: "claude-conversation".into(),
                host,
                cwd: self.workspace.to_string_lossy().into_owned(),
                title: "Claude session".into(),
                origin: None,
                revision: "claude-revision".into(),
                context_epoch: 0,
                codex_home: None,
                codex_can_receive: false,
                codex_program: None,
                claude_config_dir: Some(config),
                pi_agent_dir: None,
                opencode_config_dir: None,
                opencode_server: None,
                retired: None,
            },
        )
        .expect("native source")
    }

    fn receiver(source: &NativeSession) -> Value {
        json!({"source_id": source.source_id, "connection_id": "receiver"})
    }

    /// The next handoff offered to the attached receiver, waiting up to `wait`.
    fn next_handoff(&self, source: &NativeSession, wait: Duration) -> Option<Value> {
        let deadline = Instant::now() + wait;
        loop {
            let next = self.request("local_session_receiver_next", Self::receiver(source));
            assert!(next.ok, "{:?}", next.error);
            if !next.result["handoff"].is_null() {
                return Some(next.result["handoff"].clone());
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn store(&self) -> GroupStore {
        GroupStore::new(self.home.clone()).expect("store")
    }

    fn group(&self) -> GroupDoc {
        self.store().load(&self.group_id).expect("group")
    }

    fn actor(&self) -> Actor {
        actors::find(&self.group(), "x").cloned().expect("member x")
    }

    fn mutate(&self, change: impl FnOnce(&mut GroupDoc)) {
        self.store()
            .mutate(&self.group_id, |doc| {
                change(doc);
                Ok(())
            })
            .expect("mutate");
    }

    fn request(&self, op: &str, args: Value) -> DaemonResponse {
        let mut args = args.as_object().cloned().expect("args");
        args.entry("group_id")
            .or_insert_with(|| json!(self.group_id));
        args.entry("by").or_insert_with(|| json!("user"));
        crate::dispatch::dispatch(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args,
            },
        )
    }

    /// Switch `x` as it is stored now; `extra` replaces request arguments.
    fn try_switch(&self, target: Value, extra: Value) -> DaemonResponse {
        let actor = self.actor();
        let mut args = json!({
            "actor_id": "x",
            "expected_actor_generation": actors::generation_identity(&actor),
            "expected_actor_updated_at": actor.updated_at,
            "expected_source_id": actor.external_session_id,
            "target": target,
        });
        if let (Some(args), Some(extra)) = (args.as_object_mut(), extra.as_object()) {
            args.extend(extra.clone());
        }
        self.request("actor_runtime_switch", args)
    }

    fn switch(&self, target: Value) -> Value {
        let response = self.try_switch(target, json!({}));
        assert!(response.ok, "switch: {:?}", response.error);
        Value::Object(response.result)
    }

    fn refused(&self, target: Value) -> String {
        code(self.try_switch(target, json!({})))
    }

    fn events(&self, kind: &str) -> Vec<Event> {
        let path = self.store().ledger_path(&self.group_id).expect("ledger");
        ledger::read_all(&path)
            .expect("ledger")
            .into_iter()
            .filter(|event| event.kind == kind)
            .collect()
    }

    fn notices(&self) -> Vec<Event> {
        self.events("system.notify")
            .into_iter()
            .filter(|event| event.data.get("kind") == Some(&json!("runtime_switch_handoff")))
            .collect()
    }

    /// A message for `x`, in the ledger and handed to dispatch.
    fn send(&self, group: &GroupDoc, actor: &Actor, text: String) -> Event {
        let path = self.store().ledger_path(&self.group_id).expect("ledger");
        let mut message = Event::new("chat.message", &self.group_id);
        message.by = "user".into();
        message.data.insert("text".into(), json!(text));
        message.data.insert("to".into(), json!(["x"]));
        message.data.insert("message_mode".into(), json!("send"));
        ledger::append(&path, &message).expect("message");
        actor_delivery::dispatch_to(
            &self.home,
            group,
            &message,
            std::slice::from_ref(actor),
            false,
        );
        message
    }

    fn delivery_state(&self, event: &Event) -> Option<String> {
        runtime_delivery::latest_state(&self.home, &self.group_id, "x", &event.id)
            .expect("delivery state")
            .map(|(state, _)| state)
    }

    fn wait_for_delivery(&self, event: &Event, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.delivery_state(event).as_deref() != Some(expected) {
            assert!(
                Instant::now() < deadline,
                "{expected}: {:?}",
                self.delivery_state(event)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// After a switch to `source`: `cut` may have reached the old runtime, so
    /// the handoff lists it and it is not sent again, and `queued` moves on.
    fn assert_cut_off_work_moves_on(&self, source: &NativeSession, cut: &Event, queued: &Event) {
        assert_eq!(self.delivery_state(cut).as_deref(), Some("ambiguous"));
        let notice = self.notices().pop().expect("notice");
        assert_eq!(notice.data["unconfirmed"][0]["event_id"], json!(cut.id));
        let first = self
            .next_handoff(source, Duration::from_secs(5))
            .expect("the handoff");
        assert_eq!(first["event_ids"], json!([notice.id]));
        let mut ack = Self::receiver(source);
        ack["handoff_id"] = first["handoff_id"].clone();
        ack["state"] = json!("accepted");
        assert!(self.request("local_session_receiver_ack", ack).ok);
        let second = self
            .next_handoff(source, Duration::from_secs(5))
            .expect("the queued message");
        assert_eq!(
            second["event_ids"],
            json!([queued.id]),
            "the cut-off message is never repeated"
        );
    }

    fn set_private(&self) {
        actor_secrets::replace(
            &self.home,
            &self.group_id,
            "x",
            BTreeMap::from([("SECRET".into(), "s".into())]),
        )
        .expect("private env");
    }

    fn private_keys(&self) -> Vec<String> {
        actor_secrets::values(&self.home, &self.group_id, "x")
            .expect("private env")
            .into_keys()
            .collect()
    }

    /// Make completion fail at its last step, after the audit and notice: the
    /// member's private-env file becomes a directory, which file removal
    /// refuses for every user.
    fn block_private_env_cleanup(&self) -> PathBuf {
        self.set_private();
        let directory = self
            .home
            .root()
            .join("state/secrets/actors")
            .join(&self.group_id);
        let file = std::fs::read_dir(&directory)
            .expect("private env directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .expect("private env file");
        std::fs::remove_file(&file).expect("remove private env file");
        std::fs::create_dir(&file).expect("block cleanup");
        file
    }

    /// Undo the block and leave a private value that completion must remove.
    fn unblock(&self, blocked: &PathBuf) {
        std::fs::remove_dir(blocked).expect("unblock cleanup");
        self.set_private();
    }
}

#[test]
fn native_foreman_defaults_wait_for_switch_completion() {
    let fixture = Fixture::new();
    fixture.mutate(|group| group.running = true);
    let source = fixture.source("baseline", true);
    let blocked = fixture.block_private_env_cleanup();
    let result = fixture.switch(native_target(&source));
    assert_eq!(result["completion"], "pending");
    let caps = cccc_core::capabilities::CapabilityStore::new(fixture.home.clone());
    let enabled = |id| {
        caps.is_enabled_for(id, &fixture.group_id, "x", "actor")
            .expect("state")
    };
    assert!(!enabled("pack:group-runtime"));
    assert!(!enabled("pack:diagnostics"));

    fixture.unblock(&blocked);
    let response = fixture.request("actor_runtime_switch_complete", json!({"actor_id":"x"}));
    assert!(response.ok, "completion: {:?}", response.error);
    assert!(enabled("pack:group-runtime"));
    assert!(enabled("pack:diagnostics"));
}

impl Drop for Fixture {
    fn drop(&mut self) {
        actor_delivery::shutdown_group(&self.group_id);
        let _ = cccc_runtime::stop(&self.group_id, "x");
        let _ = cccc_runtime::stop(&self.group_id, "y");
        crate::ops::deepseek_runtime::stop_group(&self.group_id);
    }
}

fn code(response: DaemonResponse) -> String {
    assert!(!response.ok, "expected a refusal: {:?}", response.result);
    response.error.map(|error| error.code).unwrap_or_default()
}

/// A managed target whose terminal is a shell that only waits, so starting it
/// launches no provider.
fn idle_terminal(start: bool) -> Value {
    json!({
        "kind": "managed",
        "patch": {"runtime": "custom", "command": ["sh", "-c", "exec sleep 60"]},
        "start": start,
    })
}

fn native_target(source: &NativeSession) -> Value {
    json!({
        "kind": "native",
        "source_id": source.source_id,
        "expected_native_session_id": source.native_session_id,
    })
}

#[test]
fn managed_to_native_discards_the_managed_configuration() {
    let fixture = Fixture::new();
    fixture.set_private();
    let source = fixture.source("a", true);

    let result = fixture.switch(native_target(&source));

    assert_eq!(result["completion"], "done");
    assert_eq!(result["start"], "native");
    assert_eq!(result["from"]["kind"], "managed");
    assert_eq!(result["to"]["source_id"], json!(source.source_id));
    let actor = fixture.actor();
    assert_eq!(actor.external_session_id, source.source_id);
    assert!(actor.command.is_empty() && actor.env.is_empty());
    assert_eq!(
        actor.title, source.title,
        "an empty title takes the session's"
    );
    assert_eq!(actor.default_scope_key, fixture.group().scopes[0].scope_key);
    assert!(fixture.private_keys().is_empty());
    // Completed, and no native session was left: nothing to remember.
    assert!(runtime_switches::records(&fixture.group()).is_empty());
    let audit = fixture.events("actor.update");
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].data["reason"], "runtime_switched");
    assert_eq!(audit[0].data["op_id"], result["op_id"]);
    let notices = fixture.notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].data["target_actor_id"], "x");
    assert_eq!(result["notice_event_id"], json!(notices[0].id));
}

#[test]
fn native_switches_keep_the_member_and_remember_the_session_left() {
    let fixture = Fixture::new();
    let first = fixture.source("a", true);
    let second = fixture.source("b", true);
    fixture.switch(native_target(&first));
    assert_eq!(
        fixture.refused(native_target(&first)),
        "actor_switch_unsupported"
    );

    let result = fixture.switch(native_target(&second));

    assert_eq!(result["from"]["source_id"], json!(first.source_id));
    let actor = fixture.actor();
    assert_eq!(actor.external_session_id, second.source_id);
    assert_eq!(
        actor.title, second.title,
        "a session title follows the session"
    );
    let left = runtime_switches::left_source(&fixture.group(), &first.source_id)
        .expect("the session left is remembered");
    assert!(!left.pending);

    // A title the user chose stays; returning to a session lifts its record.
    fixture.mutate(|doc| doc.actors[0].title = "Reviewer".into());
    fixture.switch(native_target(&first));
    assert_eq!(fixture.actor().title, "Reviewer");
    let group = fixture.group();
    assert!(runtime_switches::left_source(&group, &first.source_id).is_none());
    assert!(runtime_switches::left_source(&group, &second.source_id).is_some());
}

#[test]
fn native_to_managed_waits_for_a_stopped_group() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));

    let result = fixture.switch(idle_terminal(true));

    assert_eq!(result["start"], "pending", "it starts with its Group");
    let actor = fixture.actor();
    assert!(actor.external_session_id.is_empty());
    assert!(actor.title.is_empty(), "the session's title does not stay");
    assert_eq!(actor.runtime, ActorRuntime::Custom);
    assert!(actor.enabled);
    let group = fixture.group();
    assert!(
        !group.running,
        "switching a member does not start its Group"
    );
    assert!(runtime_switches::left_source(&group, &source.source_id).is_some());
}

#[test]
fn native_to_managed_starts_in_a_running_group() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));
    fixture.mutate(|doc| doc.running = true);

    let result = fixture.switch(idle_terminal(true));

    assert_eq!(result["start"], "started", "{result}");
    assert!(fixture.group().running);
}

#[test]
fn switching_without_start_stops_the_member_and_its_empty_group() {
    let fixture = Fixture::new();
    fixture.mutate(|doc| doc.running = true);

    let result = fixture.switch(idle_terminal(false));

    assert_eq!(result["start"], "stopped");
    assert!(!fixture.actor().enabled);
    assert!(!fixture.group().running, "no enabled member is left to run");
}

#[test]
fn an_unfinished_switch_holds_the_member_until_completion() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    let blocked = fixture.block_private_env_cleanup();

    let result = fixture.switch(native_target(&source));

    assert_eq!(result["completion"], "pending");
    assert_eq!(result["start"], "held");
    let group = fixture.group();
    let actor = fixture.actor();
    assert_eq!(
        actor.external_session_id, source.source_id,
        "the commit stands"
    );
    assert!(runtime_switches::pending_for(&group, &actor).is_some());
    let mut message = Event::new("chat.message", &fixture.group_id);
    message.by = "user".into();
    let held = actor_delivery::dispatch_to(
        &fixture.home,
        &group,
        &message,
        std::slice::from_ref(&actor),
        false,
    );
    assert_eq!(held.queued, 0, "no delivery before the handoff is written");
    let mut released = group.clone();
    runtime_switches::set_records(&mut released, &[]);
    let control = actor_delivery::dispatch_to(
        &fixture.home,
        &released,
        &message,
        std::slice::from_ref(&actor),
        false,
    );
    assert_eq!(
        control.queued, 1,
        "the same member is otherwise deliverable"
    );
    for (op, args) in [
        (
            "actor_update",
            json!({"actor_id": "x", "patch": {"title": "t"}}),
        ),
        ("actor_start", json!({"actor_id": "x"})),
        ("actor_remove", json!({"actor_id": "x"})),
        ("actor_runtime_switch_complete", json!({"actor_id": "x"})),
    ] {
        assert_eq!(
            code(fixture.request(op, args)),
            "actor_switch_pending",
            "{op}"
        );
    }
    assert_eq!(
        fixture.refused(json!({"kind": "managed"})),
        "actor_switch_pending"
    );
    let stop = fixture.request("actor_stop", json!({"actor_id": "x"}));
    assert!(stop.ok, "stop stays available: {:?}", stop.error);

    fixture.unblock(&blocked);
    // Completion could succeed now, yet only the retry may do it: it also
    // resumes the member, which these operations would leave undone.
    for (op, args) in [
        (
            "actor_update",
            json!({"actor_id": "x", "patch": {"title": "t"}}),
        ),
        ("actor_remove", json!({"actor_id": "x"})),
        ("group_reset", json!({"confirm": fixture.group_id})),
    ] {
        assert_eq!(
            code(fixture.request(op, args)),
            "actor_switch_pending",
            "{op}"
        );
    }
    assert_eq!(
        fixture.refused(json!({"kind": "managed"})),
        "actor_switch_pending"
    );
    // Authority is checked before the pending state is revealed.
    for (op, refusal) in [
        ("actor_update", "invalid_args"),
        ("actor_start", "invalid_args"),
        ("actor_remove", "invalid_args"),
        ("actor_env_private_update", "permission_denied"),
    ] {
        let unauthorized = fixture.request(
            op,
            json!({"actor_id": "x", "by": "stranger", "patch": {"title": "t"}, "set": {"K": "v"}}),
        );
        assert_eq!(code(unauthorized), refusal, "{op}");
    }
    assert!(runtime_switches::pending_for(&fixture.group(), &fixture.actor()).is_some());
    assert!(
        !fixture.private_keys().is_empty(),
        "nothing completed the switch"
    );
    let done = fixture.request("actor_runtime_switch_complete", json!({"actor_id": "x"}));

    assert!(done.ok, "{:?}", done.error);
    assert_eq!(done.result["completion"], "done");
    assert_eq!(done.result["start"], "native");
    assert!(fixture.private_keys().is_empty());
    assert!(runtime_switches::pending_for(&fixture.group(), &fixture.actor()).is_none());
    let again = fixture.request("actor_runtime_switch_complete", json!({"actor_id": "x"}));
    assert_eq!(again.result["switched"], false);
    // Every attempt found what the first one wrote.
    assert_eq!(fixture.events("actor.update").len(), 1);
    let notices = fixture.notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(done.result["notice_event_id"], json!(notices[0].id));
}

#[test]
fn an_owner_not_confirmed_stopped_leaves_even_a_native_member_unknown() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));
    let current = fixture.actor();
    assert!(!current.external_session_id.is_empty());
    let request = DaemonRequest {
        v: 1,
        op: "actor_runtime_switch".into(),
        args: json!({"group_id": fixture.group_id, "actor_id": "x", "by": "user"})
            .as_object()
            .cloned()
            .expect("args"),
    };
    let hold = Hold {
        _browser_work: actor_runtime::hold_browser_work(&fixture.home, &fixture.group_id, "x")
            .expect("nothing else holds it"),
        _fence: actor_delivery::fence_actor(&fixture.group_id, "x"),
    };

    let error = incomplete(
        &fixture.home,
        &request,
        &current,
        None,
        hold,
        OpError::new("stop_failed", "the previous owner did not stop"),
    );

    assert_eq!(error.code, "actor_switch_incomplete");
    assert_eq!(error.details["old_owner"], "unknown");
    assert_eq!(error.details["cause"], "stop_failed");
}

#[test]
fn startup_completes_an_unfinished_switch() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    let blocked = fixture.block_private_env_cleanup();
    assert_eq!(fixture.switch(native_target(&source))["start"], "held");
    fixture.unblock(&blocked);

    complete_stranded(&fixture.home, &fixture.group());

    assert!(runtime_switches::records(&fixture.group()).is_empty());
    assert!(fixture.private_keys().is_empty());
    assert_eq!(fixture.notices().len(), 1);
    assert_eq!(fixture.events("actor.update").len(), 1);
}

#[test]
fn the_handoff_lists_recent_unconfirmed_messages_and_keeps_all_in_its_data() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    let actor = fixture.actor();
    let ledger_path = fixture
        .store()
        .ledger_path(&fixture.group_id)
        .expect("ledger");
    let mut sent = Vec::new();
    for index in 0..1000 {
        let mut message = Event::new("chat.message", &fixture.group_id);
        message.by = "user".into();
        message
            .data
            .insert("text".into(), json!(format!("message {index}")));
        ledger::append(&ledger_path, &message).expect("message");
        runtime_delivery::append_state(
            &fixture.home,
            &fixture.group_id,
            "x",
            &actor.created_at,
            &message.id,
            "pty",
            DeliveryOutcome::Claimed,
        )
        .expect("claimed");
        sent.push(message.id);
    }

    fixture.switch(native_target(&source));

    let notice = fixture.notices().pop().expect("notice");
    let unconfirmed = notice.data["unconfirmed"].as_array().expect("full list");
    assert_eq!(unconfirmed.len(), 1000);
    assert_eq!(unconfirmed[0]["event_id"], json!(sent[999]), "newest first");
    assert_eq!(unconfirmed[0]["by"], "user");
    let text = notice.data["message"].as_str().expect("text");
    assert_eq!(
        text.lines().filter(|line| line.starts_with("- ")).count(),
        20
    );
    assert!(text.contains("...and 980 older ones"));
    // Native Codex receives the text as one command argument.
    assert!(text.len() < 8 * 1024, "{} bytes", text.len());
    assert!(!text.contains(&sent[0]), "the oldest stay in the data only");
    let state = runtime_delivery::latest_state(&fixture.home, &fixture.group_id, "x", &sent[0])
        .expect("delivery state");
    assert_eq!(
        state.map(|(state, _)| state).as_deref(),
        Some("ambiguous"),
        "a claim the old owner never confirmed is not delivered again"
    );
}

#[test]
fn refused_switches_leave_the_member_unchanged() {
    let fixture = Fixture::new();
    let free = fixture.source("a", true);
    let busy = fixture.source("b", true);
    let offline = fixture.source("c", false);
    fixture.mutate(|doc| {
        let mut other = Actor::new("y");
        other.runtime = ActorRuntime::Codex;
        other.external_session_id.clone_from(&busy.source_id);
        actors::add(doc, other).expect("second member");
    });
    let before = fixture.group();

    let in_use = fixture
        .try_switch(native_target(&busy), json!({}))
        .error
        .expect("refused");
    assert_eq!(in_use.code, "local_session_source_in_use");
    assert_eq!(in_use.details["actor_id"], "y");
    for (target, expected) in [
        (
            json!({"kind": "native", "source_id": free.source_id, "expected_native_session_id": "another-thread"}),
            "local_session_source_changed",
        ),
        (native_target(&offline), "local_session_offline"),
        (
            json!({"kind": "managed", "patch": {"title": "renamed"}}),
            "invalid_args",
        ),
        (
            json!({"kind": "managed", "profile_id": "shared"}),
            "invalid_args",
        ),
        (
            json!({"kind": "managed", "patch": {"default_scope_key": "/not/attached"}}),
            "scope_not_attached",
        ),
        (
            json!({"kind": "managed", "patch": {"runtime": "claude", "runtime_mode": "acp"}}),
            "invalid_args",
        ),
        (json!({"kind": "other"}), "invalid_args"),
        (json!({"kind": "managed", "start": "false"}), "invalid_args"),
    ] {
        assert_eq!(fixture.refused(target.clone()), expected, "{target}");
    }
    assert_eq!(
        code(fixture.try_switch(
            native_target(&free),
            json!({"expected_actor_updated_at": "2000-01-01T00:00:00Z"}),
        )),
        "actor_switch_conflict"
    );
    for caller in [
        json!({"by": "y"}),
        json!({"by": "system"}),
        json!({"caller_id": "viewer", "is_admin": false}),
        json!({"_cccc_web_binding": {"binding_id": "binding"}}),
    ] {
        assert_eq!(
            code(fixture.try_switch(native_target(&free), caller.clone())),
            "permission_denied",
            "{caller}"
        );
    }
    let after = fixture.group();
    assert_eq!(after.actors, before.actors);
    assert!(runtime_switches::records(&after).is_empty());
    assert!(fixture.events("actor.update").is_empty());
}

#[test]
fn private_env_belongs_to_the_configuration_that_uses_it() {
    let fixture = Fixture::new();
    fixture.set_private();
    let custom = idle_terminal(false);

    // Custom to custom keeps the member's own values.
    fixture.switch(custom.clone());
    assert_eq!(fixture.private_keys(), ["SECRET"]);
    assert_eq!(
        fixture.actor().env.get("PUBLIC").map(String::as_str),
        Some("1")
    );

    // Custom to Profile leaves them dormant behind the Profile.
    let profile = ProfileStore::new(fixture.home.clone())
        .expect("profiles")
        .upsert(
            json!({"id": "shared", "name": "Shared", "runtime": "codex", "command": ["codex"]})
                .as_object()
                .cloned()
                .expect("profile"),
            None,
        )
        .expect("profile");
    assert_eq!(
        fixture.refused(
            json!({"kind": "managed", "profile_id": "shared", "expected_profile_revision": 99})
        ),
        "actor_switch_conflict"
    );
    fixture.switch(json!({
        "kind": "managed",
        "profile_id": "shared",
        "expected_profile_revision": profile["revision"],
        "start": false,
    }));
    assert_eq!(fixture.actor().profile_id, "shared");
    assert_eq!(fixture.private_keys(), ["SECRET"]);

    // Profile to custom discards the dormant configuration with its values.
    fixture.switch(custom);
    let actor = fixture.actor();
    assert!(actor.profile_id.is_empty());
    assert!(actor.env.is_empty(), "dormant public env does not return");
    assert!(fixture.private_keys().is_empty());
}

#[test]
fn managed_targets_follow_the_member_update_rules() {
    let fixture = Fixture::new();
    fixture.mutate(|doc| doc.actors[0].default_scope_key = doc.scopes[0].scope_key.clone());

    fixture.switch(json!({"kind": "managed", "patch": {"default_scope_key": ""}, "start": false}));

    assert_eq!(
        fixture.actor().default_scope_key,
        "",
        "empty follows the Group's active directory"
    );
}

#[test]
fn saving_a_profile_copies_only_the_member_that_was_read() {
    let fixture = Fixture::new();
    fixture.set_private();
    ProfileStore::new(fixture.home.clone())
        .expect("profiles")
        .upsert(
            json!({"id": "saved", "name": "Saved", "runtime": "codex"})
                .as_object()
                .cloned()
                .expect("profile"),
            None,
        )
        .expect("profile");
    let actor = fixture.actor();
    let copy = |updated_at: &str| {
        fixture.request(
            "actor_profile_copy_actor_secrets",
            json!({
                "profile_id": "saved",
                "actor_id": "x",
                "expected_actor_generation": actors::generation_identity(&actor),
                "expected_actor_updated_at": updated_at,
            }),
        )
    };

    assert_eq!(code(copy("2000-01-01T00:00:00Z")), "actor_changed");
    let copied = copy(&actor.updated_at);
    assert!(copied.ok, "{:?}", copied.error);
}

#[test]
fn a_native_member_has_no_private_values_to_copy_into_a_profile() {
    let fixture = Fixture::new();
    fixture.set_private();
    let profiles = ProfileStore::new(fixture.home.clone()).expect("profiles");
    profiles
        .upsert(
            json!({"id": "saved", "name": "Saved", "runtime": "codex"})
                .as_object()
                .cloned()
                .expect("profile"),
            None,
        )
        .expect("profile");
    let copy = || {
        fixture.request(
            "actor_profile_copy_actor_secrets",
            json!({"profile_id": "saved", "actor_id": "x"}),
        )
    };
    let copied = copy();
    assert!(copied.ok, "{:?}", copied.error);
    let saved = profiles.secret_values("saved").expect("saved values");
    assert!(saved.contains_key("SECRET"));
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));

    assert_eq!(code(copy()), "external_session_owned");
    assert_eq!(
        profiles.secret_values("saved").expect("kept values"),
        saved,
        "the Profile keeps its values"
    );
}

#[test]
fn a_managed_target_needs_a_directory_it_can_use_before_anything_stops() {
    let fixture = Fixture::new();
    let attached = fixture.group().scopes[0].scope_key.clone();
    fixture.mutate(|doc| doc.actors[0].default_scope_key = "scope_detached".into());
    let before = fixture.group();

    assert_eq!(fixture.refused(idle_terminal(false)), "scope_not_attached");

    fixture.mutate(|doc| doc.actors[0].default_scope_key = attached);
    std::fs::remove_dir_all(&fixture.workspace).expect("remove workspace");
    assert_eq!(
        fixture.refused(idle_terminal(false)),
        "invalid_project_root"
    );
    let after = fixture.group();
    assert_eq!(after.actors[0].runtime, before.actors[0].runtime);
    assert_eq!(after.actors[0].command, before.actors[0].command);
    assert!(runtime_switches::records(&after).is_empty());
    assert!(fixture.events("actor.update").is_empty());

    // Following the Group's directory, only starting now needs it.
    fixture.mutate(|doc| doc.actors[0].default_scope_key.clear());
    assert_eq!(fixture.refused(idle_terminal(true)), "invalid_project_root");
    let stopped = fixture.switch(idle_terminal(false));
    assert_eq!(stopped["start"], "stopped");
}

#[test]
fn an_unfinished_switch_holds_only_its_member_when_the_group_starts() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));
    let blocked = fixture.block_private_env_cleanup();
    fixture.mutate(|doc| {
        let mut other = Actor::new("y");
        other.runtime = ActorRuntime::Custom;
        other.command = vec!["sh".into(), "-c".into(), "exec sleep 60".into()];
        actors::add(doc, other).expect("second member");
    });
    let held = fixture.switch(idle_terminal(true));
    assert_eq!(held["completion"], "pending");

    let started = fixture.request("group_start", json!({}));

    assert!(started.ok, "{:?}", started.error);
    let group = fixture.group();
    assert!(group.running);
    let running = |id: &str| {
        let actor = actors::find(&group, id).expect("member");
        actor_runtime::actor_is_running(&group, actor)
    };
    assert!(running("y"), "the other member starts");
    assert!(!running("x"), "the held member does not");

    fixture.unblock(&blocked);
    let finished = fixture.request("actor_runtime_switch_complete", json!({"actor_id": "x"}));
    assert!(finished.ok, "{:?}", finished.error);
    assert_eq!(finished.result["start"], "started");
    assert!(running("x"), "Retry completion starts it");
}

#[test]
fn a_held_member_with_an_unreadable_profile_does_not_stop_its_group() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));
    fixture.block_private_env_cleanup();
    let profiles = ProfileStore::new(fixture.home.clone()).expect("profiles");
    let profile = profiles
        .upsert(
            json!({"id": "shared", "name": "Shared", "runtime": "codex", "command": ["codex"]})
                .as_object()
                .cloned()
                .expect("profile"),
            None,
        )
        .expect("profile");
    let held = fixture.switch(json!({
        "kind": "managed",
        "profile_id": "shared",
        "expected_profile_revision": profile["revision"],
        "start": true,
    }));
    assert_eq!(held["completion"], "pending");
    std::fs::remove_file(
        fixture
            .home
            .root()
            .join("state/actor_profiles/profiles.json"),
    )
    .expect("lose the Profile");

    let started = fixture.request("group_start", json!({}));

    assert!(started.ok, "{:?}", started.error);
}

#[test]
fn removing_a_member_removes_its_switch_records() {
    let fixture = Fixture::new();
    let first = fixture.source("a", true);
    let second = fixture.source("b", true);
    fixture.switch(native_target(&first));
    fixture.switch(native_target(&second));
    assert_eq!(runtime_switches::records(&fixture.group()).len(), 1);

    let removed = fixture.request("actor_remove", json!({"actor_id": "x"}));

    assert!(removed.ok, "{:?}", removed.error);
    assert!(runtime_switches::records(&fixture.group()).is_empty());
}

#[test]
fn entering_a_managed_runtime_removes_values_left_by_an_earlier_configuration() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));
    fixture.set_private();

    fixture.switch(idle_terminal(false));

    assert!(fixture.private_keys().is_empty());
}

#[test]
fn a_new_runtime_that_cannot_start_still_counts_as_switched() {
    let fixture = Fixture::new();

    let result = fixture.switch(json!({
        "kind": "managed",
        "patch": {"runtime": "custom", "command": ["/nonexistent/cccc-switch-fixture"]},
        "start": true,
    }));

    assert_eq!(result["switched"], true);
    assert_eq!(result["start"], "error", "{result}");
    assert!(result["start_error"]["message"].is_string(), "{result}");
    assert_eq!(fixture.actor().runtime, ActorRuntime::Custom);
}

#[test]
fn a_stopped_member_switched_to_a_native_session_stays_paused() {
    let fixture = Fixture::new();
    fixture.mutate(|doc| doc.actors[0].enabled = false);
    let source = fixture.source("a", true);

    let result = fixture.switch(native_target(&source));

    assert_eq!(result["start"], "native");
    assert_eq!(result["access_paused"], true);
    assert!(!fixture.actor().enabled);
}

#[test]
fn a_session_that_changes_before_the_commit_is_not_bound() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    let group = fixture.group();
    let current = fixture.actor();
    let plan = plan_native(
        &fixture.home,
        &group,
        &current,
        &source.source_id,
        &source.native_session_id,
    )
    .expect("plan");
    let mut moved = source.clone();
    moved.native_session_id = "thread-other".into();
    native::write(&fixture.home, moved).expect("the session moves to another conversation");
    let record = SwitchRecord {
        op_id: "op".into(),
        actor_id: "x".into(),
        actor_generation: actors::generation_identity(&current),
        from: Endpoint::of(&current),
        to: Endpoint::of(&plan.actor),
        at: utc_now(),
        pending: true,
        clear_private_env: plan.clear_private_env,
    };

    let rejected = commit(
        &fixture.home,
        &fixture.store(),
        &fixture.group_id,
        &current,
        &plan,
        &record,
    )
    .expect("lock")
    .expect_err("not committed");

    assert_eq!(rejected.code, "local_session_source_changed");
    assert_eq!(fixture.actor(), current);
    assert!(runtime_switches::records(&fixture.group()).is_empty());
}

#[test]
fn held_work_follows_the_handoff_once_the_switch_completes() {
    let fixture = Fixture::new();
    let source = fixture.claude_source();
    let attached = fixture.request("local_session_receiver_attach", Fixture::receiver(&source));
    assert!(attached.ok, "{:?}", attached.error);
    let blocked = fixture.block_private_env_cleanup();
    assert_eq!(fixture.switch(native_target(&source))["start"], "held");
    // A message dispatched from a snapshot taken before the switch.
    let mut message = Event::new("chat.message", &fixture.group_id);
    message.by = "user".into();
    message.data.insert("text".into(), json!("held message"));
    message.data.insert("to".into(), json!(["x"]));
    message.data.insert("message_mode".into(), json!("send"));
    let ledger_path = fixture
        .store()
        .ledger_path(&fixture.group_id)
        .expect("ledger");
    ledger::append(&ledger_path, &message).expect("message");
    let mut stale = fixture.group();
    runtime_switches::set_records(&mut stale, &[]);
    let mut jobs = vec![actor_delivery::DeliveryJob {
        home: fixture.home.clone(),
        group: stale,
        actor: fixture.actor(),
        event: message.clone(),
        claimed: false,
        transport: "pty",
    }];

    let outcome = actor_delivery_worker::process_batch_tracked(
        &mut jobs,
        &mut String::new(),
        &AtomicBool::new(false),
        &mut HashSet::new(),
        &mut actor_delivery_worker::WakeBudget::default(),
    );

    assert!(matches!(outcome, BatchOutcome::Delivered) && jobs.is_empty());
    assert!(
        fixture.next_handoff(&source, Duration::ZERO).is_none(),
        "nothing reaches the new owner before its handoff"
    );
    fixture.unblock(&blocked);
    let done = fixture.request("actor_runtime_switch_complete", json!({"actor_id": "x"}));
    assert!(done.ok, "{:?}", done.error);
    let first = fixture
        .next_handoff(&source, Duration::from_secs(5))
        .expect("the handoff");
    assert_eq!(first["event_ids"], json!([done.result["notice_event_id"]]));
    let mut ack = Fixture::receiver(&source);
    ack["handoff_id"] = first["handoff_id"].clone();
    ack["state"] = json!("accepted");
    let acknowledged = fixture.request("local_session_receiver_ack", ack);
    assert!(acknowledged.ok, "{:?}", acknowledged.error);
    let second = fixture
        .next_handoff(&source, Duration::from_secs(5))
        .expect("the held message");
    assert_eq!(second["event_ids"], json!([message.id]));
}

#[test]
fn members_saved_without_an_update_time_can_switch() {
    let fixture = Fixture::new();
    fixture.mutate(|doc| doc.actors[0].updated_at.clear());

    let result = fixture.switch(idle_terminal(false));

    assert_eq!(result["switched"], true);
}

#[test]
fn an_explicit_start_starts_a_stopped_member_as_save_and_restart_did() {
    let fixture = Fixture::new();
    fixture.mutate(|doc| doc.actors[0].enabled = false);

    let result = fixture.switch(idle_terminal(true));

    assert_eq!(result["start"], "started", "{result}");
    assert!(fixture.actor().enabled);
    assert!(fixture.group().running);
}

#[test]
fn a_paused_native_member_stays_paused_on_a_managed_target() {
    let fixture = Fixture::new();
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));
    fixture.mutate(|doc| doc.actors[0].enabled = false);

    let result = fixture.switch(idle_terminal(true));

    assert_eq!(
        result["start"], "stopped",
        "paused access is resumed only by the user"
    );
    assert!(!fixture.actor().enabled);
}

#[test]
fn a_submission_cut_off_by_the_switch_is_not_repeated_and_queued_work_moves_on() {
    let fixture = Fixture::new();
    let command = terminal_that_stops_reading(&fixture);
    fixture.mutate(|doc| {
        doc.running = true;
        let actor = &mut doc.actors[0];
        actor.runtime = ActorRuntime::Custom;
        actor.command.clone_from(&command);
        actor.env.clear();
    });
    let source = fixture.claude_source();
    let attached = fixture.request("local_session_receiver_attach", Fixture::receiver(&source));
    assert!(attached.ok, "{:?}", attached.error);
    cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: fixture.group_id.clone(),
        actor_id: "x".into(),
        runner: cccc_contracts::RunnerKind::Pty,
        command,
        cwd: fixture.workspace.clone(),
        env: BTreeMap::new(),
        cols: 120,
        rows: 40,
    })
    .expect("managed owner");
    let cut = a_write_cut_part_way(&fixture);
    let group = fixture.group();
    let actor = fixture.actor();
    let queued = fixture.send(&group, &actor, "queued behind the blocked write".into());

    receiver::while_connected(&fixture, &source, || {
        let result = fixture.switch(native_target(&source));

        assert_eq!(result["completion"], "done");
        fixture.assert_cut_off_work_moves_on(&source, &cut, &queued);
    });
}

/// A program that reads into the large message of `a_write_cut_part_way`, says
/// so, and reads no more: the rest of that write blocks.
fn terminal_that_stops_reading(fixture: &Fixture) -> Vec<String> {
    let script = format!(
        "stty -icanon -echo; head -c 65536 | grep -q {} && : > '{}'; exec sleep 60",
        "x".repeat(64),
        fixture.workspace.join("stalled").display()
    );
    vec!["sh".to_owned(), "-c".to_owned(), script]
}

/// A large message whose first bytes have reached the program.
fn a_write_cut_part_way(fixture: &Fixture) -> Event {
    let group = fixture.group();
    let actor = fixture.actor();
    // The first delivery writes the terminal's preamble.
    let warm = fixture.send(&group, &actor, "first".into());
    fixture.wait_for_delivery(&warm, "accepted");
    let cut = fixture.send(&group, &actor, "x".repeat(512 * 1024));
    let stalled = fixture.workspace.join("stalled");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !stalled.exists() {
        assert!(
            Instant::now() < deadline,
            "the write never reached the terminal"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    cut
}

fn terminal_with_a_blocked_write(fixture: &Fixture) -> Event {
    let command = terminal_that_stops_reading(fixture);
    fixture.mutate(|doc| {
        doc.running = true;
        let actor = &mut doc.actors[0];
        actor.runtime = ActorRuntime::Custom;
        actor.command.clone_from(&command);
        actor.env.clear();
    });
    let started = fixture.request("actor_start", json!({"actor_id": "x"}));
    assert!(started.ok, "{:?}", started.error);
    a_write_cut_part_way(fixture)
}

#[test]
fn a_submission_cut_off_by_a_restart_is_not_repeated() {
    let fixture = Fixture::new();
    let cut = terminal_with_a_blocked_write(&fixture);

    let restarted = fixture.request("actor_restart", json!({"actor_id": "x"}));

    assert!(restarted.ok, "{:?}", restarted.error);
    assert_eq!(fixture.delivery_state(&cut).as_deref(), Some("ambiguous"));
    let stopped = fixture.request("actor_stop", json!({"actor_id": "x"}));
    assert!(stopped.ok, "{:?}", stopped.error);
}

#[test]
fn a_submission_cut_off_by_a_stop_stays_out_of_the_queue() {
    let fixture = Fixture::new();
    let cut = terminal_with_a_blocked_write(&fixture);

    let stopped = fixture.request("actor_stop", json!({"actor_id": "x"}));

    assert!(stopped.ok, "{:?}", stopped.error);
    assert_eq!(fixture.delivery_state(&cut).as_deref(), Some("ambiguous"));
    assert!(!fixture.pending().contains(&cut.id));
}

/// A DeepSeek member that took the returned message; `answer` is its provider's
/// shell reply to the prompt.
fn deepseek_prompted(fixture: &Fixture, answer: &str) -> Event {
    let prompted = fixture.workspace.join("prompted");
    let script = format!(
        r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"fake"}}}}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"fake-session"}}}}'
elif printf '%s' "$line" | grep -q '"method":"session/prompt"'; then
  : > '{}'
  {answer}
fi
done"#,
        prompted.display()
    );
    fixture.mutate(|doc| {
        doc.running = true;
        let actor = &mut doc.actors[0];
        actor.runtime = ActorRuntime::Deepseek;
        actor.command = vec!["sh".into(), "-c".into(), script];
        actor.env.clear();
        actor.normalize_runtime_constraints();
    });
    let group = fixture.group();
    let actor = fixture.actor();
    crate::ops::deepseek_runtime::start(&fixture.home, &group, &actor, &fixture.workspace)
        .expect("fake ACP");
    let cut = fixture.send(&group, &actor, "cut off".into());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !prompted.exists() {
        assert!(Instant::now() < deadline, "the prompt never arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
    cut
}

/// A provider that keeps the turn open for the test to cut off.
fn deepseek_with_an_open_turn(fixture: &Fixture) -> Event {
    deepseek_prompted(fixture, ":")
}

#[test]
fn a_deepseek_turn_cut_off_by_the_switch_is_not_repeated() {
    let fixture = Fixture::new();
    let source = fixture.claude_source();
    let attached = fixture.request("local_session_receiver_attach", Fixture::receiver(&source));
    assert!(attached.ok, "{:?}", attached.error);
    let cut = deepseek_with_an_open_turn(&fixture);
    let group = fixture.group();
    let actor = fixture.actor();
    let queued = fixture.send(&group, &actor, "queued behind the open turn".into());

    let result = fixture.switch(native_target(&source));

    assert_eq!(result["completion"], "done");
    fixture.assert_cut_off_work_moves_on(&source, &cut, &queued);
}

/// What a restart does to the delivery worker, without the provider setup its
/// launch needs.
fn stop_worker_for_a_restart(fixture: &Fixture) {
    let reason = crate::ops::actors::delivery_shutdown_reason(
        &fixture.home,
        &fixture.group_id,
        &fixture.actor(),
        "actor.restart",
    )
    .expect("reason");
    crate::ops::actor_delivery::shutdown_actor_for(&fixture.group_id, "x", reason);
    crate::ops::deepseek_runtime::stop(&fixture.group_id, "x");
}

#[test]
fn a_deepseek_turn_cut_off_by_a_restart_stays_undelivered() {
    let fixture = Fixture::new();
    let cut = deepseek_with_an_open_turn(&fixture);

    stop_worker_for_a_restart(&fixture);

    assert_ne!(fixture.delivery_state(&cut).as_deref(), Some("ambiguous"));
    assert!(fixture.pending().contains(&cut.id));
}

#[test]
fn a_deepseek_turn_that_asks_for_a_restart_stays_undelivered() {
    let fixture = Fixture::new();
    let cut = deepseek_prompted(
        &fixture,
        r#"rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"This model request failed","data":"maximum context length is 1048576 tokens"}}\n' "${rid:-3}""#,
    );
    let group = fixture.group();
    let actor = fixture.actor();
    let deadline = Instant::now() + Duration::from_secs(5);
    // The gate is written before the owner stops running.
    while !crate::ops::deepseek_runtime::manual_restart_required(&fixture.home, &group, &actor)
        || crate::ops::deepseek_runtime::running(&fixture.group_id, "x")
    {
        assert!(Instant::now() < deadline, "the provider never failed");
        std::thread::sleep(Duration::from_millis(10));
    }

    stop_worker_for_a_restart(&fixture);

    assert_ne!(fixture.delivery_state(&cut).as_deref(), Some("ambiguous"));
    assert!(fixture.pending().contains(&cut.id));
}

#[test]
fn a_deepseek_turn_cut_off_by_a_restart_onto_another_runtime_is_not_repeated() {
    let fixture = Fixture::new();
    let cut = deepseek_with_an_open_turn(&fixture);
    fixture.mutate(|doc| doc.actors[0].runtime = ActorRuntime::Custom);

    stop_worker_for_a_restart(&fixture);

    assert_eq!(fixture.delivery_state(&cut).as_deref(), Some("ambiguous"));
    assert!(!fixture.pending().contains(&cut.id));
}
