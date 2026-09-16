use super::*;
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, Event};
use cccc_core::context::{ContextDoc, ContextStore};
use cccc_core::{GroupStore, HomeLayout, actors, inbox, ledger};
use chrono::{Duration, Utc};
use serde_json::{Map, Value, json};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
}

impl Fixture {
    fn new(title: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("group store");
        let mut group = store.create(title, "").expect("group");
        let mut lead = Actor::new("web-lead");
        lead.runtime = ActorRuntime::WebModel;
        actors::add(&mut group, lead).expect("web lead");
        actors::add(&mut group, Actor::new("worker-a")).expect("worker a");
        actors::add(&mut group, Actor::new("worker-b")).expect("worker b");
        group.running = true;
        store.save(&group).expect("save group");
        Self {
            _temp: temp,
            home,
            group,
        }
    }

    fn path(&self) -> std::path::PathBuf {
        GroupStore::new(self.home.clone())
            .expect("store")
            .ledger_path(&self.group.group_id)
            .expect("ledger path")
    }

    fn context(&self) -> ContextDoc {
        ContextStore::new(self.home.clone())
            .expect("context store")
            .load(&self.group.group_id)
            .expect("context")
    }

    fn sync(&self, by: &str, op: Value) -> ContextDoc {
        ContextStore::new(self.home.clone())
            .expect("context store")
            .sync(
                &self.group.group_id,
                &[op.as_object().cloned().expect("op object")],
                None,
                by,
                false,
            )
            .expect("context sync")
            .context
    }

    fn task(&self, title: &str, assignee: &str) -> String {
        let context = self.sync(
            "web-lead",
            json!({
                "op":"task.create","title":title,"outcome":format!("Finish {title}"),
                "status":"active","assignee":assignee,"waiting_on":"actor"
            }),
        );
        context.tasks.last().expect("task")["id"]
            .as_str()
            .expect("task id")
            .to_owned()
    }

    fn pause(&mut self) {
        self.group.state = GroupState::Paused;
        GroupStore::new(self.home.clone())
            .expect("store")
            .save(&self.group)
            .expect("pause group");
    }

    fn report(&self, actor_id: &str, task_id: Option<&str>) -> Event {
        let mut refs = Vec::new();
        if let Some(task_id) = task_id {
            refs.push(json!({
                "kind":"task_ref","task_id":task_id,"title":"Source task",
                "status":"active","waiting_on":"actor"
            }));
        }
        let mut report = Event::new("chat.message", &self.group.group_id);
        report.by = actor_id.into();
        report.data = json!({
            "to":["web-lead"],"message_mode":"mail","text":"Member result","refs":refs
        })
        .as_object()
        .cloned()
        .expect("report data");
        ledger::append(&self.path(), &report).expect("append report");
        report
    }

    fn handoff(&self, report: &Event, turn_id: &str) -> Event {
        record_handoff(
            &self.home,
            &self.group,
            &report.by,
            "web-lead",
            turn_id,
            std::slice::from_ref(report),
            "completed",
        )
        .expect("record handoff")
    }

    fn claim_for_web(&self, event: &Event) {
        let lead = actors::find(&self.group, "web-lead").expect("lead");
        super::super::runtime_delivery::append_state(
            &self.home,
            &self.group.group_id,
            &lead.id,
            &lead.created_at,
            &event.id,
            "web_model_browser",
            super::super::runtime_delivery::DeliveryOutcome::Claimed,
        )
        .expect("claim source");
    }

    fn append_delivery(&self, source_event_id: &str, state: &str, seconds_ago: i64) {
        let mut delivery = Event::new("runtime.delivery", &self.group.group_id);
        delivery.ts = (Utc::now() - Duration::seconds(seconds_ago)).to_rfc3339();
        delivery.by = "system".into();
        delivery.data = json!({
            "actor_id":"web-lead","source_event_id":source_event_id,
            "state":state,"transport":"web_model_browser"
        })
        .as_object()
        .cloned()
        .expect("delivery data");
        ledger::append(&self.path(), &delivery).expect("append delivery");
    }

    fn delivery_state(&self, source_event_id: &str) -> String {
        super::super::runtime_delivery::latest_state(
            &self.home,
            &self.group.group_id,
            "web-lead",
            source_event_id,
        )
        .expect("delivery state")
        .expect("delivery recorded for source")
        .0
    }

    fn pending_sources(&self) -> Vec<Event> {
        super::super::runtime_delivery::pending_sources(
            &self.home,
            &self.group,
            actors::find(&self.group, "web-lead").expect("lead"),
            20,
        )
        .expect("pending sources")
    }

    fn status(&self) -> Map<String, Value> {
        status(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: "coordination_relay_status".into(),
                args: json!({"group_id":self.group.group_id,"actor_id":"web-lead","by":"web-lead"})
                    .as_object()
                    .cloned()
                    .expect("status args"),
            },
        )
        .expect("relay status")
    }

    fn remind(&self, browser_idle: bool) -> Map<String, Value> {
        remind_due(
            &self.home,
            &remind_request(&self.group.group_id, browser_idle),
        )
        .expect("relay reminder check")
    }

    fn decide(&self, value: Value) -> Result<Map<String, Value>, OpError> {
        let mut args = value.as_object().cloned().expect("decision args");
        args.insert("group_id".into(), json!(self.group.group_id));
        args.insert("by".into(), json!("web-lead"));
        decide(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: "coordination_decide".into(),
                args,
            },
        )
    }

    fn notice(&self, handoff: &Event, kind: RelayNotice) -> Event {
        send_relay_notice(
            &self.home,
            &self.group.group_id,
            "web-lead",
            &self.events(),
            std::slice::from_ref(handoff),
            kind,
        )
        .expect("relay notification")
    }

    fn events(&self) -> Vec<Event> {
        ledger::read_all(&self.path()).expect("ledger")
    }
}

fn task<'a>(context: &'a ContextDoc, task_id: &str) -> &'a Map<String, Value> {
    context
        .tasks
        .iter()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(task_id))
        .expect("task exists")
}

fn relay_events<'a>(events: &'a [Event], kind: &str) -> Vec<&'a Event> {
    events.iter().filter(|event| event.kind == kind).collect()
}

fn remind_request(group_id: &str, browser_idle: bool) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: "coordination_relay_remind".into(),
        args: json!({
            "group_id":group_id,"actor_id":"web-lead","by":"web-lead",
            "browser_idle":browser_idle
        })
        .as_object()
        .cloned()
        .expect("reminder request"),
    }
}

fn handoff_id(handoff: &Event) -> &str {
    handoff.data["handoff_id"].as_str().expect("handoff id")
}

fn handoff_note<'a>(context: &'a ContextDoc, handoff_id: &str) -> &'a Value {
    context.coordination["recent_handoffs"]
        .as_array()
        .expect("handoff notes")
        .iter()
        .find(|note| note["id"] == handoff_id)
        .expect("handoff note")
}

fn lead_messages<'a>(events: &'a [Event], to: &str) -> Vec<&'a Event> {
    events
        .iter()
        .filter(|event| {
            event.kind == "chat.message"
                && event.by == "web-lead"
                && event.data["to"] == json!([to])
        })
        .collect()
}

fn relay_notes<'a>(events: &'a [Event], relay_kind: &str) -> Vec<&'a Event> {
    events
        .iter()
        .filter(|event| event.data.get("relay_kind").and_then(Value::as_str) == Some(relay_kind))
        .collect()
}

fn interrupted_continue(
    fixture: &Fixture,
    report: &Event,
    handoff: &Event,
    next_text: &str,
) -> Map<String, Value> {
    let source_event_ids = vec![report.id.clone()];
    let handoff_ids = vec![handoff_id(handoff).to_owned()];
    let decision_id = decision_id(&fixture.group.group_id, "web-lead", &source_event_ids);
    let request = DaemonRequest {
        v: 1,
        op: "coordination_decide".into(),
        args: json!({
            "decision":"continue","summary":"Run the final verification.",
            "next_actor_id":"worker-b","next_title":"Final verification",
            "next_text":next_text
        })
        .as_object()
        .cloned()
        .expect("continue request"),
    };
    let fingerprint = decision_fingerprint(
        "continue",
        "",
        "worker-b",
        "Final verification",
        next_text,
        &request,
    );
    let scope = DecisionScope {
        home: &fixture.home,
        group_id: &fixture.group.group_id,
        actor_id: "web-lead",
        decision_id: &decision_id,
        request_fingerprint: &fingerprint,
        source_event_ids: &source_event_ids,
        handoff_ids: &handoff_ids,
    };
    tracked_continue(
        &scope,
        &request,
        ContinueSpec {
            actor_id: "worker-b",
            title: "Final verification",
            text: next_text,
        },
    )
    .expect("simulated interrupted continue dispatch")
}

#[test]
fn continue_creates_real_work_resolves_the_report_and_replays_without_duplicates() {
    let fixture = Fixture::new("relay continue");
    let source_task = fixture.task("Implement relay", "worker-a");
    let report = fixture.report("worker-a", Some(&source_task));
    fixture.handoff(&report, "turn-continue");
    fixture.claim_for_web(&report);
    assert_eq!(fixture.pending_sources().len(), 1);

    let request = json!({
        "event_ids":[report.id],"decision":"continue",
        "next_actor_id":"worker-b","next_title":"Run the complete regression",
        "next_text":"Run all affected Rust tests and report any regression with exact evidence.",
        "outcome":"All affected tests pass with recorded evidence"
    });
    let first = fixture.decide(request.clone()).expect("continue decision");
    assert_eq!(first["relay"]["decision"], "continue");
    assert_eq!(first["caller_may_idle"], true);
    assert_eq!(first["relay"]["safe_to_idle"], false);
    assert_eq!(first["safe_to_idle"], false);
    assert_eq!(first["current_responsibility"]["kind"], "actor_work");
    let next_task = first["relay"]["next_task_id"]
        .as_str()
        .expect("next task id")
        .to_owned();

    let context = fixture.context();
    assert_eq!(task(&context, &source_task)["status"], "done");
    assert_eq!(task(&context, &next_task)["status"], "active");
    assert_eq!(task(&context, &next_task)["assignee"], "worker-b");
    assert_eq!(task(&context, &next_task)["waiting_on"], "actor");
    let events = fixture.events();
    assert_eq!(relay_events(&events, DECISION_KIND).len(), 1);
    let delegated = lead_messages(&events, "worker-b");
    assert_eq!(
        delegated.len(),
        1,
        "continue must have one visible task message"
    );
    let delegated = delegated[0];
    assert_eq!(
        delegated.data["text"],
        "Run all affected Rust tests and report any regression with exact evidence."
    );
    assert!(
        !delegated.data["text"]
            .as_str()
            .unwrap_or_default()
            .contains("Previous handoff reviewed")
    );
    assert_eq!(fixture.delivery_state(&report.id), "accepted");
    assert!(
        fixture.pending_sources().is_empty(),
        "handled report was still eligible for browser wake"
    );

    let replay = fixture.decide(request).expect("idempotent replay");
    assert_eq!(replay["replayed"], true);
    let context = fixture.context();
    assert_eq!(context.tasks.len(), 2, "replay created another next task");
    assert_eq!(relay_events(&fixture.events(), DECISION_KIND).len(), 1);
    assert_eq!(
        lead_messages(&fixture.events(), "worker-b").len(),
        1,
        "replay duplicated the visible task message"
    );

    let conflict = fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"wait_user"
        }))
        .expect_err("conflicting decision");
    assert_eq!(conflict.code, "relay_decision_conflict");
}

#[test]
fn status_decisions_update_only_their_task_and_never_duplicate_the_report() {
    let blocked_reason = "The provider returns HTTP 503 for the required endpoint.";
    for (decision, reason, task_status, waiting_on, responsibility) in [
        ("wait_user", None, "active", Some("user"), Some("user")),
        (
            "blocked",
            Some(blocked_reason),
            "active",
            Some("external"),
            Some("external"),
        ),
        ("complete", None, "done", None, None),
    ] {
        let fixture = Fixture::new("relay decision states");
        let source_task = fixture.task("Source task", "worker-a");
        let other_task = fixture.task("Other live work", "worker-b");
        let report = fixture.report("worker-a", Some(&source_task));
        fixture.handoff(&report, "turn-states");

        if reason.is_some() {
            let missing = fixture
                .decide(json!({"event_ids":[report.id],"decision":"blocked",
                    "summary":"The external provider is blocking progress."}))
                .expect_err("blocked requires a reason");
            assert_eq!(missing.code, "relay_reason_required");
        }
        let refused = fixture
            .decide(json!({"event_ids":[report.id],"decision":"complete",
                "summary":"The requested work is complete and verified."}))
            .expect_err("other live work must block completion");
        assert_eq!(refused.code, "relay_work_remains");
        assert_eq!(task(&fixture.context(), &source_task)["status"], "active");
        assert!(relay_events(&fixture.events(), DECISION_KIND).is_empty());
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|event| event.kind == "chat.message")
                .count(),
            1,
            "a rejected decision was shown to the user"
        );
        fixture.sync(
            "worker-b",
            json!({"op":"task.move","task_id":other_task,"status":"done"}),
        );

        let mut request = json!({"event_ids":[report.id],"decision":decision});
        if let Some(reason) = reason {
            request["reason"] = json!(reason);
        }
        let result = fixture.decide(request).expect("decision applies");
        assert_eq!(result["relay"]["decision"], decision);
        assert_eq!(result["relay"]["safe_to_idle"], true);
        assert_eq!(result["safe_to_idle"], true);
        if let Some(kind) = responsibility {
            assert_eq!(result["relay"]["responsibility"]["kind"], kind);
        }
        let context = fixture.context();
        let source = task(&context, &source_task);
        assert_eq!(source["status"], task_status);
        if let Some(waiting) = waiting_on {
            assert_eq!(source["waiting_on"], waiting);
        }
        if reason.is_some() {
            assert_eq!(source["notes"], blocked_reason);
        }
        if decision == "complete" {
            assert_eq!(result["relay"]["task_ids"], json!([source_task]));
        }
        let events = fixture.events();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "chat.message")
                .count(),
            1,
            "the decision duplicated the original visible report"
        );
        assert!(lead_messages(&events, "user").is_empty());
    }
}

#[test]
fn task_updates_require_an_explicit_reference_and_preserve_other_members_work() {
    let fixture = Fixture::new("relay task ownership");
    let source_task = fixture.task("Unreferenced source task", "worker-a");
    let unrelated_task = fixture.task("Unrelated task", "worker-b");
    let report = fixture.report("worker-a", None);
    fixture.handoff(&report, "turn-owned-task");

    let error = fixture
        .decide(json!({
            "event_ids":[report.id],"task_id":unrelated_task,"decision":"wait_user",
            "summary":"Do not mutate this unrelated task."
        }))
        .expect_err("unrelated task rejected");
    assert_eq!(error.code, "relay_task_not_owned");
    assert_eq!(
        task(&fixture.context(), &unrelated_task)["status"],
        "active"
    );

    let result = fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"complete",
            "summary":"The one source-owned task is complete."
        }))
        .expect_err("unrelated live task still blocks group completion");
    assert_eq!(result.code, "relay_work_remains");
    assert_eq!(task(&fixture.context(), &source_task)["status"], "active");

    fixture.sync(
        "worker-b",
        json!({"op":"task.move","task_id":unrelated_task,"status":"done"}),
    );
    let result = fixture
        .decide(json!({
            "event_ids":[report.id],"task_id":source_task,"decision":"complete",
            "summary":"The source-owned task is complete by explicit reference."
        }))
        .expect("complete explicitly owned task");
    assert_eq!(result["relay"]["task_ids"], json!([source_task]));
    assert_eq!(task(&fixture.context(), &source_task)["status"], "done");
}

#[test]
fn unreferenced_reports_never_infer_task_ownership_from_a_single_active_task() {
    let fixture = Fixture::new("no task inference");
    let stray_task = fixture.task("Only active task", "worker-a");
    // The report carries no task_ref, its handoff has no task_ids, and there is
    // no reply_to chain to an original assignment.
    let report = fixture.report("worker-a", None);
    fixture.handoff(&report, "turn-no-task-link");

    let result = fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"complete",
            "summary":"Completed the report without touching tasks."
        }))
        .expect_err("a live task still counts as remaining work");
    assert_eq!(result.code, "relay_work_remains");
    assert_eq!(task(&fixture.context(), &stray_task)["status"], "active");

    let result = fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"wait_user",
            "summary":"Waiting on user direction."
        }))
        .expect("wait_user succeeds without task inference");
    assert_eq!(result["relay"]["task_ids"], json!([]));
    assert_eq!(
        task(&fixture.context(), &stray_task)["waiting_on"],
        "actor",
        "an unreferenced task was flipped to waiting_on=user"
    );
    assert_eq!(task(&fixture.context(), &stray_task)["status"], "active");
}

#[test]
fn replays_keep_machine_intent_and_recompute_current_safety() {
    let fixture = Fixture::new("replay intent");
    let first = fixture.report("worker-a", None);
    let early = fixture
        .decide(json!({"event_ids":[first.id],"decision":"wait_user"}))
        .expect("summary is not required");
    assert_eq!(early["relay"]["summary"], "Waiting for user");
    let initial = fixture.events();
    assert_eq!(relay_events(&initial, HANDOFF_KIND).len(), 1);
    assert_eq!(relay_events(&initial, DECISION_KIND).len(), 1);
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|event| event.kind == "chat.message")
            .count(),
        1,
        "the decision duplicated the original visible report"
    );

    let second = fixture.report("worker-a", None);
    fixture
        .decide(json!({"event_ids":[second.id],"decision":"blocked",
            "reason":"The provider returns HTTP 503."}))
        .expect("first blocked decision");
    let decisions_on = |source_id: &str| {
        fixture
            .events()
            .iter()
            .filter(|event| {
                event.kind == DECISION_KIND
                    && event.data["source_event_ids"]
                        .as_array()
                        .is_some_and(|ids| ids.contains(&json!(source_id)))
            })
            .count()
    };
    let changed = fixture
        .decide(json!({"event_ids":[second.id],"decision":"blocked",
            "reason":"The credential is invalid."}))
        .expect_err("a different blocking cause is different machine intent");
    assert_eq!(changed.code, "relay_decision_conflict");
    assert_eq!(decisions_on(&second.id), 1);

    fixture.task("New actor-owned work", "worker-b");
    let replay = fixture
        .decide(json!({"event_ids":[first.id],"decision":"wait_user",
            "summary":"Legacy wording must not change machine intent."}))
        .expect("a worded replay is still a replay");
    assert_eq!(replay["replayed"], true);
    assert_eq!(decisions_on(&first.id), 1);
    assert_eq!(
        replay["relay"]["safe_to_idle"], true,
        "the replay changed the recorded historical fact"
    );
    assert_eq!(
        replay["safe_to_idle"], false,
        "the replay did not recompute the current responsibility"
    );
    assert_eq!(replay["current_responsibility"]["kind"], "actor_work");
}

#[test]
fn status_reconciles_an_interrupted_acceptance_and_never_rewrites_it_twice() {
    let fixture = Fixture::new("decision acceptance recovery");
    let report = fixture.report("worker-a", None);
    let handoff = fixture.handoff(&report, "turn-recovery");
    fixture.claim_for_web(&report);
    let decision_id = "decision-recovery";
    let mut decision = Event::new(DECISION_KIND, &fixture.group.group_id);
    decision.id = stable_event_id(decision_id);
    decision.by = "web-lead".into();
    decision.data = json!({
        "decision_id":decision_id,"by":"web-lead","decision":"wait_user",
        "summary":"Wait for the user's approval.","source_event_ids":[report.id],
        "handoff_ids":[handoff.data["handoff_id"]],"status":"applied",
        "caller_may_idle":true,"safe_to_idle":true
    })
    .as_object()
    .cloned()
    .expect("decision data");
    ledger::append(&fixture.path(), &decision).expect("durable decision");
    assert_eq!(fixture.delivery_state(&report.id), "claimed");
    assert_eq!(fixture.status()["count"], 0);
    assert_eq!(fixture.delivery_state(&report.id), "accepted");

    let accepted = || {
        fixture
            .events()
            .iter()
            .filter(|event| {
                event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == report.id
                    && event.data["state"] == "accepted"
            })
            .count()
    };
    assert_eq!(accepted(), 1);
    for _ in 0..20 {
        fixture.status();
    }
    assert_eq!(accepted(), 1);
}

#[test]
fn reading_mail_is_not_acknowledgement_but_deciding_cancels_later_browser_wake() {
    let fixture = Fixture::new("read then decide");
    let task_id = fixture.task("Review report", "worker-a");
    let report = fixture.report("worker-a", Some(&task_id));
    fixture.handoff(&report, "turn-read");
    fixture.claim_for_web(&report);
    let request = DaemonRequest {
        v: 1,
        op: "runtime_wait_next_turn".into(),
        args: json!({
            "group_id":fixture.group.group_id,
            "actor_id":"web-lead","by":"web-lead","transport":"web_model_browser"
        })
        .as_object()
        .cloned()
        .expect("wait request"),
    };
    let wait = super::super::runtime_state::resolve_operation(&request)
        .expect("wait operation")
        .execute(&fixture.home, &request)
        .expect("work available");
    assert_eq!(wait["status"], "work_available");
    assert_eq!(
        super::super::runtime_state::actor_state(
            &fixture.home,
            &fixture.group.group_id,
            "web-lead",
        )
        .expect("active state")["status"],
        "working"
    );

    let consumed = inbox::consume_unread(&fixture.home, &fixture.group, "web-lead", "web-lead", 20)
        .expect("read mail");
    assert_eq!(
        consumed
            .messages
            .iter()
            .map(|event| &event.id)
            .collect::<Vec<_>>(),
        [&report.id]
    );
    assert_eq!(consumed.read_event.expect("read event").kind, "mail.read");
    assert_eq!(
        fixture
            .pending_sources()
            .iter()
            .map(|event| &event.id)
            .collect::<Vec<_>>(),
        [&report.id],
        "read was incorrectly treated as acknowledgement"
    );

    let browser_attempt = DaemonRequest {
        v: 1,
        op: "web_model_browser_delivery_record".into(),
        args: json!({"group_id":fixture.group.group_id,"actor_id":"web-lead","by":"web-lead",
            "turn_id":wait["turn"]["turn_id"],"event_ids":wait["turn"]["event_ids"],
            "delivery_id":"browser-in-turn","browser_delivery":{"state":"submitting"}})
        .as_object()
        .cloned()
        .expect("browser attempt"),
    };
    super::super::runtime_state::resolve_operation(&browser_attempt)
        .expect("browser delivery operation")
        .execute(&fixture.home, &browser_attempt)
        .expect("browser dispatch recorded before decision");

    fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"wait_user",
            "summary":"I reviewed the report inside the current turn. Please confirm the release window."
        }))
        .expect("decision after read");
    assert!(
        fixture.pending_sources().is_empty(),
        "explicit handling did not suppress the later browser wake"
    );
    assert_eq!(
        super::super::runtime_state::actor_state(
            &fixture.home,
            &fixture.group.group_id,
            "web-lead",
        )
        .expect("released state")["status"],
        "waiting",
        "the claimed browser turn remained stuck after in-turn handling"
    );
    let browser_completion = DaemonRequest {
        v: 1,
        op: "runtime_complete_turn".into(),
        args: json!({"group_id":fixture.group.group_id,"actor_id":"web-lead","by":"web-lead",
            "turn_id":wait["turn"]["turn_id"],"event_ids":wait["turn"]["event_ids"],
            "delivery_id":"browser-in-turn","status":"done"})
        .as_object()
        .cloned()
        .expect("browser completion"),
    };
    let completion = super::super::runtime_state::resolve_operation(&browser_completion)
        .expect("completion operation")
        .execute(&fixture.home, &browser_completion)
        .expect("late browser confirmation reuses the decision's completion");
    assert_eq!(completion["delivery_id"], "browser-in-turn");
}

#[test]
fn handled_reports_stay_handled_across_late_duplicate_and_larger_handoffs() {
    let fixture = Fixture::new("handled report stability");
    let first = fixture.report("worker-a", None);
    fixture
        .decide(json!({"event_ids":[first.id],"decision":"wait_user",
            "summary":"Please review the completed result."}))
        .expect("decision before managed completion event");

    let handoff = fixture.handoff(&first, "real-managed-turn");
    let state = fixture.status();
    assert_eq!(state["count"], 0, "late completion reopened handled output");
    assert_eq!(state["requires_decision"], false);

    let store = ContextStore::new(fixture.home.clone()).expect("context store");
    let version_before = store.version(&fixture.context()).expect("version");
    fixture.handoff(&first, "real-managed-turn");
    let context = fixture.context();
    let version_after = store.version(&context).expect("version");
    assert_eq!(
        version_before, version_after,
        "duplicate completion rewrote context"
    );
    let note = handoff_note(&context, handoff_id(&handoff));
    assert_eq!(note["status"], "resolved");
    assert!(
        note["decision_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );

    let second = fixture.report("worker-a", None);
    record_handoff(
        &fixture.home,
        &fixture.group,
        "worker-a",
        "web-lead",
        "real-multi-output-turn",
        &[first.clone(), second.clone()],
        "completed",
    )
    .expect("actual turn handoff");
    let pending = fixture.status();
    assert_eq!(pending["count"], 1);
    assert_eq!(
        pending["pending"][0]["source_event_ids"],
        json!([second.id.clone()])
    );
    assert_eq!(
        pending["pending"][0]["all_source_event_ids"],
        json!([first.id.clone(), second.id.clone()])
    );
    fixture
        .decide(json!({
            "event_ids":[second.id],"decision":"wait_user",
            "summary":"Please also review the second result."
        }))
        .expect("second output decision");
    assert_eq!(fixture.status()["count"], 0);
}

#[test]
fn one_decision_resolves_a_whole_verbose_member_turn() {
    let fixture = Fixture::new("verbose member handoff");
    let reports = (0..25)
        .map(|_| fixture.report("worker-a", None))
        .collect::<Vec<_>>();
    let handoff = record_handoff(
        &fixture.home,
        &fixture.group,
        "worker-a",
        "web-lead",
        "turn-verbose-output",
        &reports,
        "completed",
    )
    .expect("verbose handoff");
    assert_eq!(
        handoff.data["source_event_ids"]
            .as_array()
            .expect("source ids")
            .len(),
        25
    );
    fixture.claim_for_web(&reports[0]);
    fixture.claim_for_web(&reports[1]);

    let result = fixture
        .decide(json!({
            "event_id":reports[0].id,"decision":"wait_user",
            "summary":"All visible output parts were reviewed. Please choose the rollout window."
        }))
        .expect("one decision resolves the full member turn");
    let mut expected = reports
        .iter()
        .map(|report| report.id.clone())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(result["relay"]["source_event_ids"], json!(expected));
    assert_eq!(fixture.status()["count"], 0);
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|event| {
                event.kind == "runtime.delivery"
                    && event.data["actor_id"] == "web-lead"
                    && event.data["state"] == "accepted"
            })
            .count(),
        25
    );
}

#[test]
fn only_the_foreman_may_decide_and_continue_requires_concrete_work() {
    let fixture = Fixture::new("relay permissions");
    let report = fixture.report("worker-a", None);
    fixture.handoff(&report, "turn-permission");
    let mut args = json!({
        "group_id":fixture.group.group_id,"by":"worker-a",
        "event_ids":[report.id],"decision":"wait_user","summary":"Need user input"
    })
    .as_object()
    .cloned()
    .expect("args");
    let forbidden = decide(
        &fixture.home,
        &DaemonRequest {
            v: 1,
            op: "coordination_decide".into(),
            args: std::mem::take(&mut args),
        },
    )
    .expect_err("peer decision forbidden");
    assert_eq!(forbidden.code, "relay_decision_forbidden");

    let incomplete = fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"continue",
            "summary":"Continue the work"
        }))
        .expect_err("bare continue forbidden");
    assert_eq!(incomplete.code, "invalid_args");
    assert!(relay_events(&fixture.events(), DECISION_KIND).is_empty());
}

#[test]
fn interrupted_continue_partials_block_contradictions_and_recover_without_duplicates() {
    let fixture = Fixture::new("interrupted continue dispatch");
    let report = fixture.report("worker-a", None);
    let handoff = fixture.handoff(&report, "turn-continue-partial");
    let next_text = "Run the affected regression and report exact evidence.";
    let sent = interrupted_continue(&fixture, &report, &handoff, next_text);
    assert_eq!(sent["message_sent"], true);

    let contradiction = fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"wait_user",
            "summary":"Ask the user instead."
        }))
        .expect_err("real next work must not be contradicted by a later wait decision");
    assert_eq!(contradiction.code, "relay_decision_conflict");
    assert!(relay_events(&fixture.events(), DECISION_KIND).is_empty());
    assert_eq!(lead_messages(&fixture.events(), "worker-b").len(), 1);

    let recovered = fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"continue",
            "summary":"Run the final verification.",
            "next_actor_id":"worker-b","next_title":"Final verification",
            "next_text":next_text
        }))
        .expect("the same continue recovers");
    assert_eq!(recovered["relay"]["decision"], "continue");
    let relay_tasks = fixture
        .context()
        .tasks
        .iter()
        .filter(|task| {
            task.get("client_request_id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.contains("tracked-send:"))
        })
        .count();
    assert_eq!(relay_tasks, 1);
    assert_eq!(lead_messages(&fixture.events(), "worker-b").len(), 1);

    let follow_up = fixture.report("worker-a", None);
    fixture.handoff(&follow_up, "turn-task-only");
    let follow_up_decision =
        decision_id(&fixture.group.group_id, "web-lead", &[follow_up.id.clone()]);
    let client_id = super::super::message_idempotency::tracked_client_id(
        &fixture.group.group_id,
        "web-lead",
        &follow_up_decision,
    );
    fixture.sync(
        "web-lead",
        json!({
            "op":"task.create","title":"Prepared follow-up","outcome":"Finish it",
            "status":"active","assignee":"worker-b","waiting_on":"actor",
            "client_request_id":client_id
        }),
    );
    let conflict = fixture
        .decide(json!({
            "event_ids":[follow_up.id],"decision":"wait_user",
            "summary":"Ask the user instead."
        }))
        .expect_err("prepared next work must prevent a contradictory wait decision");
    assert_eq!(conflict.code, "relay_decision_conflict");
}

#[test]
fn generic_context_sync_cannot_write_private_relay_machine_state() {
    let fixture = Fixture::new("relay note boundary");
    let request = DaemonRequest {
        v: 1,
        op: "context_sync".into(),
        args: json!({
            "group_id":fixture.group.group_id,"by":"user",
            "ops":[{"op":"coordination.relay.note","kind":"decision","id":"forged",
                "summary":"Pretend complete","decision":"complete","safe_to_idle":true}]
        })
        .as_object()
        .cloned()
        .expect("request"),
    };
    let error = super::super::context::resolve_operation(&request)
        .expect("context operation")
        .execute(&fixture.home, &request)
        .expect_err("private relay state must not be public context input");
    assert_eq!(error.code, "permission_denied");
}

#[test]
fn resolving_one_handoff_does_not_tell_the_foreman_to_idle_with_another_pending() {
    let fixture = Fixture::new("multiple relay obligations");
    let first = fixture.report("worker-a", None);
    let second = fixture.report("worker-b", None);
    fixture.handoff(&first, "turn-first-obligation");
    fixture.handoff(&second, "turn-second-obligation");

    let first_result = fixture
        .decide(json!({
            "event_ids":[first.id],"decision":"wait_user",
            "summary":"The first result needs user input."
        }))
        .expect("first decision");
    assert_eq!(first_result["caller_may_idle"], false);
    assert_eq!(first_result["safe_to_idle"], false);
    assert_eq!(
        first_result["current_responsibility"]["kind"],
        "foreman_review"
    );

    let pending = fixture.status();
    assert_eq!(pending["count"], 1);
    assert_eq!(pending["caller_may_idle"], false);

    let second_result = fixture
        .decide(json!({
            "event_ids":[second.id],"decision":"wait_user",
            "summary":"The second result also needs user input."
        }))
        .expect("second decision");
    assert_eq!(second_result["caller_may_idle"], true);
    assert_eq!(second_result["safe_to_idle"], true);
}

#[test]
fn assigned_and_unassigned_work_stay_the_foremans_responsibility() {
    let fixture = Fixture::new("current responsibility");
    let task_id = fixture.task("Actor still owns work", "worker-a");
    let active = fixture.status();
    assert_eq!(active["count"], 0);
    assert_eq!(active["safe_to_idle"], false);
    assert_eq!(active["responsibility"]["kind"], "actor_work");
    assert_eq!(active["responsibility"]["tasks"][0]["task_id"], task_id);

    let unassigned = Fixture::new("unassigned responsibility");
    unassigned.sync(
        "web-lead",
        json!({
            "op":"task.create","title":"Unassigned follow-up","outcome":"Find an owner",
            "status":"active","waiting_on":"actor"
        }),
    );
    let state = current_group_state(&unassigned.home, &unassigned.group, &unassigned.events())
        .expect("group responsibility");
    assert_eq!(state["safe_to_idle"], false);
    assert_eq!(state["responsibility"]["kind"], "actor_work");
    assert_eq!(state["responsibility"]["tasks"][0]["actor_id"], "web-lead");
    assert!(!actor_may_idle_from_state(&state, "web-lead"));
    assert!(actor_may_idle_from_state(&state, "worker-a"));
}

#[test]
fn a_paused_group_blocks_continue_and_reminders_but_keeps_decisions_safe() {
    let mut fixture = Fixture::new("paused relay");
    let report = fixture.report("worker-a", None);
    fixture.handoff(&report, "paused-active-turn");
    fixture.claim_for_web(&report);
    let wait_request = DaemonRequest {
        v: 1,
        op: "runtime_wait_next_turn".into(),
        args: json!({"group_id":fixture.group.group_id,"actor_id":"web-lead",
            "by":"web-lead","transport":"web_model_browser"})
        .as_object()
        .cloned()
        .expect("wait args"),
    };
    let wait = super::super::runtime_state::resolve_operation(&wait_request)
        .expect("wait operation")
        .execute(&fixture.home, &wait_request)
        .expect("active turn before pause");
    assert_eq!(wait["status"], "work_available");

    fixture.pause();
    let refused = fixture
        .decide(json!({
            "event_ids":[report.id],"decision":"continue","summary":"Continue",
            "next_actor_id":"worker-b","next_title":"Next","next_text":"Do the next step"
        }))
        .expect_err("paused group cannot continue");
    assert_eq!(refused.code, "relay_group_paused");
    assert_eq!(fixture.context().tasks.len(), 0);
    assert!(relay_events(&fixture.events(), DECISION_KIND).is_empty());

    fixture.append_delivery(&report.id, "accepted", 30);
    let reminder = fixture.remind(false);
    assert_eq!(reminder["reminded"], false);
    assert_eq!(reminder["reason"], "actor_inactive");
    let state = fixture.status();
    assert_eq!(state["count"], 1, "pause discarded the handoff");
    assert_eq!(state["safe_to_idle"], true);
    assert_eq!(state["responsibility"]["kind"], "user_pause");

    let recorded = fixture
        .decide(json!({"event_ids":[report.id],"decision":"wait_user"}))
        .expect("recording responsibility must not require restarting a paused actor");
    assert_eq!(recorded["current_responsibility"]["kind"], "user_pause");
    assert_eq!(recorded["caller_may_idle"], true);
    let replay = fixture
        .decide(json!({"event_ids":[report.id],"decision":"wait_user"}))
        .expect("retry remains successful while paused");
    assert_eq!(replay["replayed"], true);
    assert_eq!(relay_events(&fixture.events(), DECISION_KIND).len(), 1);
    assert_eq!(
        GroupStore::new(fixture.home.clone())
            .expect("store")
            .load(&fixture.group.group_id)
            .expect("group")
            .state,
        GroupState::Paused
    );
}

#[test]
fn an_ignored_reminder_escalates_once_and_any_decision_resolves_it() {
    let fixture = Fixture::new("relay reminder lifecycle");
    let report = fixture.report("worker-a", None);
    let handoff = fixture.handoff(&report, "turn-reminder");
    fixture.append_delivery(&report.id, "accepted", 30);

    let reminder = fixture.remind(false);
    assert_eq!(reminder["reminded"], true);
    let reminder_id = reminder["reminder_event"]["id"]
        .as_str()
        .expect("reminder id")
        .to_owned();
    assert!(
        reminder["reminder_event"]["data"]["text"]
            .as_str()
            .expect("reminder text")
            .contains("call cccc_coordination")
    );
    let again = fixture.remind(false);
    assert_eq!(again["reminded"], false);
    assert_eq!(relay_notes(&fixture.events(), "decision_reminder").len(), 1);

    fixture
        .decide(json!({
            "event_ids":[reminder_id],"decision":"wait_user",
            "summary":"Please approve the final rollout."
        }))
        .expect("deciding the reminder resolves the handoff too");
    assert_eq!(fixture.delivery_state(&reminder_id), "accepted");
    let context = fixture.context();
    assert_eq!(
        handoff_note(&context, handoff_id(&handoff))["status"],
        "resolved"
    );

    let ignored = fixture.report("worker-a", None);
    let escalated_handoff = fixture.handoff(&ignored, "turn-escalation");
    fixture.append_delivery(&ignored.id, "accepted", 90);
    let second = fixture.remind(false);
    let second_id = second["reminder_event"]["id"]
        .as_str()
        .expect("reminder id")
        .to_owned();
    fixture.append_delivery(&second_id, "accepted", 40);
    let busy = fixture.remind(false);
    assert_eq!(busy["escalated"], false, "a working web page was escalated");
    let escalated = fixture.remind(true);
    assert_eq!(escalated["reminded"], false);
    assert_eq!(escalated["escalated"], true);
    let escalation_id = escalated["escalation_event"]["id"]
        .as_str()
        .expect("escalation id")
        .to_owned();
    assert_eq!(escalated["escalation_event"]["data"]["to"], json!(["user"]));
    assert!(
        escalated["escalation_event"]["data"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("No model will be woken repeatedly"))
    );

    let state = fixture.status();
    assert_eq!(state["count"], 1);
    assert_eq!(state["requires_decision"], false);
    assert_eq!(state["awaiting_user_intervention"], true);
    assert_eq!(state["caller_may_idle"], true);
    assert_eq!(state["safe_to_idle"], true);
    assert_eq!(state["responsibility"]["kind"], "user_intervention");
    let context = fixture.context();
    let note = handoff_note(&context, handoff_id(&escalated_handoff));
    assert_eq!(note["status"], "waiting_user");
    assert_eq!(note["escalation_event_id"], escalation_id);
    assert!(
        fixture.pending_sources().is_empty(),
        "the user escalation re-entered the web model queue and could create a loop"
    );

    let repeated = fixture.remind(true);
    assert_eq!(repeated["escalated"], false);
    assert_eq!(
        relay_notes(&fixture.events(), "decision_escalation").len(),
        1
    );

    fixture
        .decide(json!({
            "event_ids":[ignored.id],"decision":"wait_user",
            "summary":"Please choose the next step for the preserved result."
        }))
        .expect("explicit decision after user resumes the foreman");
    assert_eq!(fixture.status()["count"], 0);
}

#[test]
fn live_work_and_pending_reviews_are_never_hidden_by_each_other() {
    for escalated in [true, false] {
        let fixture = Fixture::new(if escalated {
            "escalation plus live task"
        } else {
            "review plus live task"
        });
        let live_task = fixture.task("Independent live work", "worker-b");
        let report = fixture.report("worker-a", None);
        let handoff = fixture.handoff(&report, "turn-combined");
        if escalated {
            fixture.append_delivery(&report.id, "accepted", 90);
            let reminder = fixture.remind(false);
            let reminder_id = reminder["reminder_event"]["id"]
                .as_str()
                .expect("reminder id")
                .to_owned();
            fixture.append_delivery(&reminder_id, "accepted", 40);
            assert_eq!(fixture.remind(true)["escalated"], true);
        }
        let state = current_group_state(&fixture.home, &fixture.group, &fixture.events())
            .expect("combined responsibility");
        assert_eq!(state["safe_to_idle"], false);
        assert_eq!(
            state["responsibilities"]
                .as_array()
                .expect("responsibilities")
                .len(),
            2
        );
        assert!(!actor_may_idle_from_state(&state, "worker-b"));
        if escalated {
            assert_eq!(state["responsibility"]["kind"], "actor_work");
            assert_eq!(state["responsibility"]["tasks"][0]["task_id"], live_task);
            assert!(actor_may_idle_from_state(&state, "web-lead"));
            assert_eq!(
                state["user_intervention"]["handoff_ids"],
                json!([handoff.data["handoff_id"]])
            );
        } else {
            assert_eq!(state["responsibility"]["kind"], "foreman_review");
            assert_eq!(state["actor_work"]["tasks"][0]["task_id"], live_task);
            assert!(!actor_may_idle_from_state(&state, "web-lead"));
            assert_eq!(
                state["responsibility"]["handoff_ids"],
                json!([handoff.data["handoff_id"]])
            );
        }
    }
}

#[test]
fn status_repairs_the_context_note_after_an_escalation_write_is_interrupted() {
    let fixture = Fixture::new("escalation context recovery");
    let report = fixture.report("worker-a", None);
    let handoff = fixture.handoff(&report, "turn-escalation-recovery");
    let handoff_id = handoff.data["handoff_id"].as_str().expect("handoff id");
    let reminder = fixture.notice(&handoff, RelayNotice::Reminder);
    for id in [&report.id, &reminder.id] {
        fixture.append_delivery(id, "accepted", 0);
    }
    let mut escalation = Event::new("chat.message", &fixture.group.group_id);
    escalation.by = "system".into();
    escalation.data = json!({
        "to":["user"],"message_mode":"send","text":"Collaboration is waiting for your decision.",
        "relay_kind":"decision_escalation","relay_actor_id":"web-lead",
        "relay_handoff_ids":[handoff_id],"relay_source_event_ids":[report.id]
    })
    .as_object()
    .cloned()
    .expect("escalation");
    ledger::append(&fixture.path(), &escalation).expect("simulate visible escalation write");
    assert_eq!(
        fixture.context().coordination["recent_handoffs"][0]["status"],
        "pending_review"
    );

    let result = fixture.status();
    assert_eq!(result["awaiting_user_intervention"], true);
    let context = fixture.context();
    let note = handoff_note(&context, handoff_id);
    assert_eq!(note["status"], "waiting_user");
    assert_eq!(note["escalation_event_id"], escalation.id);
}

#[test]
fn old_false_escalation_cannot_leave_an_undelivered_handoff_waiting_for_user() {
    let fixture = Fixture::new("repair old false escalation");
    let report = fixture.report("worker-a", None);
    let handoff = fixture.handoff(&report, "old-bad-delivery");
    let reminder = fixture.notice(&handoff, RelayNotice::Reminder);
    let append = |state: &str| {
        for id in [&report.id, &reminder.id] {
            fixture.append_delivery(id, state, 0);
        }
    };
    append("ambiguous");
    let escalation = fixture.notice(&handoff, RelayNotice::Escalation);
    ensure_handoff_note(
        &fixture.home,
        &fixture.group,
        &handoff,
        "waiting_user",
        None,
        Some(&escalation.id),
    )
    .expect("old note");
    assert_eq!(fixture.status()["awaiting_user_intervention"], false);
    assert_eq!(
        fixture.context().coordination["recent_handoffs"][0]["status"],
        "pending_review"
    );
    append("accepted");
    assert!(
        !escalation_for_handoff(
            &fixture.events(),
            handoff.data["handoff_id"].as_str().expect("handoff")
        ),
        "a later receipt retroactively legitimized the old false escalation"
    );
}

#[test]
fn concurrent_reminder_and_escalation_checks_create_one_visible_event_each() {
    let fixture = Fixture::new("concurrent relay reminders");
    let report = fixture.report("worker-a", None);
    fixture.handoff(&report, "turn-concurrent-reminder");
    fixture.append_delivery(&report.id, "accepted", 90);
    for (idle, kind) in [(false, "decision_reminder"), (true, "decision_escalation")] {
        let barrier = std::sync::Barrier::new(10);
        let results = std::thread::scope(|scope| {
            let threads = (0..10)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        remind_due(
                            &fixture.home,
                            &remind_request(&fixture.group.group_id, idle),
                        )
                    })
                })
                .collect::<Vec<_>>();
            threads
                .into_iter()
                .map(|t| t.join().expect("racing check"))
                .collect::<Vec<_>>()
        });
        assert!(results.iter().all(Result::is_ok), "{kind}: {results:?}");
        let events = fixture.events();
        let emitted = relay_notes(&events, kind);
        assert_eq!(emitted.len(), 1, "duplicate concurrent event: {kind}");
        if !idle {
            fixture.append_delivery(&emitted[0].id, "accepted", 40);
        }
    }
}

#[test]
fn concurrent_identical_decisions_commit_one_machine_decision_without_duplicate_output() {
    let fixture = Fixture::new("concurrent identical decision");
    let report = fixture.report("worker-a", None);
    fixture.handoff(&report, "turn-concurrent-decision");
    let barrier = std::sync::Barrier::new(10);
    let results = std::thread::scope(|scope| {
        let threads = (0..10)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    fixture.decide(json!({"event_ids":[report.id],"decision":"wait_user"}))
                })
            })
            .collect::<Vec<_>>();
        threads
            .into_iter()
            .map(|t| t.join().expect("racing decision"))
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let events = fixture.events();
    assert_eq!(relay_events(&events, DECISION_KIND).len(), 1);
    assert!(lead_messages(&events, "user").is_empty());
}

#[test]
fn replay_cannot_claim_success_for_a_batch_containing_a_new_report() {
    let fixture = Fixture::new("mixed handled and new sources");
    let first = fixture.report("worker-a", None);
    fixture.handoff(&first, "first-turn");
    fixture
        .decide(json!({"event_ids":[first.id],"decision":"wait_user"}))
        .expect("first decision");
    let second = fixture.report("worker-b", None);
    let handoff = fixture.handoff(&second, "second-turn");
    let error = fixture
        .decide(json!({
            "event_ids":[first.id, second.id],"decision":"wait_user"
        }))
        .expect_err("a partial overlap must not report the whole batch as applied");
    assert_eq!(error.code, "relay_decision_conflict");
    let events = fixture.events();
    assert_eq!(relay_events(&events, DECISION_KIND).len(), 1);
    assert_eq!(
        unresolved_source_ids(&events, &handoff),
        vec![second.id.clone()]
    );
    fixture
        .decide(json!({"event_ids":[second.id],"decision":"wait_user"}))
        .expect("the new source can still be decided separately");
    assert_eq!(relay_events(&fixture.events(), DECISION_KIND).len(), 2);
}

#[test]
fn unconfirmed_delivery_never_claims_the_foreman_received_a_report_or_reminder() {
    let fixture = Fixture::new("unconfirmed transport is not receipt");
    let report = fixture.report("worker-a", None);
    fixture.handoff(&report, "unconfirmed-turn");
    for state in ["claimed", "failed", "ambiguous"] {
        fixture.append_delivery(&report.id, state, 180);
        let check = fixture.remind(true);
        assert_eq!(
            check["reminded"], false,
            "{state} was misreported as delivered"
        );
        assert_eq!(check["escalated"], false);
    }
    fixture.append_delivery(&report.id, "accepted", 180);
    let reminded = fixture.remind(true);
    assert_eq!(reminded["reminded"], true);
    let reminder = reminded["reminder_event"]["id"]
        .as_str()
        .expect("reminder id")
        .to_owned();
    for state in ["claimed", "failed", "ambiguous"] {
        fixture.append_delivery(&reminder, state, 180);
        assert_eq!(
            fixture.remind(true)["escalated"],
            false,
            "{state} reminder was blamed on the foreman"
        );
    }
    fixture.append_delivery(&reminder, "accepted", 180);
    assert_eq!(fixture.remind(true)["escalated"], true);
}

#[test]
fn delayed_report_completion_never_completes_a_newer_task_for_the_same_member() {
    let fixture = Fixture::new("late report does not own new work");
    let old = fixture.task("Original assignment", "worker-a");
    fixture.sync(
        "web-lead",
        json!({"op":"task.move","task_id":old,"status":"done"}),
    );
    let mut dispatch = Event::new("chat.message", &fixture.group.group_id);
    dispatch.by = "web-lead".into();
    dispatch.data=json!({"to":["worker-a"],"message_mode":"send","text":"Do original task","refs":[{"kind":"task_ref","task_id":old}]}).as_object().cloned().expect("dispatch");
    ledger::append(&fixture.path(), &dispatch).expect("dispatch event");
    let mut report = Event::new("chat.message", &fixture.group.group_id);
    report.by = "worker-a".into();
    report.data=json!({"to":["web-lead"],"message_mode":"send","text":"Original result","reply_to":dispatch.id}).as_object().cloned().expect("report");
    ledger::append(&fixture.path(), &report).expect("report event");
    fixture.handoff(&report, "old-turn");
    let newer = fixture.task("New assignment after the report", "worker-a");
    let error = fixture
        .decide(json!({"event_ids":[report.id],"decision":"complete"}))
        .expect_err("whole-group completion must not silently complete the newer assignment");
    assert_eq!(error.code, "relay_work_remains");
    assert_eq!(task(&fixture.context(), &old)["status"], "done");
    assert_eq!(task(&fixture.context(), &newer)["status"], "active");
}
