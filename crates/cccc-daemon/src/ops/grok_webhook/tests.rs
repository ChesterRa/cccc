use super::*;
use cccc_contracts::{DaemonResponse, Event};
use cccc_core::ledger;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("isolated Grok fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("isolated Grok fixture");
        home.initialize().expect("isolated Grok fixture");
        let groups = GroupStore::new(home.clone()).expect("isolated Grok fixture");
        let mut group = groups
            .create("Grok fixture", "")
            .expect("isolated Grok fixture");
        let mut actor = Actor::new("bot");
        actor.runtime = ActorRuntime::GrokWebModel;
        actor.runner = RunnerKind::Headless;
        actor.enabled = false;
        cccc_core::actors::add(&mut group, actor).expect("isolated Grok fixture");
        group.running = true;
        group.state = GroupState::Active;
        groups.save(&group).expect("isolated Grok fixture");
        let configured =
            store::configure_provider(&home, "grok_web").expect("isolated Grok fixture");
        let id = configured["connector"]["connector_id"]
            .as_str()
            .expect("isolated Grok fixture")
            .to_owned();
        store::bind_grok(
            &home,
            &id,
            &group.group_id,
            "bot",
            &cccc_core::actors::generation_identity(&group.actors[0]),
            "https://grok.com/bot/00000000-0000-4000-8000-000000000001",
        )
        .expect("isolated Grok fixture");
        let f = Self {
            _temp: temp,
            home,
            group,
            id,
        };
        f.call("grok_webhook_configure",json!({"url":"https://api2.cursor.sh/automations/webhook/synthetic","key":"synthetic-provider-key"}));
        f
    }
    fn response(&self, op: &str, mut args: Value) -> DaemonResponse {
        let m = args.as_object_mut().expect("isolated Grok fixture");
        m.entry("group_id").or_insert(json!(self.group.group_id));
        m.entry("actor_id").or_insert(json!("bot"));
        m.entry("by").or_insert(json!("user"));
        crate::dispatch::dispatch(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args: m.clone(),
            },
        )
    }
    fn call(&self, op: &str, args: Value) -> Value {
        let r = self.response(op, args);
        assert!(r.ok, "{op}: {:?}", r.error);
        Value::Object(r.result)
    }
    fn start(&self) {
        self.call("actor_start", json!({}));
    }
    fn private(&self) -> Value {
        store::load(&self.home)
            .expect("isolated Grok fixture")
            .into_iter()
            .find(|c| c["connector_id"] == self.id)
            .expect("isolated Grok fixture")
    }
    fn source(&self, text: &str) -> String {
        let mut e = Event::new("chat.message", &self.group.group_id);
        e.by = "user".into();
        e.data = json!({"to":["bot"],"text":text,"message_mode":"send"})
            .as_object()
            .expect("isolated Grok fixture")
            .clone();
        ledger::append(
            &GroupStore::new(self.home.clone())
                .expect("isolated Grok fixture")
                .ledger_path(&self.group.group_id)
                .expect("isolated Grok fixture"),
            &e,
        )
        .expect("isolated Grok fixture");
        e.id
    }
    fn next(&self) -> Value {
        self.call("grok_webhook_next_delivery", json!({}))["delivery"].clone()
    }
    fn capture(&self, d: &Value, tool: &str) -> Value {
        let mut b = private::resolve(
            &self.private(),
            d["body"]["actor_token"]
                .as_str()
                .expect("isolated Grok fixture"),
            tool,
        )
        .expect("isolated Grok fixture");
        b["connector_id"] = json!(self.id);
        b
    }
    fn claim(&self, b: &Value) -> Value {
        self.call("grok_work_claim", json!({"by":"bot","_cccc_web_binding":b}))
    }
    fn execution(&self, claimed: &Value, tool: &str) -> Value {
        let mut b = private::resolve(
            &self.private(),
            claimed["actor_token"]
                .as_str()
                .expect("isolated Grok fixture"),
            tool,
        )
        .expect("isolated Grok fixture");
        b["connector_id"] = json!(self.id);
        b
    }
    fn complete(&self, c: &Value) -> Value {
        self.call("runtime_complete_turn",json!({"by":"bot","turn_id":c["turn"]["turn_id"],"event_ids":c["turn"]["event_ids"],"_cccc_web_binding":self.execution(c,"cccc_runtime_complete_turn")}))
    }
    fn record(&self, d: &Value, http: u16) -> Value {
        let mut args = d.clone();
        args.as_object_mut()
            .expect("isolated Grok fixture")
            .remove("body");
        args["http_status"] = json!(http);
        self.call("grok_webhook_delivery_record", args)
    }
}

#[test]
fn idle_configuration_preserves_access_and_queued_work_and_retires_old_wakes() {
    let f = Fixture::new();
    let args = json!({"url":"https://api2.cursor.sh/automations/webhook/replacement","key":"synthetic-key"});
    assert_eq!(
        f.call("grok_webhook_configure", args.clone())["enabled"],
        false
    );
    f.start();
    let source = f.source("queued before editing");
    let old = f.next();
    let captured = f.capture(&old, "cccc_runtime_wait_next_turn");
    let configured = f.call("grok_webhook_configure", args);
    assert_eq!(configured["enabled"], true);
    assert_eq!(configured["pending_count"], 1);
    assert!(store::validate_binding(&f.home, &captured).is_err());
    let next = f.next();
    assert_ne!(old["event_id"], next["event_id"]);
    f.record(&old, 200);
    assert_eq!(
        f.call("grok_webhook_status", json!({}))["phase"],
        "delivering"
    );
    let claimed = f.claim(&f.capture(&next, "cccc_runtime_wait_next_turn"));
    assert_eq!(claimed["turn"]["event_ids"], json!([source]));
    f.complete(&claimed);
}

#[test]
fn invalid_idle_configuration_keeps_the_live_connection_and_access() {
    let f = Fixture::new();
    f.start();
    f.source("queued");
    let old = f.next();
    let captured = f.capture(&old, "cccc_runtime_wait_next_turn");
    let before = f.private();
    for args in [
        json!({"url":"http://127.0.0.1/invalid","key":"synthetic-key"}),
        json!({"url":"https://api2.cursor.sh/automations/webhook/replacement","key":"bad\nkey"}),
    ] {
        assert!(!f.response("grok_webhook_configure", args).ok);
        assert_eq!(f.private(), before);
        assert_eq!(f.call("grok_webhook_status", json!({}))["enabled"], true);
        assert!(store::validate_binding(&f.home, &captured).is_ok());
    }
    f.complete(&f.claim(&captured));
}

#[test]
fn connection_edits_cannot_interrupt_a_claimed_task() {
    let f = Fixture::new();
    f.start();
    f.source("already claimed");
    let d = f.next();
    let claimed = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    let execution = f.execution(&claimed, "cccc_runtime_complete_turn");
    let before = f.private();
    for (op, args) in [
        (
            "grok_webhook_configure",
            json!({"url":"https://api2.cursor.sh/automations/webhook/replacement","key":"synthetic-key"}),
        ),
        (
            "web_model_grok_bind",
            json!({"connector_id":f.id,"url":"https://grok.com/bot/00000000-0000-4000-8000-000000000002"}),
        ),
    ] {
        let response = f.response(op, args);
        assert_eq!(
            response.error.expect("active task rejection").code,
            "delivery_unresolved"
        );
        assert_eq!(f.private(), before);
        let status = f.call("grok_webhook_status", json!({}));
        assert_eq!(status["enabled"], true);
        assert_eq!(status["active_turn_id"], claimed["turn"]["turn_id"]);
        assert!(store::validate_binding(&f.home, &execution).is_ok());
    }
    f.complete(&claimed);
    let configured = f.call("grok_webhook_configure", json!({"url":"https://api2.cursor.sh/automations/webhook/replacement","key":"synthetic-key"}));
    assert_eq!(configured["enabled"], true);
}

#[test]
fn previous_completion_credential_cannot_complete_a_new_turn() {
    let f = Fixture::new();
    f.start();
    f.source("first task");
    let d = f.next();
    let first = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    f.complete(&first);
    let previous = f.execution(&first, "cccc_runtime_complete_turn");
    assert_eq!(previous["grok_stage"], "completion");
    f.source("second task");
    let d = f.next();
    let second = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    let before = f.private();
    let path = GroupStore::new(f.home.clone())
        .expect("store")
        .ledger_path(&f.group.group_id)
        .expect("ledger");
    let ledger_before = std::fs::read(&path).expect("ledger");
    for (mut args, code) in [
        (
            json!({"turn_id":second["turn"]["turn_id"],"event_ids":second["turn"]["event_ids"]}),
            "permission_denied",
        ),
        (
            json!({"turn_id":first["turn"]["turn_id"],"event_ids":second["turn"]["event_ids"]}),
            "completion_conflict",
        ),
        (
            json!({"turn_id":second["turn"]["turn_id"],"event_ids":first["turn"]["event_ids"]}),
            "permission_denied",
        ),
        (
            json!({"event_ids":second["turn"]["event_ids"]}),
            "permission_denied",
        ),
    ] {
        args["by"] = json!("bot");
        args["_cccc_web_binding"] = previous.clone();
        let r = f.response("runtime_complete_turn", args);
        assert!(
            !r.ok,
            "A previous task credential must not complete newer work"
        );
        assert_eq!(r.error.expect("scope rejection").code, code);
    }
    let r = f.response("runtime_complete_turn", json!({"by":"bot","turn_id":first["turn"]["turn_id"],"event_ids":first["turn"]["event_ids"],"_cccc_web_binding":f.execution(&second,"cccc_runtime_complete_turn")}));
    assert_eq!(
        r.error
            .expect("current credential cannot acknowledge an older turn")
            .code,
        "permission_denied"
    );
    assert_eq!(std::fs::read(&path).expect("ledger"), ledger_before);
    assert_eq!(f.private(), before);
    assert_eq!(
        f.call("grok_webhook_status", json!({}))["active_turn_id"],
        second["turn"]["turn_id"]
    );
    f.complete(&first);
    assert_eq!(std::fs::read(&path).expect("ledger"), ledger_before);
    assert_eq!(
        f.call("grok_webhook_status", json!({}))["active_turn_id"],
        second["turn"]["turn_id"]
    );
    f.complete(&second);
}

#[test]
fn native_claims_once_and_drains_later_work_in_ledger_order() {
    let f = Fixture::new();
    f.start();
    let first = f.source("first");
    let d = f.next();
    let b = f.capture(&d, "cccc_runtime_wait_next_turn");
    assert!(!d["body"].to_string().contains("first"));
    assert!(d["body"].get("key").is_none());
    assert!(
        !f.response(
            "message_send",
            json!({"by":"bot","to":["user"],"text":"forbidden","_cccc_web_binding":b})
        )
        .ok
    );
    let c = f.claim(&b);
    assert_eq!(c["turn"]["event_ids"], json!([first]));
    assert_ne!(c["actor_token"], d["body"]["actor_token"]);
    assert_eq!(f.claim(&b)["status"], "turn_in_progress");
    assert!(f.claim(&b).get("actor_token").is_none());
    let second = f.source("second");
    let third = f.source("third");
    assert!(f.next().is_null());
    // A late HTTP acknowledgement cannot overwrite an already claimed wake.
    assert_eq!(f.record(&d, 200)["recorded"], false);
    let execution = f.execution(&c, "cccc_message_send");
    assert!(store::validate_binding(&f.home, &execution).is_ok());
    f.complete(&c);
    assert!(store::validate_binding(&f.home, &execution).is_err());
    assert!(
        private::resolve(
            &f.private(),
            c["actor_token"].as_str().expect("isolated Grok fixture"),
            "cccc_message_send"
        )
        .is_none()
    );
    f.complete(&c); // exact completion retry is still valid
    assert_eq!(f.claim(&b)["status"], "idle");
    let next = f.next();
    let new = f.capture(&next, "cccc_runtime_wait_next_turn");
    assert!(store::validate_binding(&f.home, &b).is_err());
    let c2 = f.claim(&new);
    assert_eq!(c2["turn"]["event_ids"], json!([second, third]));
    f.complete(&c); // a delayed old receipt must not release the newer turn
    assert_eq!(
        f.call("grok_webhook_status", json!({}))["active_turn_id"],
        c2["turn"]["turn_id"]
    );
    f.complete(&c2);
    assert!(f.next().is_null());
    let public = f.call("grok_webhook_status", json!({})).to_string();
    let ledger = std::fs::read_to_string(
        GroupStore::new(f.home.clone())
            .expect("isolated Grok fixture")
            .ledger_path(&f.group.group_id)
            .expect("isolated Grok fixture"),
    )
    .expect("isolated Grok fixture");
    for secret in [
        "synthetic-provider-key",
        d["body"]["actor_token"]
            .as_str()
            .expect("isolated Grok fixture"),
        c["actor_token"].as_str().expect("isolated Grok fixture"),
        "api2.cursor.sh",
    ] {
        assert!(!public.contains(secret));
        assert!(!ledger.contains(secret));
    }
}

#[test]
fn grok_claims_require_a_current_webhook_event() {
    let f = Fixture::new();
    f.start();
    f.source("queued");
    for transport in ["web_model_browser", "web_model_pull"] {
        let rejected = f.response(
            "runtime_wait_next_turn",
            json!({"by":"bot","transport":transport}),
        );
        assert!(!rejected.ok);
        assert_eq!(
            rejected.error.expect("claim rejected").code,
            "grok_event_required"
        );
    }
    f.call("actor_stop", json!({}));
    assert!(
        !f.response("grok_webhook_configure", json!({"transport":"browser"}))
            .ok
    );
    f.start();
    for op in [
        "web_model_runtime_recover_turn",
        "web_model_browser_delivery_record",
        "web_model_delivery_preferences_get",
    ] {
        let response = f.response(op, json!({"by":"bot"}));
        assert!(!response.ok, "{op} must not create a browser path");
        assert_ne!(
            response.error.expect("rejected operation").code,
            "unknown_op"
        );
    }
    let d = f.next();
    let claimed = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    f.complete(&claimed);
}

#[test]
fn user_can_resolve_enabled_turn_without_replaying_inputs_or_reviving_credentials() {
    let f = Fixture::new();
    f.start();
    f.source("abandoned input");
    let delivery = f.next();
    let claimed = f.claim(&f.capture(&delivery, "cccc_runtime_wait_next_turn"));
    let execution = f.execution(&claimed, "cccc_message_send");
    let exact =
        json!({"turn_id":claimed["turn"]["turn_id"],"event_ids":claimed["turn"]["event_ids"]});
    let mut denied = exact.clone();
    denied["by"] = json!("bot");
    assert!(!f.response("grok_work_resolve", denied).ok);
    let mut stale = exact.clone();
    stale["event_ids"] = json!(["different-input"]);
    assert!(!f.response("grok_work_resolve", stale).ok);
    assert!(store::validate_binding(&f.home, &execution).is_ok());
    let later = f.source("independent queued input");
    f.call("grok_work_resolve", exact.clone());
    let status = f.call("grok_webhook_status", json!({}));
    assert_eq!(status["enabled"], true);
    assert_eq!(status["active_turn_id"], "");
    assert_eq!(status["last_execution"]["status"], "cancelled");
    assert!(store::validate_binding(&f.home, &execution).is_err());
    let next = f.next();
    let next_claim = f.claim(&f.capture(&next, "cccc_runtime_wait_next_turn"));
    assert_eq!(next_claim["turn"]["event_ids"], json!([later]));
    assert!(!f.response("grok_work_resolve", exact).ok);
    assert_eq!(
        f.call("grok_webhook_status", json!({}))["active_turn_id"],
        next_claim["turn"]["turn_id"]
    );
    f.complete(&next_claim);
}

#[test]
fn stop_revokes_authority_and_requires_explicit_resolution_without_replay() {
    let f = Fixture::new();
    f.start();
    let old = f.source("accepted");
    let d = f.next();
    let claim = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    let b = f.execution(&claim, "cccc_message_send");
    f.call("actor_stop", json!({}));
    assert!(store::validate_binding(&f.home, &b).is_err());
    f.start();
    assert!(f.next().is_null());
    assert_eq!(
        f.call("grok_webhook_status", json!({}))["phase"],
        "interrupted"
    );
    assert!(store::validate_binding(&f.home, &b).is_err());
    assert!(!f.response("grok_webhook_retry", json!({})).ok);
    f.call("actor_stop", json!({}));
    assert!(
        !f.response("grok_webhook_configure", json!({"url":"https://api2.cursor.sh/automations/webhook/replacement","key":"synthetic-key"}))
            .ok
    );
    assert!(
        !f.response(
            "grok_work_resolve",
            json!({"turn_id":"wrong","event_ids":[old]})
        )
        .ok
    );
    f.call(
        "grok_work_resolve",
        json!({"turn_id":claim["turn"]["turn_id"],"event_ids":claim["turn"]["event_ids"]}),
    );
    let later = f.source("later");
    f.start();
    let next = f.next();
    let result = f.claim(&f.capture(&next, "cccc_runtime_wait_next_turn"));
    assert_eq!(result["turn"]["event_ids"], json!([later]));
}

#[test]
fn failed_or_lost_wakes_retry_the_same_identity_not_business_inputs() {
    let f = Fixture::new();
    f.start();
    f.source("queued");
    let d = f.next();
    f.record(&d, 401);
    assert_eq!(f.call("grok_webhook_status", json!({}))["phase"], "failed");
    assert!(f.next().is_null());
    f.call("grok_webhook_retry", json!({}));
    let retry = f.next();
    assert_eq!(retry["body"], d["body"]);
    f.record(&retry, 200);
    assert!(f.next().is_null());
    assert!(!f.response("grok_webhook_retry", json!({})).ok);
    private::change(&f.home, &f.id, &f.group.group_id, "bot", |_, s| {
        s["wake"]["sent_at_ms"] = json!(0);
        Ok(())
    })
    .expect("isolated Grok fixture");
    assert_eq!(
        f.call("grok_webhook_status", json!({}))["phase"],
        "unclaimed"
    );
    f.call("grok_webhook_retry", json!({}));
    let retry = f.next();
    assert_eq!(retry["body"], d["body"]);
    let c = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    assert_eq!(
        f.claim(&f.capture(&retry, "cccc_runtime_wait_next_turn"))["status"],
        "turn_in_progress"
    );
    f.complete(&c);
}

#[test]
fn configuration_is_private_and_retires_with_the_actor_route() {
    let f = Fixture::new();
    for url in [
        "http://127.0.0.1/automations/webhook/a",
        "https://api2.cursor.sh.evil.test/automations/webhook/a",
        "https://api2.cursor.sh/automations/webhook/a?key=secret",
        "https://api2.cursor.sh/automations/webhook/a/b",
    ] {
        assert!(
            !f.response(
                "grok_webhook_configure",
                json!({"url":url,"key":"synthetic"})
            )
            .ok
        );
    }
    let retired =
        store::retire_actor(&f.home, &f.group.group_id, "bot").expect("isolated Grok fixture");
    assert!(private::state(&f.private(), &f.group.group_id, "bot").is_null());
    store::restore(&f.home, &retired).expect("isolated Grok fixture");
    assert!(private::state(&f.private(), &f.group.group_id, "bot")["key"].is_string());
    store::revoke(&f.home, &f.id).expect("isolated Grok fixture");
    let c = f.private();
    assert!(c["grok_webhooks"].is_null());
}

#[test]
fn concurrent_claims_admit_only_one_turn() {
    let f = Fixture::new();
    f.start();
    f.source("once");
    let d = f.next();
    let b = f.capture(&d, "cccc_runtime_wait_next_turn");
    std::thread::scope(|scope| {
        let handles = (0..6)
            .map(|_| scope.spawn(|| f.claim(&b)))
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|h| h.join().expect("isolated Grok fixture"))
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .iter()
                .filter(|r| r["status"] == "work_available")
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|r| r["status"] == "turn_in_progress")
                .count(),
            5
        );
    });
}

#[test]
fn explicit_mail_delivery_uses_the_native_claim_lane() {
    let f = Fixture::new();
    f.start();
    let mail = f.call(
        "message_send",
        json!({"to":["bot"],"text":"explicitly delivered Mail","message_mode":"mail"}),
    )["event"]["id"]
        .clone();
    f.call(
        "message_deliver",
        json!({"source_event_id":mail,"actor_ids":["bot"]}),
    );
    let delivery = super::super::runtime_delivery::latest_state(
        &f.home,
        &f.group.group_id,
        "bot",
        mail.as_str().expect("fixture Mail ID"),
    )
    .expect("delivery")
    .expect("claim");
    assert_eq!(delivery, ("claimed".into(), "web_model_pull".into()));
    let d = f.next();
    let claimed = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    assert!(
        claimed["turn"]["event_ids"]
            .as_array()
            .expect("event IDs")
            .contains(&mail)
    );
    f.complete(&claimed);
}

fn legacy_browser_claim(f: &Fixture) -> String {
    let source = f.source("old uncertain browser handoff");
    super::super::runtime_delivery::claim(
        &f.home,
        &f.group,
        &f.group.actors[0],
        &source,
        "web_model_browser",
        false,
    )
    .expect("legacy claim");
    source
}

#[test]
fn configuring_native_retires_legacy_unowned_claims_without_replay() {
    let f = Fixture::new();
    let old = legacy_browser_claim(&f);
    f.call("grok_webhook_configure", json!({"url":"https://api2.cursor.sh/automations/webhook/replacement","key":"synthetic-key"}));
    f.start();
    assert!(f.next().is_null());
    assert_eq!(
        super::super::runtime_delivery::latest_state(&f.home, &f.group.group_id, "bot", &old)
            .expect("old handoff")
            .expect("old claim"),
        ("ambiguous".into(), "web_model_browser".into())
    );
    let fresh = f.source("new native work");
    let d = f.next();
    let claimed = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    assert_eq!(claimed["turn"]["event_ids"], json!([fresh]));
    f.complete(&claimed);
    assert!(f.next().is_null());
}

#[test]
fn native_scheduler_settles_legacy_claims_after_interrupted_configuration() {
    let f = Fixture::new();
    let source = legacy_browser_claim(&f);
    // Simulate process exit between configuration and claim bookkeeping.
    private::configure(
        &f.home,
        &f.id,
        &f.group.group_id,
        "bot",
        &cccc_core::actors::generation_identity(&f.group.actors[0]),
        "https://api2.cursor.sh/automations/webhook/replacement",
        "synthetic-key",
    )
    .expect("saved native binding");
    f.start();
    assert!(f.next().is_null());
    assert_eq!(
        super::super::runtime_delivery::latest_state(&f.home, &f.group.group_id, "bot", &source)
            .expect("old handoff")
            .expect("old claim")
            .0,
        "ambiguous"
    );
}

#[test]
fn connector_revocation_does_not_hide_or_strand_an_accepted_turn() {
    let f = Fixture::new();
    f.start();
    f.source("interrupted by connector revocation");
    let d = f.next();
    let claimed = f.claim(&f.capture(&d, "cccc_runtime_wait_next_turn"));
    f.call("web_model_connector_revoke", json!({"connector_id":f.id}));
    let status = f.call("grok_webhook_status", json!({}));
    assert_eq!(status["connected"], false);
    assert_eq!(status["phase"], "interrupted");
    assert_eq!(status["configured"], false);
    f.call("actor_stop", json!({}));
    f.call(
        "grok_work_resolve",
        json!({"turn_id":claimed["turn"]["turn_id"],"event_ids":claimed["turn"]["event_ids"]}),
    );
    assert_eq!(
        f.call("grok_webhook_status", json!({}))["active_turn_id"],
        ""
    );
    let new = store::configure_provider(&f.home, "grok_web").expect("new connector");
    let id = new["connector"]["connector_id"]
        .as_str()
        .expect("new connector ID");
    f.call("web_model_grok_bind",json!({"connector_id":id,"url":"https://grok.com/bot/00000000-0000-4000-8000-000000000001"}));
    assert_eq!(f.call("grok_webhook_configure",json!({"url":"https://api2.cursor.sh/automations/webhook/reconfigured","key":"synthetic-new-key"}))["configured"],true);
}
