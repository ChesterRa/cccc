//! A Web Model has no process to stop: what ends in a switch is its turn and its
//! route. These run through real dispatch; no browser or provider is involved.
use super::*;
use cccc_core::{integration_state, web_model_connectors as connectors};

const BROWSER: &str = "web_model_browser";

/// On Unix, a child spawned by a concurrently running test can keep an
/// inherited copy of the lock's descriptor until it execs, and the flock
/// with it, so a released lock can stay busy until that copy closes.
fn after_browser_work(mut attempt: impl FnMut() -> DaemonResponse) -> DaemonResponse {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = attempt();
        let busy = response
            .error
            .as_ref()
            .is_some_and(|error| error.code == "actor_browser_busy");
        if !busy || Instant::now() >= deadline {
            return response;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Fixture {
    /// `x` as a running browser Web Model, ready to claim work.
    fn browser_member(&self) {
        self.mutate(|doc| {
            doc.running = true;
            let actor = &mut doc.actors[0];
            actor.runtime = ActorRuntime::WebModel;
            actor.runner = cccc_contracts::RunnerKind::Headless;
            actor.command.clear();
            actor.env.clear();
            actor.normalize_runtime_constraints();
        });
    }

    fn message(&self, text: &str) -> Event {
        let path = self.store().ledger_path(&self.group_id).expect("ledger");
        let mut message = Event::new("chat.message", &self.group_id);
        message.by = "user".into();
        message.data.insert("text".into(), json!(text));
        message.data.insert("to".into(), json!(["x"]));
        message.data.insert("message_mode".into(), json!("send"));
        ledger::append(&path, &message).expect("message");
        message
    }

    fn wait_next(&self, extra: Value) -> serde_json::Map<String, Value> {
        let mut args = json!({"actor_id": "x", "by": "x", "transport": BROWSER});
        args.as_object_mut()
            .expect("args")
            .extend(extra.as_object().cloned().expect("extra"));
        let response = self.request("runtime_wait_next_turn", args);
        assert!(response.ok, "{:?}", response.error);
        response.result
    }

    fn claim(&self) -> Value {
        let wait = self.wait_next(json!({}));
        assert_eq!(wait["status"], "work_available", "{wait:?}");
        wait["turn"].clone()
    }

    /// The durable mark a browser writes before it submits a claimed turn.
    fn fence(&self, turn: &Value) {
        let store = self.store();
        integration_state::group_update(
            &store,
            &self.group_id,
            "web_model_browser_targets",
            |targets| {
                targets["x"] = json!({
                    "last_delivery_turn_id": turn["turn_id"],
                    "last_delivery_status": "submitting",
                });
                Ok(())
            },
        )
        .expect("target");
    }

    fn record(&self, turn: &Value, state: &str) -> DaemonResponse {
        self.request(
            "web_model_browser_delivery_record",
            json!({
                "actor_id": "x",
                "by": "x",
                "turn_id": turn["turn_id"],
                "event_ids": turn["event_ids"],
                "delivery_id": "delivery",
                "browser_delivery": {"state": state, "detail": "test", "provider": "chatgpt"},
            }),
        )
    }

    fn active_turn(&self) -> String {
        crate::ops::runtime_state::actor_state(&self.home, &self.group_id, "x").expect("state")
            ["active_turn_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    /// The messages waiting for `x`, leaving out the switch's own handoff.
    pub(super) fn pending(&self) -> Vec<String> {
        let group = self.group();
        let actor = actors::find(&group, "x").expect("member");
        runtime_delivery::pending_sources(&self.home, &group, actor, 20)
            .expect("pending")
            .into_iter()
            .filter(|event| event.kind == "chat.message")
            .map(|event| event.id)
            .collect()
    }

    fn transport(&self, event: &Event) -> String {
        runtime_delivery::latest_state(&self.home, &self.group_id, "x", &event.id)
            .expect("state")
            .expect("delivery")
            .1
    }

    /// A pairing bound for the member's current generation.
    fn bound_route(&self) {
        connectors::configure_provider(&self.home, "chatgpt_web").expect("shared");
        let shared = connectors::load(&self.home).expect("store");
        let id = shared[0]["connector_id"].as_str().expect("id").to_owned();
        let generation = actors::generation_identity(&self.actor());
        let pairing =
            connectors::begin_pairing(&self.home, &id, &self.group_id, "x", &generation, false)
                .expect("pairing");
        connectors::accept_pairing(
            &self.home,
            &id,
            pairing["code"].as_str().expect("code"),
            "session",
        )
        .expect("accepted");
        connectors::confirm_pairing(
            &self.home,
            &id,
            &self.group_id,
            "x",
            &generation,
            pairing["pairing_id"].as_str().expect("pairing id"),
            "https://chatgpt.com/c/old-conversation",
        )
        .expect("bound");
    }

    fn has_route(&self) -> bool {
        connectors::load(&self.home)
            .expect("store")
            .iter()
            .any(|c| {
                c["bindings"]
                    .as_object()
                    .is_some_and(|bindings| bindings.values().any(|b| b["actor_id"] == "x"))
            })
    }
}

fn to_codex() -> Value {
    json!({"kind": "managed", "start": false, "patch": {"runtime": "codex"}})
}

#[test]
fn work_a_browser_never_marked_for_submission_goes_back_to_the_queue() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let message = fixture.message("claimed, never submitted");
    fixture.claim();

    fixture.switch(to_codex());

    assert_eq!(fixture.active_turn(), "");
    assert_eq!(fixture.delivery_state(&message).as_deref(), Some("failed"));
    assert_eq!(fixture.transport(&message), BROWSER);
    assert_eq!(fixture.pending(), [message.id]);
}

#[test]
fn work_a_browser_may_have_submitted_is_left_for_review() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let message = fixture.message("past the fence");
    let turn = fixture.claim();
    fixture.fence(&turn);

    fixture.switch(to_codex());

    assert_eq!(fixture.active_turn(), "");
    assert_eq!(
        fixture.delivery_state(&message).as_deref(),
        Some("ambiguous")
    );
    assert_eq!(fixture.transport(&message), BROWSER);
    assert!(fixture.pending().is_empty());
}

#[test]
fn a_turn_handed_over_at_its_claim_is_left_for_review() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let message = fixture.message("pulled");
    let wait = fixture.wait_next(json!({"transport": "web_model_pull"}));
    assert_eq!(wait["status"], "work_available", "{wait:?}");

    fixture.switch(to_codex());

    assert_eq!(fixture.active_turn(), "");
    assert_eq!(
        fixture.delivery_state(&message).as_deref(),
        Some("ambiguous")
    );
    assert!(fixture.pending().is_empty());
}

#[test]
fn a_report_arriving_after_the_switch_changes_nothing_it_should_not() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let message = fixture.message("past the fence");
    let turn = fixture.claim();
    fixture.fence(&turn);
    fixture.switch(to_codex());

    assert_eq!(
        code(fixture.record(&turn, "submitted")),
        "invalid_actor_runtime"
    );
    let completion = fixture.request(
        "runtime_complete_turn",
        json!({
            "actor_id": "x",
            "by": "x",
            "turn_id": turn["turn_id"],
            "event_ids": turn["event_ids"],
            "status": "done",
        }),
    );
    assert!(!completion.ok);
    assert_eq!(
        fixture.delivery_state(&message).as_deref(),
        Some("ambiguous")
    );
    assert!(fixture.pending().is_empty());
}

#[test]
fn a_late_failure_does_not_return_uncertain_work_to_the_queue() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let message = fixture.message("staged");
    let turn = fixture.claim();
    assert!(fixture.record(&turn, "ambiguous").ok);

    let late = fixture.record(&turn, "failed");

    assert!(late.ok, "{:?}", late.error);
    assert_eq!(
        fixture.delivery_state(&message).as_deref(),
        Some("ambiguous")
    );
    assert!(fixture.pending().is_empty());
}

#[test]
fn a_switch_answers_busy_while_browser_work_runs_and_changes_nothing() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let message = fixture.message("in flight");
    fixture.claim();
    let before = fixture.actor();
    let lock = connectors::browser_work_lock(&fixture.home, &fixture.group_id, "x");
    let work = cccc_core::fs::try_exclusive_lock(&lock)
        .expect("lock")
        .expect("free");

    assert_eq!(fixture.refused(to_codex()), "actor_browser_busy");

    assert_eq!(fixture.actor(), before);
    assert!(!fixture.active_turn().is_empty());
    assert_eq!(fixture.delivery_state(&message).as_deref(), Some("claimed"));
    drop(work);
    let switched = after_browser_work(|| fixture.try_switch(to_codex(), json!({})));
    assert!(switched.ok, "{:?}", switched.error);
    assert_eq!(fixture.actor().runtime, ActorRuntime::Codex);
}

#[test]
fn removing_a_member_or_its_group_waits_for_browser_work() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let lock = connectors::browser_work_lock(&fixture.home, &fixture.group_id, "x");
    let work = cccc_core::fs::try_exclusive_lock(&lock)
        .expect("lock")
        .expect("free");
    let group_id = fixture.group_id.clone();

    for (op, args) in [
        ("actor_remove", json!({"actor_id": "x"})),
        ("group_delete", json!({})),
        ("group_reset", json!({"confirm": group_id})),
    ] {
        assert_eq!(
            code(fixture.request(op, args)),
            "actor_browser_busy",
            "{op}"
        );
    }
    assert_eq!(fixture.actor().runtime, ActorRuntime::WebModel);

    drop(work);
    let deleted = after_browser_work(|| fixture.request("group_delete", json!({})));
    assert!(deleted.ok, "{:?}", deleted.error);
    assert!(!lock.parent().expect("locks of the Group").exists());
}

#[test]
fn leaving_a_web_model_retires_its_route_and_its_target() {
    let fixture = Fixture::new();
    fixture.browser_member();
    fixture.bound_route();
    let turn = fixture.message("for the old route");
    let claimed = fixture.claim();
    fixture.fence(&claimed);
    assert!(fixture.has_route());

    let result = fixture.switch(to_codex());

    assert_eq!(result["completion"], "done");
    assert!(!fixture.has_route());
    let targets = fixture.group().extra["web_model_browser_targets"].clone();
    assert!(targets.get("x").is_none(), "{targets}");
    assert_eq!(fixture.delivery_state(&turn).as_deref(), Some("ambiguous"));
}

#[test]
fn entering_a_web_model_starts_nothing_and_waits_for_its_connection() {
    let fixture = Fixture::new();
    fixture.mutate(|doc| doc.running = true);
    let result = fixture.switch(json!({"kind": "managed", "patch": {"runtime": "web_model"}}));

    assert_eq!(result["completion"], "done");
    assert_eq!(result["start"], "started", "{result}");
    let actor = fixture.actor();
    assert_eq!(actor.runtime, ActorRuntime::WebModel);
    assert!(actor.enabled);
    assert!(actor.command.is_empty());
    assert!(
        crate::ops::actor_runtime::delivery_owner(&fixture.group_id, "x")
            .expect("owner")
            .is_none()
    );
    let message = fixture.message("before it is connected");
    actor_delivery::dispatch(&fixture.home, &fixture.group(), &message);
    assert_eq!(fixture.delivery_state(&message), None);
    assert_eq!(fixture.pending(), std::slice::from_ref(&message.id));
    // The handoff waits with it, ahead of it.
    let handoff = fixture.notices().pop().expect("handoff");
    assert_eq!(fixture.delivery_state(&handoff), None);
    let turn = fixture.claim();
    assert_eq!(turn["event_ids"], json!([handoff.id, message.id]));
}

#[test]
fn a_native_member_goes_to_a_web_model_and_back() {
    let fixture = Fixture::new();
    fixture.mutate(|doc| doc.running = true);
    let source = fixture.source("a", true);
    fixture.switch(native_target(&source));

    let to_web = fixture.switch(json!({"kind": "managed", "patch": {"runtime": "web_model"}}));

    assert_eq!(to_web["completion"], "done");
    let actor = fixture.actor();
    assert_eq!(actor.runtime, ActorRuntime::WebModel);
    assert!(actor.external_session_id.is_empty());

    let back = fixture.switch(native_target(&source));

    assert_eq!(back["completion"], "done");
    let actor = fixture.actor();
    assert_eq!(actor.external_session_id, source.source_id);
    assert_eq!(fixture.notices().len(), 3);
}

#[test]
fn a_profile_can_carry_a_member_into_a_web_model() {
    let fixture = Fixture::new();
    fixture.mutate(|doc| doc.running = true);
    let saved = ProfileStore::new(fixture.home.clone())
        .expect("profiles")
        .upsert(
            json!({"id": "browser", "name": "Browser", "runtime": "web_model"})
                .as_object()
                .cloned()
                .expect("profile"),
            None,
        )
        .expect("profile");

    let result = fixture.switch(json!({
        "kind": "managed",
        "profile_id": "browser",
        "expected_profile_revision": saved["revision"],
    }));

    assert_eq!(result["completion"], "done");
    let actor = fixture.actor();
    assert_eq!(actor.runtime, ActorRuntime::WebModel);
    assert_eq!(actor.profile_id, "browser");
}

#[test]
fn a_failed_switch_does_not_call_a_web_model_stopped() {
    let report = |was_running: bool| {
        let fixture = Fixture::new();
        fixture.browser_member();
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
        incomplete(
            &fixture.home,
            &request,
            &fixture.actor(),
            Some(was_running),
            hold,
            OpError::new("local_session_offline", "the session went away"),
        )
        .details["old_owner"]
            .clone()
    };

    assert_eq!(report(false), "web_model");
    // A terminal that ran under the saved Web Model is an owner like any other.
    assert_eq!(report(true), "restarted");
}

#[test]
fn a_connection_made_before_the_switch_finishes_would_be_retired_so_it_waits() {
    let fixture = Fixture::new();
    let hold = |runtime: ActorRuntime| {
        fixture.mutate(|doc| {
            let before = doc.actors[0].clone();
            let actor = &mut doc.actors[0];
            actor.runtime = runtime;
            actor.runner = cccc_contracts::RunnerKind::Headless;
            actor.command.clear();
            actor.env.clear();
            actor.normalize_runtime_constraints();
            let record = SwitchRecord {
                op_id: "op".into(),
                actor_id: "x".into(),
                actor_generation: actors::generation_identity(actor),
                from: Endpoint::of(&before),
                to: Endpoint::of(actor),
                at: utc_now(),
                pending: true,
                clear_private_env: false,
            };
            runtime_switches::set_records(doc, &[record]);
        });
    };
    let args = json!({"actor_id": "x", "connector_id": "shared", "url": "https://grok.com/"});

    hold(ActorRuntime::GrokWebModel);
    for op in [
        "web_model_grok_bind",
        "web_model_pairing_begin",
        "grok_webhook_configure",
    ] {
        assert_eq!(
            code(fixture.request(op, args.clone())),
            "actor_switch_pending",
            "{op}"
        );
    }
    hold(ActorRuntime::ChatgptDots);
    assert_eq!(
        code(fixture.request("dots_events_connect", args)),
        "actor_switch_pending"
    );
}

#[test]
fn a_save_cannot_take_the_route_from_a_turn_still_using_it() {
    let fixture = Fixture::new();
    fixture.browser_member();
    fixture.bound_route();
    fixture.message("in progress");
    let turn = fixture.claim();
    let update = json!({"actor_id": "x", "patch": {"runtime": "codex"}});

    assert_eq!(
        code(fixture.request("actor_update", update.clone())),
        "delivery_unresolved"
    );
    assert_eq!(fixture.actor().runtime, ActorRuntime::WebModel);
    assert!(fixture.has_route());

    assert!(fixture.record(&turn, "submitted").ok);
    let completion = fixture.request(
        "runtime_complete_turn",
        json!({
            "actor_id": "x",
            "by": "x",
            "turn_id": turn["turn_id"],
            "event_ids": turn["event_ids"],
            "status": "done",
            "delivery_id": "delivery",
        }),
    );
    assert!(completion.ok, "{:?}", completion.error);
    let saved = fixture.request("actor_update", update);
    assert!(saved.ok, "{:?}", saved.error);
    assert!(!fixture.has_route());
}

#[test]
fn a_staged_batch_comes_back_whole_whatever_the_request_or_the_queue_says() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let first = fixture.message("first");
    let second = fixture.message("second");
    let turn = fixture.claim();
    fixture.message("arrived after the claim");
    let resume = json!({"limit": 1, "resume_turn_id": turn["turn_id"]});

    let resumed = fixture.wait_next(resume.clone());

    assert_eq!(resumed["status"], "work_available", "{resumed:?}");
    assert_eq!(resumed["turn"]["turn_id"], turn["turn_id"]);
    assert_eq!(resumed["turn"]["event_ids"], json!([first.id, second.id]));
    assert_eq!(
        fixture.wait_next(json!({"resume_turn_id": "another"}))["status"],
        "turn_in_progress"
    );
    // Only the browser that staged it takes it back; a pull would claim its draft.
    assert_eq!(
        fixture.wait_next(json!({
            "transport": "web_model_pull", "resume_turn_id": turn["turn_id"]
        }))["status"],
        "turn_in_progress"
    );

    // Once part of it has left the claim, this is no longer the batch staged.
    runtime_delivery::append_state(
        &fixture.home,
        &fixture.group_id,
        "x",
        &fixture.actor().created_at,
        &first.id,
        BROWSER,
        DeliveryOutcome::Ambiguous("settled"),
    )
    .expect("settled");
    assert_eq!(fixture.wait_next(resume)["status"], "turn_in_progress");
    assert_eq!(fixture.active_turn(), turn["turn_id"].as_str().expect("id"));
}

#[test]
fn a_settled_browser_turn_is_reported_for_review_without_releasing_it() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let first = fixture.message("first");
    let second = fixture.message("second");
    let turn = fixture.claim();
    let waiting = fixture.wait_next(json!({}));
    assert_eq!(waiting["status"], "turn_in_progress");
    assert!(waiting.get("browser_recovery_required").is_none());

    // The daemon settles claims on restart, including a claim made before
    // the browser could write a target fence.
    assert_eq!(
        runtime_delivery::settle_stranded_claims(&fixture.home, &fixture.group()).expect("settle"),
        2
    );
    let waiting = fixture.wait_next(json!({}));
    assert_eq!(waiting["status"], "turn_in_progress");
    assert_eq!(waiting["browser_recovery_required"], true);
    assert_eq!(waiting["active_turn_id"], turn["turn_id"]);
    assert_eq!(waiting["event_ids"], turn["event_ids"]);
    assert_eq!(
        fixture.active_turn(),
        turn["turn_id"].as_str().expect("turn")
    );

    // A pull caller cannot use the browser's recovery path.
    let pull = fixture.wait_next(json!({"transport": "web_model_pull"}));
    assert_eq!(pull["status"], "turn_in_progress");
    assert!(pull.get("browser_recovery_required").is_none());

    // A failed browser receipt from an older version also left this turn held.
    let append = |event: &Event, transport: &str, outcome| {
        runtime_delivery::append_state(
            &fixture.home,
            &fixture.group_id,
            "x",
            &fixture.actor().created_at,
            &event.id,
            transport,
            outcome,
        )
        .expect("state");
    };
    append(&second, BROWSER, DeliveryOutcome::Failed("legacy failure"));
    assert_eq!(
        fixture.wait_next(json!({}))["browser_recovery_required"],
        true
    );

    // Evidence of acceptance or a different transport never authorizes this
    // recovery, even if another event in the batch is ambiguous.
    append(&first, BROWSER, DeliveryOutcome::Accepted);
    assert!(
        fixture
            .wait_next(json!({}))
            .get("browser_recovery_required")
            .is_none()
    );
    append(&first, BROWSER, DeliveryOutcome::Ambiguous("restart"));
    append(
        &second,
        "web_model_pull",
        DeliveryOutcome::Failed("pull failure"),
    );
    assert!(
        fixture
            .wait_next(json!({}))
            .get("browser_recovery_required")
            .is_none()
    );
    assert_eq!(
        fixture.active_turn(),
        turn["turn_id"].as_str().expect("turn")
    );
}

#[test]
fn a_failed_batch_stops_holding_up_the_work_behind_it() {
    let fixture = Fixture::new();
    fixture.browser_member();
    let failed = fixture.message("conversation unavailable");
    let turn = fixture.claim();
    let behind = fixture.message("behind it");

    assert!(fixture.record(&turn, "failed").ok);

    assert_eq!(fixture.active_turn(), "");
    let next = fixture.claim();
    assert_eq!(next["event_ids"], json!([failed.id, behind.id]));
}
