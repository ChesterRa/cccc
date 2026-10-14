use super::*;
use crate::{GroupStore, actors};
use cccc_contracts::{Actor, ActorRuntime, GroupState};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group_id: String,
    connector_id: String,
    binding: Value,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Dots", "").expect("group");
        let mut actor = Actor::new("dots");
        actor.runtime = ActorRuntime::ChatgptDots;
        actors::add(&mut group, actor).expect("actor");
        group.running = true;
        group.state = GroupState::Active;
        store.save(&group).expect("save");
        let configured = configure(&home).expect("configure");
        let connector_id = configured["connector"]["connector_id"]
            .as_str()
            .expect("id")
            .to_owned();
        let binding = bind_dots(
            &home,
            &connector_id,
            &group.group_id,
            "dots",
            &group.actors[0].generation,
        )
        .expect("bind");
        Self {
            _temp: temp,
            home,
            group_id: group.group_id,
            connector_id,
            binding,
        }
    }

    fn connector(&self) -> Value {
        load(&self.home)
            .expect("load")
            .into_iter()
            .find(|connector| connector["connector_id"] == self.connector_id)
            .expect("connector")
    }

    fn wake(&self, event_id: &str) -> String {
        dots_update(&self.home, &self.connector_id, |connector| {
            connector["dots"]["wake"] = json!({"event_id":event_id,"state":"delivered"});
            Ok(())
        })
        .expect("wake");
        dots_wake_token(&self.connector(), &self.binding, event_id).expect("credential")
    }

    fn execution(&self, event_id: &str, turn_id: &str) {
        dots_update(&self.home, &self.connector_id, |connector| {
            connector["dots"]["execution"] = json!({"event_id":event_id,
                "epoch":connector["dots"]["epoch"],"turn_id":turn_id,
                "event_ids":["source-a"],"state":"active"});
            Ok(())
        })
        .expect("execution");
        GroupStore::new(self.home.clone())
            .expect("store")
            .mutate(&self.group_id, |group| {
                group.extra.insert(
                    "runtime_states".into(),
                    json!({"dots":{"active_turn_id":turn_id,
                "active_event_ids":["source-a"]}}),
                );
                Ok(())
            })
            .expect("active turn");
    }

    fn pair(&self) -> String {
        let session = session_key(
            &self.connector(),
            &json!({"openai/session":"synthetic-dot-session", "openai/subject":"synthetic-user"}),
        )
        .expect("host session hash");
        let setup = begin_dots_pairing(&self.home, &self.connector_id)
            .expect("setup")
            .expect("unpaired Dot");
        accept_pairing(
            &self.home,
            &self.connector_id,
            setup["code"].as_str().expect("code"),
            &session,
        )
        .expect("accept Dot code");
        confirm_dots_pairing(
            &self.home,
            &self.connector_id,
            setup["pairing_id"].as_str().expect("pair id"),
        )
        .expect("pair Dot session");
        session
    }

    fn route(&self, token: &str, tool: &str) -> Value {
        dots_binding_for_token(&self.connector(), token, tool).expect("route")
    }
}

#[test]
fn dot_pairing_reuses_approved_session_without_touching_work_or_subscription() {
    let fixture = Fixture::new();
    let session = fixture.pair();
    fixture.wake("event-one");
    dots_update(&fixture.home, &fixture.connector_id, |c| {
        c["dots"]["subscription"] = json!({"enabled":true,"subscription_id":"fixture-sub"});
        Ok(())
    })
    .expect("native task");
    let before = std::fs::read(store_path(&fixture.home)).expect("private state");
    assert!(
        begin_dots_pairing(&fixture.home, &fixture.connector_id)
            .expect("existing pair")
            .is_none()
    );
    assert!(std::fs::read(store_path(&fixture.home)).expect("unchanged state") == before);
    assert!(dots_claim_binding(&fixture.connector(), &session).is_some());
    assert!(dots_claim_binding(&fixture.connector(), "another-session").is_none());
}

#[test]
fn dot_pairing_codes_cannot_outlive_expiry_disconnect_or_binding_replacement() {
    for reason in ["expiry", "disconnect", "replacement"] {
        let fixture = Fixture::new();
        let setup = begin_dots_pairing(&fixture.home, &fixture.connector_id)
            .expect("setup")
            .expect("new pair");
        accept_pairing(
            &fixture.home,
            &fixture.connector_id,
            setup["code"].as_str().expect("code"),
            "synthetic-host-key",
        )
        .expect("accepted candidate");
        match reason {
            "expiry" => dots_update(&fixture.home, &fixture.connector_id, |c| {
                c["pairings"][route_key(&fixture.group_id, "dots")]["expires_at_ms"] = json!(0);
                Ok(())
            })
            .expect("expire candidate"),
            _ => {
                retire_actor(&fixture.home, &fixture.group_id, "dots").expect("retire");
                bind_dots(
                    &fixture.home,
                    &fixture.connector_id,
                    &fixture.group_id,
                    "dots",
                    if reason == "disconnect" {
                        fixture.binding["generation"].as_str().expect("generation")
                    } else {
                        "new-generation"
                    },
                )
                .expect("replacement");
            }
        }
        let before = std::fs::read(store_path(&fixture.home)).expect("before refused confirmation");
        assert!(
            confirm_dots_pairing(
                &fixture.home,
                &fixture.connector_id,
                setup["pairing_id"].as_str().expect("pair id")
            )
            .is_err(),
            "stale {reason} candidate must be refused"
        );
        assert!(dots_claim_binding(&fixture.connector(), "synthetic-host-key").is_none());
        assert!(
            std::fs::read(store_path(&fixture.home)).expect("after refused confirmation") == before
        );
    }
}

#[test]
fn dot_pairing_while_paused_does_not_restore_task_authority() {
    let fixture = Fixture::new();
    let setup = begin_dots_pairing(&fixture.home, &fixture.connector_id)
        .expect("setup")
        .expect("new pair");
    dots_invalidate(&fixture.home, &fixture.group_id, "dots").expect("stop execution authority");
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    groups
        .mutate(&fixture.group_id, |group| {
            group.actors[0].enabled = false;
            Ok(())
        })
        .expect("pause access");
    accept_pairing(
        &fixture.home,
        &fixture.connector_id,
        setup["code"].as_str().expect("code"),
        "synthetic-host-key",
    )
    .expect("accept unexpired code");
    confirm_dots_pairing(
        &fixture.home,
        &fixture.connector_id,
        setup["pairing_id"].as_str().expect("pair id"),
    )
    .expect("pair the same paused Actor");
    let captured =
        dots_claim_binding(&fixture.connector(), "synthetic-host-key").expect("paired claim route");
    assert!(validate_binding(&fixture.home, &captured).is_err());
    groups
        .mutate(&fixture.group_id, |group| {
            group.actors[0].enabled = true;
            Ok(())
        })
        .expect("resume access");
    validate_binding(&fixture.home, &captured).expect("explicitly resumed claim authority");
    dots_invalidate(&fixture.home, &fixture.group_id, "dots").expect("revoke captured authority");
    assert!(validate_binding(&fixture.home, &captured).is_err());
}

#[test]
fn a_dot_cannot_take_a_session_already_owned_by_a_browser_actor() {
    let fixture = Fixture::new();
    let browser_pair = begin_pairing(
        &fixture.home,
        &fixture.connector_id,
        "browser-group",
        "browser",
        "browser-generation",
        false,
    )
    .expect("browser setup");
    accept_pairing(
        &fixture.home,
        &fixture.connector_id,
        browser_pair["code"].as_str().expect("browser code"),
        "owned-session",
    )
    .expect("browser accept");
    let browser = confirm_pairing(
        &fixture.home,
        &fixture.connector_id,
        "browser-group",
        "browser",
        "browser-generation",
        browser_pair["pairing_id"]
            .as_str()
            .expect("browser pair id"),
        "https://chatgpt.com/c/synthetic-browser",
    )
    .expect("browser confirmation");
    let dot_pair = begin_dots_pairing(&fixture.home, &fixture.connector_id)
        .expect("Dot setup")
        .expect("Dot candidate");
    accept_pairing(
        &fixture.home,
        &fixture.connector_id,
        dot_pair["code"].as_str().expect("Dot code"),
        "owned-session",
    )
    .expect("Dot candidate accepted");
    assert!(
        confirm_dots_pairing(
            &fixture.home,
            &fixture.connector_id,
            dot_pair["pairing_id"].as_str().expect("Dot pair id")
        )
        .is_err()
    );
    assert!(binding_for_session(&fixture.connector(), "owned-session") == Some(browser));
    assert!(dots_claim_binding(&fixture.connector(), "owned-session").is_none());
}

#[test]
fn one_dot_binding_reuses_connector_without_replacing_browser_routes() {
    let fixture = Fixture::new();
    let pair = begin_pairing(
        &fixture.home,
        &fixture.connector_id,
        "g_browser",
        "browser",
        "gen",
        false,
    )
    .expect("pair");
    accept_pairing(
        &fixture.home,
        &fixture.connector_id,
        pair["code"].as_str().expect("code"),
        "session-browser",
    )
    .expect("accept");
    let browser = confirm_pairing(
        &fixture.home,
        &fixture.connector_id,
        "g_browser",
        "browser",
        "gen",
        pair["pairing_id"].as_str().expect("pairing id"),
        "https://chatgpt.com/c/browser",
    )
    .expect("browser");
    assert_eq!(
        bind_dots(
            &fixture.home,
            &fixture.connector_id,
            &fixture.group_id,
            "dots",
            fixture.binding["generation"].as_str().expect("generation")
        )
        .expect("same binding"),
        fixture.binding
    );
    for (group, actor, generation) in [
        ("other-group", "dots", "gen"),
        (fixture.group_id.as_str(), "other", "gen"),
        (fixture.group_id.as_str(), "dots", "replacement"),
    ] {
        assert!(
            bind_dots(
                &fixture.home,
                &fixture.connector_id,
                group,
                actor,
                generation
            )
            .is_err()
        );
    }
    let connector = fixture.connector();
    assert_eq!(
        binding_for_session(&connector, "session-browser"),
        Some(browser)
    );
    assert_eq!(fixture.binding["url"], DOTS_URL);
    assert_eq!(fixture.binding["transport"], "mcp_events");
    assert!(
        conversation_url(DOTS_URL).is_err(),
        "browser pairing rules stay strict"
    );
    assert!(
        begin_pairing(
            &fixture.home,
            &fixture.connector_id,
            &fixture.group_id,
            "dots",
            "gen",
            false
        )
        .is_err()
    );
}

#[test]
fn subscription_credentials_cannot_claim_and_unissued_execution_credentials_cannot_act() {
    let fixture = Fixture::new();
    let subscription =
        dots_subscription_token(&fixture.connector(), &fixture.binding).expect("subscription");
    assert!(dots_subscription_binding(&fixture.connector(), &subscription).is_some());
    for tool in [
        "cccc_runtime_wait_next_turn",
        "cccc_message_send",
        "cccc_runtime_complete_turn",
    ] {
        assert!(dots_binding_for_token(&fixture.connector(), &subscription, tool).is_none());
    }
    let token = fixture.wake("event-one");
    assert!(token != subscription, "authority credentials must differ");
    assert!(dots_subscription_binding(&fixture.connector(), &token).is_none());
    assert!(dots_claim_binding(&fixture.connector(), "unpaired-host-session").is_none());
    let session = fixture.pair();
    let claim = dots_claim_binding(&fixture.connector(), &session).expect("paired claim");
    assert_eq!(claim["dots_stage"], "claim");
    validate_binding(&fixture.home, &claim).expect("claim route");
    for tool in [
        "cccc_runtime_wait_next_turn",
        "cccc_message_send",
        "cccc_context_get",
        "cccc_code_exec",
        "cccc_runtime_complete_turn",
    ] {
        assert!(dots_binding_for_token(&fixture.connector(), &token, tool).is_none());
    }
    assert!(
        dots_binding_for_token(&fixture.connector(), "bad", "cccc_runtime_wait_next_turn")
            .is_none()
    );
}

#[test]
fn paired_session_claim_is_limited_and_snapshots_the_current_wake_and_epoch() {
    let fixture = Fixture::new();
    let grant =
        dots_subscription_token(&fixture.connector(), &fixture.binding).expect("claim grant");
    let session = fixture.pair();
    let idle = dots_claim_binding(&fixture.connector(), &session).expect("paired idle claim");
    assert_eq!(idle["dots_stage"], "claim");
    assert_eq!(idle["dots_wake_id"], Value::Null);
    validate_binding(&fixture.home, &idle).expect("idle identity only");
    for tool in [
        "cccc_message_send",
        "cccc_context_get",
        "cccc_code_exec",
        "cccc_code_wait",
        "cccc_runtime_complete_turn",
    ] {
        assert!(dots_binding_for_token(&fixture.connector(), &grant, tool).is_none());
    }

    let execution_token = fixture.wake("event-one");
    assert!(
        validate_binding(&fixture.home, &idle).is_err(),
        "an idle read cannot borrow a wake created while waiting for the permit"
    );
    let waiting = dots_claim_binding(&fixture.connector(), &session).expect("paired waiting claim");
    assert_eq!(waiting["dots_wake_id"], "event-one");
    fixture.execution("event-one", "turn-one");
    validate_binding(&fixture.home, &waiting).expect("duplicate claim identity only");
    assert!(
        dots_binding_for_token(
            &fixture.connector(),
            &execution_token,
            "cccc_runtime_wait_next_turn"
        )
        .is_none(),
        "execution authority must not become continuous claim authority"
    );
    assert_eq!(
        fixture.route(&execution_token, "cccc_message_send")["dots_stage"],
        "execution"
    );
    let execution = fixture.route(&execution_token, "cccc_message_send");
    dots_invalidate(&fixture.home, &fixture.group_id, "dots").expect("stop authority");
    assert!(validate_binding(&fixture.home, &waiting).is_err());
    assert!(validate_binding(&fixture.home, &execution).is_err());
    let resumed_claim =
        dots_claim_binding(&fixture.connector(), &session).expect("resumed paired claim");
    assert_ne!(waiting["dots_epoch"], resumed_claim["dots_epoch"]);
    validate_binding(&fixture.home, &resumed_claim)
        .expect("continuing paired session captures only the new read identity");

    retire_actor(&fixture.home, &fixture.group_id, "dots").expect("unbind");
    let replacement = bind_dots(
        &fixture.home,
        &fixture.connector_id,
        &fixture.group_id,
        "dots",
        fixture.binding["generation"].as_str().expect("generation"),
    )
    .expect("new explicit binding");
    assert_ne!(replacement["revision"], fixture.binding["revision"]);
    assert!(
        dots_binding_for_token(&fixture.connector(), &grant, "cccc_runtime_wait_next_turn")
            .is_none()
    );
}

#[test]
fn execution_uses_the_exact_canonical_turn_and_rejects_incomplete_bindings() {
    let fixture = Fixture::new();
    let token = fixture.wake("event-one");
    fixture.execution("event-one", "turn-one");
    let route = fixture.route(&token, "cccc_message_send");
    assert_eq!(route["dots_stage"], "execution");
    assert_eq!(route["dots_turn_id"], "turn-one");
    assert_eq!(route["group_id"], fixture.group_id);
    validate_binding(&fixture.home, &route).expect("active");
    GroupStore::new(fixture.home.clone())
        .expect("store")
        .mutate(&fixture.group_id, |group| {
            group.extra["runtime_states"]["dots"]["active_turn_id"] = json!("turn-two");
            Ok(())
        })
        .expect("different turn");
    assert!(validate_binding(&fixture.home, &route).is_err());
    assert!(
        validate_binding(&fixture.home, &fixture.binding).is_err(),
        "safe binding is not task authority"
    );
}

#[test]
fn completion_retry_is_allowed_but_completed_cloud_work_cannot_keep_acting() {
    let fixture = Fixture::new();
    let token = fixture.wake("event-one");
    fixture.execution("event-one", "turn-one");
    let active = fixture.route(&token, "cccc_message_send");
    dots_update(&fixture.home, &fixture.connector_id, |connector| {
        let mut completed = connector["dots"]["execution"].clone();
        completed["state"] = json!("completed");
        connector["dots"]["last_execution"] = completed.clone();
        connector["dots"]["execution"] = completed;
        connector["dots"]["wake"]["state"] = json!("completed");
        Ok(())
    })
    .expect("complete");
    assert!(validate_binding(&fixture.home, &active).is_err());
    for tool in [
        "cccc_message_send",
        "cccc_runtime_wait_next_turn",
        "cccc_code_exec",
    ] {
        assert!(dots_binding_for_token(&fixture.connector(), &token, tool).is_none());
    }
    let completed = fixture.route(&token, "cccc_runtime_complete_turn");
    assert_eq!(completed["dots_stage"], "completion");
    validate_binding(&fixture.home, &completed).expect("receipt retry");
    let newer = fixture.wake("event-two");
    fixture.execution("event-two", "turn-two");
    validate_binding(&fixture.home, &completed).expect("old receipt does not borrow new turn");
    assert!(
        newer != token,
        "separate wakes must have distinct credentials"
    );
}

#[test]
fn stop_then_start_rejects_old_tasks_with_the_same_generation_and_turn() {
    let fixture = Fixture::new();
    let subscription =
        dots_subscription_token(&fixture.connector(), &fixture.binding).expect("subscription");
    let token = fixture.wake("event-one");
    fixture.execution("event-one", "turn-one");
    let old = fixture.route(&token, "cccc_message_send");
    dots_update(&fixture.home, &fixture.connector_id, |connector| {
        connector["dots"]["subscription"] = json!({"id":"sub-one","secret":"synthetic-private","callback":"https://callback.invalid/"});
        Ok(())
    }).expect("subscription");
    let before = fixture.connector();
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    store
        .mutate(&fixture.group_id, |group| {
            group.actors[0].enabled = false;
            Ok(())
        })
        .expect("stop");
    assert!(validate_binding(&fixture.home, &old).is_err());
    dots_invalidate(&fixture.home, &fixture.group_id, "dots").expect("invalidate");
    store
        .mutate(&fixture.group_id, |group| {
            group.actors[0].enabled = true;
            Ok(())
        })
        .expect("start");
    let after = fixture.connector();
    assert!(
        before["dots"]["subscription"] == after["dots"]["subscription"],
        "stopping must retain the private subscription"
    );
    assert_eq!(after["dots"]["execution"]["turn_id"], "turn-one");
    assert_eq!(after["dots"]["execution"]["state"], "interrupted");
    assert_eq!(after["dots"]["wake"]["state"], "interrupted");
    assert_ne!(before["dots"]["epoch"], after["dots"]["epoch"]);
    assert!(
        dots_subscription_token(&after, &fixture.binding).expect("stable subscription")
            == subscription,
        "the continuous binding grant must remain stable"
    );
    assert!(dots_binding_for_token(&after, &token, "cccc_message_send").is_none());
    assert!(validate_binding(&fixture.home, &old).is_err());
    let fresh = dots_wake_token(&after, &fixture.binding, "event-one").expect("new epoch");
    assert!(
        dots_binding_for_token(&after, &fresh, "cccc_runtime_wait_next_turn").is_none(),
        "no silent replay of interrupted work"
    );
}

#[test]
fn private_state_stays_out_of_bindings_and_retirement_rolls_back_exactly() {
    let fixture = Fixture::new();
    let token = fixture.wake("event-one");
    fixture.execution("event-one", "turn-one");
    dots_update(&fixture.home, &fixture.connector_id, |connector| {
        connector["dots"]["subscription"] =
            json!({"callback":"https://private.invalid/","secret":"synthetic-private"});
        Ok(())
    })
    .expect("private state");
    let before = fixture.connector();
    assert_eq!(
        before["bindings"][route_key(&fixture.group_id, "dots")],
        fixture.binding
    );
    for secret in [
        token.as_str(),
        "synthetic-private",
        "private.invalid",
        "event-one",
        "turn-one",
    ] {
        assert!(!fixture.binding.to_string().contains(secret));
    }
    let snapshots = retire_actor(&fixture.home, &fixture.group_id, "dots").expect("retire");
    assert!(fixture.connector()["dots"].is_null());
    assert!(
        dots_subscription_binding(
            &fixture.connector(),
            &dots_subscription_token(&before, &fixture.binding).expect("before token")
        )
        .is_none()
    );
    restore(&fixture.home, &snapshots).expect("internal rollback");
    assert!(
        fixture.connector()["dots"] == before["dots"],
        "rollback must restore exact private state"
    );
    validate_binding(&fixture.home, &fixture.route(&token, "cccc_message_send"))
        .expect("original execution");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(store_path(&fixture.home))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    retire_group(&fixture.home, &fixture.group_id).expect("retire group");
    assert!(fixture.connector()["dots"].is_null());
}

#[test]
fn actor_runtime_replacement_and_revocation_do_not_lend_dots_authority() {
    let fixture = Fixture::new();
    let token = fixture.wake("event-one");
    fixture.execution("event-one", "turn-one");
    let route = fixture.route(&token, "cccc_context_get");
    GroupStore::new(fixture.home.clone())
        .expect("store")
        .mutate(&fixture.group_id, |group| {
            group.actors[0].runtime = ActorRuntime::WebModel;
            group.actors[0].normalize_runtime_constraints();
            Ok(())
        })
        .expect("same provider different runtime");
    assert!(validate_binding(&fixture.home, &route).is_err());
    let subscription =
        dots_subscription_token(&fixture.connector(), &fixture.binding).expect("subscription");
    revoke(&fixture.home, &fixture.connector_id).expect("revoke");
    let connector = fixture.connector();
    assert!(connector["dots"].is_null());
    assert!(dots_subscription_binding(&connector, &subscription).is_none());
    assert!(dots_binding_for_token(&connector, &token, "cccc_context_get").is_none());
}

#[test]
fn private_updates_abort_atomically_and_restores_reject_replacement() {
    let fixture = Fixture::new();
    let before = std::fs::read(store_path(&fixture.home)).expect("bytes");
    assert!(
        dots_update(
            &fixture.home,
            &fixture.connector_id,
            |connector| -> io::Result<()> {
                connector["dots"]["subscription"] = json!({"secret":"should-not-persist"});
                Err(error("fixture failure"))
            }
        )
        .is_err()
    );
    assert!(
        std::fs::read(store_path(&fixture.home)).expect("bytes") == before,
        "a failed update must not change the private store"
    );
    let snapshots = retire_actor(&fixture.home, &fixture.group_id, "dots").expect("retire");
    bind_dots(
        &fixture.home,
        &fixture.connector_id,
        "replacement",
        "other",
        "new-generation",
    )
    .expect("replace");
    let replacement = std::fs::read(store_path(&fixture.home)).expect("bytes");
    assert!(restore(&fixture.home, &snapshots).is_err());
    assert!(
        std::fs::read(store_path(&fixture.home)).expect("bytes") == replacement,
        "a failed restore must not change the replacement private store"
    );
}

#[test]
fn private_store_parse_errors_do_not_disclose_subscription_scalars() {
    let fixture = Fixture::new();
    let private = "synthetic-private-value-for-yaml-probe";
    let path = store_path(&fixture.home);
    for tag in ["bool", "int", "float"] {
        let malformed = format!(
            "version: 3\nconnectors:\n  chatgpt_web:\n    dots:\n      subscription:\n        signing_secret: !!{tag} {private}\n"
        );
        std::fs::write(&path, &malformed).expect("malformed private store");
        let failure = match load(&fixture.home) {
            Err(failure) => failure,
            Ok(_) => panic!("invalid typed scalar was accepted"),
        };
        assert!(
            failure.to_string() == "web-model connector store could not be read",
            "private parser diagnostics must not escape the store boundary"
        );
        assert!(!failure.to_string().contains(private));
        assert!(
            std::fs::read_to_string(&path).expect("original bytes") == malformed,
            "a failed read must not replace the private store"
        );
    }
}
