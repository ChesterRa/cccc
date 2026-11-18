use super::*;
use cccc_contracts::RunnerKind;
use serde_json::json;
use std::time::Duration;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
    connector: String,
}

impl Fixture {
    fn new(runtime: ActorRuntime) -> Self {
        let temp = tempfile::tempdir().expect("isolated remote facts");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let groups = GroupStore::new(home.clone()).expect("store");
        let mut group = groups.create("Remote facts", "").expect("group");
        let mut actor = Actor::new("worker");
        actor.runtime = runtime;
        actor.runner = RunnerKind::Headless;
        actor.enabled = true;
        group.actors.push(actor);
        group.running = true;
        group.state = GroupState::Active;
        groups.save(&group).expect("save");
        let configured = web_model_connectors::configure_provider(
            &home,
            runtime.web_model_provider().expect("remote provider"),
        )
        .expect("connector");
        let connector = configured["connector"]["connector_id"]
            .as_str()
            .expect("id")
            .to_owned();
        let generation = cccc_core::actors::generation_identity(&group.actors[0]);
        if runtime == ActorRuntime::ChatgptDots {
            web_model_connectors::bind_dots(
                &home,
                &connector,
                &group.group_id,
                "worker",
                &generation,
            )
            .expect("Dot route");
        } else {
            web_model_connectors::bind_grok(
                &home,
                &connector,
                &group.group_id,
                "worker",
                &generation,
                "https://grok.com/bot/00000000-0000-4000-8000-000000000001",
            )
            .expect("Grok route");
            web_model_connectors::grok_webhook::configure(
                &home,
                &connector,
                &group.group_id,
                "worker",
                &generation,
                "https://api2.cursor.sh/automations/webhook/fixture",
                "synthetic-facts-key",
            )
            .expect("Routine");
        }
        Self {
            _temp: temp,
            home,
            group,
            connector,
        }
    }

    fn snapshot(&self) -> Snapshot {
        read(
            &self.home,
            Instant::now() + Duration::from_secs(5),
            &BTreeSet::new(),
        )
        .expect("read facts")
    }

    fn work(&self, change: impl FnOnce(&mut Value)) {
        if self.group.actors[0].runtime == ActorRuntime::ChatgptDots {
            web_model_connectors::dots_update(&self.home, &self.connector, |connector| {
                change(&mut connector["dots"]);
                Ok(())
            })
            .expect("Dot work");
        } else {
            web_model_connectors::grok_webhook::change(
                &self.home,
                &self.connector,
                &self.group.group_id,
                "worker",
                |_, private| {
                    change(private);
                    Ok(())
                },
            )
            .expect("Grok work");
        }
    }
}

#[test]
fn idle_remote_routes_have_no_process_or_unconfirmed_activity() {
    for runtime in [ActorRuntime::ChatgptDots, ActorRuntime::GrokWebModel] {
        let fixture = Fixture::new(runtime);
        let before =
            std::fs::read(fixture.home.root().join("web_model_connectors.yaml")).expect("state");
        let snapshot = fixture.snapshot();
        assert!(snapshot.refusals.is_empty());
        assert_eq!(snapshot.covered.len(), 1);
        assert!(snapshot.active.is_empty());
        assert!(
            before
                == std::fs::read(fixture.home.root().join("web_model_connectors.yaml"))
                    .expect("state")
        );
    }
}

#[test]
fn an_observed_local_launch_outweighs_next_start_remote_settings() {
    let fixture = Fixture::new(ActorRuntime::ChatgptDots);
    let held = BTreeSet::from([(
        fixture.group.group_id.clone(),
        "worker".into(),
        fixture.group.actors[0].created_at.clone(),
    )]);
    std::fs::write(
        fixture.home.root().join("web_model_connectors.yaml"),
        "invalid: [",
    )
    .expect("unusable next-start connection");
    let snapshot = read(
        &fixture.home,
        Instant::now() + Duration::from_secs(5),
        &held,
    )
    .expect("actual holder remains authority");
    assert!(
        snapshot.covered.is_empty() && snapshot.active.is_empty() && snapshot.refusals.is_empty()
    );
}

#[test]
fn an_unfinished_canonical_turn_survives_a_missing_private_execution_projection() {
    for runtime in [ActorRuntime::ChatgptDots, ActorRuntime::GrokWebModel] {
        let fixture = Fixture::new(runtime);
        GroupStore::new(fixture.home.clone())
            .expect("store")
            .mutate(&fixture.group.group_id, |group| {
                group.extra.insert(
                    "runtime_states".into(),
                    json!({"worker":{
                "active_turn_id":"original-turn","active_event_ids":["original-event"]}}),
                );
                Ok(())
            })
            .expect("canonical accepted work");
        let snapshot = fixture.snapshot();
        assert!(snapshot.refusals.is_empty());
        assert_eq!(snapshot.active.len(), 1);
        assert!(snapshot.active[0].owner_id.is_none());
        assert!(snapshot.active[0].generation.is_none());
        assert_eq!(
            snapshot.active[0].actor_created_at,
            fixture.group.actors[0].created_at
        );
    }
}

#[test]
fn pending_wakes_are_unknown_activity_but_failed_delivery_is_not_an_endless_row() {
    for runtime in [ActorRuntime::ChatgptDots, ActorRuntime::GrokWebModel] {
        let fixture = Fixture::new(runtime);
        fixture.work(|state| {
            state["subscription"] = json!({"enabled":true,"expires_at_ms":null});
        });
        for state in [
            "pending",
            "sending",
            "delivered",
            "claimed",
            "failed",
            "completed",
        ] {
            fixture.work(|private| private["wake"] = json!({"event_id":"same-wake","state":state}));
            let snapshot = fixture.snapshot();
            assert!(snapshot.refusals.is_empty());
            assert_eq!(
                snapshot.active.len(),
                usize::from(!matches!(state, "failed" | "completed")),
                "{runtime:?}: {state}"
            );
        }
        // This cohort classification does not change authorization: Grok may
        // accept a late failed-wake claim, whereas Dots requires a live wake.
        fixture.work(|private| {
            private["wake"]["state"] = json!("failed");
            private["execution"] = json!({"state":"active"});
        });
        assert_eq!(fixture.snapshot().active.len(), 1);
    }
}

#[test]
fn an_expired_dots_subscription_does_not_make_an_unclaimed_wake_active_forever() {
    let fixture = Fixture::new(ActorRuntime::ChatgptDots);
    fixture.work(|private| {
        private["subscription"] = json!({"enabled":true,"expires_at_ms":1});
        private["wake"] = json!({"event_id":"expired-wake","state":"delivered"});
    });
    assert!(fixture.snapshot().active.is_empty());
}

#[test]
fn stale_routes_and_unreadable_connector_documents_refuse_without_mutation() {
    let fixture = Fixture::new(ActorRuntime::ChatgptDots);
    GroupStore::new(fixture.home.clone())
        .expect("store")
        .mutate(&fixture.group.group_id, |group| {
            group.actors[0].generation = "replacement-generation".into();
            Ok(())
        })
        .expect("replace actor");
    let path = fixture.home.root().join("web_model_connectors.yaml");
    let before = std::fs::read(&path).expect("state");
    let snapshot = fixture.snapshot();
    assert_eq!(snapshot.refusals[0].code, "remote_resource_unconfirmed");
    assert!(before == std::fs::read(&path).expect("unchanged state"));
    std::fs::write(&path, "invalid: [").expect("corrupt fixture");
    let snapshot = fixture.snapshot();
    assert_eq!(snapshot.refusals[0].code, "remote_resource_unconfirmed");
    assert_eq!(
        std::fs::read_to_string(path).expect("unchanged corruption"),
        "invalid: ["
    );
}
