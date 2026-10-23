use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::context::{ContextDoc, ContextStore};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Value, json};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group_id: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut fixture = Self {
            _temp: temp,
            home,
            group_id: String::new(),
        };
        let created = fixture.call("group_create", json!({"title":"Actor context cleanup"}));
        fixture.group_id = created["group"]["group_id"]
            .as_str()
            .expect("group id")
            .into();
        for actor_id in ["coordinator", "leaving", "staying"] {
            fixture.add_actor(actor_id);
        }
        fixture
    }

    fn request(&self, op: &str, mut args: Value) -> DaemonResponse {
        args["group_id"] = json!(self.group_id);
        cccc_daemon::handle_request(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("arguments").clone(),
            },
        )
    }

    fn call(&self, op: &str, args: Value) -> Value {
        let response = self.request(op, args);
        assert!(response.ok, "{op}: {:?}", response.error);
        Value::Object(response.result)
    }

    fn add_actor(&self, actor_id: &str) {
        self.call(
            "actor_add",
            json!({"actor_id":actor_id,"runtime":"custom","command":["sh"],"enabled":false,"by":"user"}),
        );
    }

    fn context(&self) -> ContextDoc {
        ContextStore::new(self.home.clone())
            .expect("context store")
            .load(&self.group_id)
            .expect("context")
    }
}

#[test]
fn removed_actor_loses_open_task_references_and_working_state_before_recreation() {
    let fixture = Fixture::new();
    fixture.call(
        "context_sync",
        json!({"by":"user","ops":[
            {"op":"task.create","title":"owned","status":"active","assignee":"leaving","handoff_to":"staying","notes":"work already started"},
            {"op":"task.create","title":"handoff only","status":"planned","assignee":"staying","handoff_to":"leaving"},
            {"op":"task.create","title":"both references","status":"active","assignee":"leaving","handoff_to":"leaving"},
            {"op":"task.create","title":"finished","status":"done","assignee":"leaving","handoff_to":"leaving"},
            {"op":"task.create","title":"archived","status":"archived","assignee":"leaving","handoff_to":"leaving"},
            {"op":"task.create","title":"unrelated","assignee":"staying"},
            {"op":"agent_state.update","actor_id":"leaving","focus":"old generation","active_task_id":"T001","commitments":["old promise"]},
            {"op":"agent_state.update","actor_id":"staying","focus":"keep this state"}
        ]}),
    );
    let before = fixture.context();
    let path = GroupStore::new(fixture.home.clone())
        .expect("groups")
        .ledger_path(&fixture.group_id)
        .expect("ledger path");
    let history = ledger::read_all(&path).expect("history");

    fixture.call("actor_remove", json!({"actor_id":"leaving","by":"user"}));
    let after = fixture.context();
    let task = |title: &str| {
        after
            .tasks
            .iter()
            .find(|task| task["title"] == title)
            .expect("task")
    };
    assert_eq!(task("owned")["assignee"], Value::Null);
    assert_eq!(task("owned")["handoff_to"], "staying");
    assert_eq!(task("owned")["status"], "active");
    assert_eq!(task("owned")["notes"], "work already started");
    assert_eq!(task("handoff only")["handoff_to"], Value::Null);
    assert_eq!(task("handoff only")["assignee"], "staying");
    assert_eq!(task("handoff only")["status"], "planned");
    assert_eq!(task("both references")["assignee"], Value::Null);
    assert_eq!(task("both references")["handoff_to"], Value::Null);
    for title in ["owned", "handoff only", "both references"] {
        assert_eq!(task(title)["rev"], 2, "one semantic update for {title}");
    }
    for title in ["finished", "archived", "unrelated"] {
        assert_eq!(
            Some(task(title)),
            before.tasks.iter().find(|task| task["title"] == title),
            "preserve {title}"
        );
    }
    assert_eq!(after.agent_states["leaving"]["hot"], json!({}));
    assert_eq!(after.agent_states["leaving"]["warm"], json!({}));
    assert_eq!(
        after.agent_states["staying"],
        before.agent_states["staying"]
    );
    assert!(
        ledger::read_all(&path)
            .expect("retained history")
            .starts_with(&history)
    );

    fixture.add_actor("leaving");
    let recreated = fixture.context();
    assert_eq!(recreated.agent_states["leaving"]["hot"], json!({}));
    assert_eq!(recreated.agent_states["leaving"]["warm"], json!({}));
    assert!(recreated.tasks.iter().all(|task| {
        !matches!(task["status"].as_str(), Some("planned" | "active"))
            || task.get("handoff_to").and_then(Value::as_str) != Some("leaving")
    }));
}

#[test]
fn removed_actor_working_state_is_cleared_without_any_open_tasks() {
    let fixture = Fixture::new();
    fixture.call(
        "context_sync",
        json!({"by":"user","ops":[
            {"op":"agent_state.update","actor_id":"leaving","focus":"old focus","environment_summary":"old environment"}
        ]}),
    );
    fixture.call("actor_remove", json!({"actor_id":"leaving","by":"user"}));
    let after = fixture.context();
    assert!(after.tasks.is_empty());
    assert_eq!(after.agent_states["leaving"]["hot"], json!({}));
    assert_eq!(after.agent_states["leaving"]["warm"], json!({}));
}

#[test]
fn rejected_actor_removal_preserves_context() {
    let fixture = Fixture::new();
    fixture.call(
        "context_sync",
        json!({"by":"user","ops":[
            {"op":"task.create","title":"keep handoff","assignee":"staying","handoff_to":"coordinator"},
            {"op":"agent_state.update","actor_id":"coordinator","focus":"keep this state"}
        ]}),
    );
    let before = fixture.context();
    let rejected = fixture.request(
        "actor_remove",
        json!({"actor_id":"coordinator","by":"coordinator"}),
    );
    assert!(!rejected.ok);
    let after = fixture.context();
    assert_eq!(after.tasks, before.tasks);
    assert_eq!(after.agent_states, before.agent_states);
}
