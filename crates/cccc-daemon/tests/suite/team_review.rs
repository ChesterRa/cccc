use cccc_contracts::{DaemonRequest, DaemonResponse, Event};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Value, json};

const SKILL: &str = "skill:cccc:self-evolution";

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
        let created = fixture.call("group_create", json!({"title":"Team review"}));
        fixture.group_id = created["group"]["group_id"]
            .as_str()
            .expect("group id")
            .into();
        for actor_id in ["lead", "peer"] {
            fixture.call(
                "actor_add",
                json!({"actor_id":actor_id,"runtime":"custom","command":["sh"],"enabled":false,"by":"user"}),
            );
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

    fn write_task(&self, by: &str, title: &str, status: &str) -> Value {
        self.call(
            "context_sync",
            json!({"by":by,"ops":[{"op":"task.create","title":title,"status":status}]}),
        )
    }

    fn finish_tasks(&self, count: usize) {
        for index in 0..count {
            self.write_task("user", &format!("finished {index}"), "done");
        }
    }

    fn append(&self, event: &Event) {
        let path = GroupStore::new(self.home.clone())
            .expect("groups")
            .ledger_path(&self.group_id)
            .expect("ledger");
        ledger::append(&path, event).expect("append");
    }

    fn message(&self, by: &str, to: &[&str], text: &str) -> Event {
        let mut event = Event::new("chat.message", &self.group_id);
        event.by = by.into();
        event.data = json!({"to":to,"text":text,"message_mode":"send"})
            .as_object()
            .expect("data")
            .clone();
        event
    }
}

fn reminded(result: &Value) -> bool {
    result.get("review_reminder").is_some()
}

fn member<'a>(activity: &'a Value, actor_id: &str) -> &'a Value {
    activity["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|row| row["actor_id"] == actor_id)
        .expect("member")
}

#[test]
fn a_foreman_hears_of_a_review_once_per_ten_finished_top_level_tasks() {
    let fixture = Fixture::new();
    fixture.finish_tasks(9);
    let parent = fixture.write_task("user", "parent", "active")["changes"][0]["task_id"].clone();
    fixture.call(
        "context_sync",
        json!({"by":"user","ops":[
            {"op":"task.create","title":"subtask","status":"done","parent_id":parent}
        ]}),
    );
    assert!(!reminded(&fixture.write_task("lead", "nine", "planned")));

    fixture.finish_tasks(1);
    assert!(!reminded(&fixture.write_task(
        "peer",
        "peers hear nothing",
        "planned"
    )));
    assert!(!reminded(&fixture.call(
        "context_sync",
        json!({"by":"lead","dry_run":true,"ops":[{"op":"task.create","title":"preview"}]}),
    )));

    let result = fixture.write_task("lead", "ten", "planned");
    let text = result["review_reminder"].as_str().expect("reminder");
    assert!(text.starts_with("10 top-level tasks"), "{text}");
    assert!(text.contains(SKILL), "{text}");
    assert!(!reminded(&fixture.write_task("lead", "once", "planned")));

    fixture.finish_tasks(10);
    assert!(reminded(&fixture.write_task("lead", "twenty", "planned")));
}

#[test]
fn archiving_finished_tasks_does_not_postpone_the_next_review() {
    let fixture = Fixture::new();
    fixture.finish_tasks(10);
    assert!(reminded(&fixture.write_task("lead", "ten", "planned")));

    let archived = (1..=10)
        .map(|index| json!({"op":"task.move","task_id":format!("T{index:03}"),"status":"archived"}))
        .collect::<Vec<_>>();
    fixture.call("context_sync", json!({"by":"user","ops":archived}));
    let restored = (1..=10)
        .map(|index| json!({"op":"task.restore","task_id":format!("T{index:03}")}))
        .collect::<Vec<_>>();
    fixture.call("context_sync", json!({"by":"user","ops":restored}));
    assert!(!reminded(&fixture.write_task(
        "lead",
        "restored is not new",
        "planned"
    )));

    fixture.finish_tasks(10);
    assert!(reminded(&fixture.write_task("lead", "ten more", "planned")));
}

#[test]
fn a_task_finished_while_another_is_archived_still_counts() {
    let fixture = Fixture::new();
    fixture.finish_tasks(9);
    fixture.write_task("user", "open", "active");
    fixture.call(
        "context_sync",
        json!({"by":"user","ops":[
            {"op":"task.move","task_id":"T001","status":"archived"},
            {"op":"task.move","task_id":"T010","status":"done"},
        ]}),
    );
    assert!(reminded(&fixture.write_task("lead", "ten", "planned")));
}

#[test]
fn only_a_task_write_carries_the_review_reminder() {
    let fixture = Fixture::new();
    fixture.finish_tasks(10);
    assert!(!reminded(&fixture.call(
        "context_sync",
        json!({"by":"lead","ops":[{"op":"coordination.brief.update","objective":"ship"}]}),
    )));
    assert!(reminded(&fixture.write_task("lead", "task", "planned")));
}

#[test]
fn a_skill_the_user_turned_off_gets_no_reminder() {
    let turned_off = [
        (
            "capability_block",
            json!({"capability_id":SKILL,"scope":"group","blocked":true,"by":"user"}),
        ),
        (
            "capability_enable",
            json!({"capability_id":SKILL,"scope":"group","enabled":false,"by":"user"}),
        ),
        (
            "capability_uninstall",
            json!({"capability_id":SKILL,"scope":"group","by":"user"}),
        ),
        (
            "actor_update",
            json!({"actor_id":"lead","by":"user","patch":{"capability_hidden":[SKILL]}}),
        ),
    ];
    for (op, args) in turned_off {
        let fixture = Fixture::new();
        fixture.call("capability_state", json!({"actor_id":"lead","by":"lead"}));
        fixture.call(op, args);
        fixture.finish_tasks(10);
        assert!(
            !reminded(&fixture.write_task("lead", "turned off", "planned")),
            "{op}"
        );
    }
}

#[test]
fn history_can_read_one_senders_messages() {
    let fixture = Fixture::new();
    fixture.append(&fixture.message("user", &["lead"], "from the user"));
    fixture.append(&fixture.message("peer", &["lead"], "from a peer"));
    let read = |args: Value| fixture.request("message_history", args);

    let result = read(json!({"actor_id":"lead","by":"lead","sender":"user"}));
    assert!(result.ok, "{:?}", result.error);
    let messages = result.result["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["data"]["text"], "from the user");

    let exact = read(json!({"actor_id":"lead","by":"lead","sender":"user","event_id":"e"}));
    assert!(!exact.ok);
    assert_eq!(exact.error.expect("error").code, "invalid_args");
}

#[test]
fn activity_counts_each_member_inside_the_window() {
    let fixture = Fixture::new();
    fixture.append(&fixture.message("lead", &["user"], "report"));
    fixture.append(&fixture.message("lead", &["peer"], "ask"));
    fixture.append(&fixture.message("user", &["lead"], "thanks"));
    let mut old = fixture.message("peer", &["lead"], "long ago");
    old.ts = "2020-01-01T00:00:00Z".into();
    fixture.append(&old);
    for (actor_id, kind) in [("peer", "automation"), ("peer", "mail_notice")] {
        let mut notice = Event::new("system.notify", &fixture.group_id);
        notice.data = json!({"kind":kind,"target_actor_id":actor_id})
            .as_object()
            .expect("data")
            .clone();
        fixture.append(&notice);
    }
    fixture.call(
        "context_sync",
        json!({"by":"user","ops":[
            {"op":"task.create","title":"done","status":"done","assignee":"peer"},
            {"op":"task.create","title":"active","status":"active","assignee":"peer"},
            {"op":"task.create","title":"planned","status":"planned","assignee":"peer"},
        ]}),
    );

    let result = fixture.call("group_activity", json!({"by":"lead"}));

    assert_eq!(result["days"], 14);
    assert_eq!(result["user"]["messages_sent"], 1);
    assert_eq!(member(&result, "lead")["messages_sent"], 2);
    assert_eq!(member(&result, "lead")["messages_to_user"], 1);
    assert_eq!(member(&result, "peer")["messages_sent"], 0);
    assert_eq!(member(&result, "peer")["scheduled_notices"], 1);
    assert_eq!(member(&result, "peer")["tasks_open"], 2);
}

#[test]
fn activity_scheduled_notices_exclude_manual_runs_and_keep_legacy_notices() {
    let fixture = Fixture::new();
    for run_kind in [None, Some("scheduled"), Some("manual")] {
        let mut notice = Event::new("system.notify", &fixture.group_id);
        let mut data = json!({
            "kind":"automation", "target_actor_id":"peer", "context":{"rule_id":"review"}
        });
        if let Some(kind) = run_kind {
            data["context"]["run_kind"] = json!(kind);
        }
        notice.data = data.as_object().expect("data").clone();
        fixture.append(&notice);
    }

    for reader in ["user", "lead"] {
        let result = fixture.call("group_activity", json!({"by":reader}));
        assert_eq!(member(&result, "peer")["scheduled_notices"], 2);
    }
}

#[test]
fn activity_counts_only_messages_the_reader_may_see() {
    let fixture = Fixture::new();
    fixture.append(&fixture.message("peer", &["user"], "private to the user"));
    fixture.append(&fixture.message("user", &["peer"], "private to the peer"));
    fixture.append(&fixture.message("peer", &["lead"], "to the foreman"));

    let foreman = fixture.call("group_activity", json!({"by":"lead"}));
    assert_eq!(member(&foreman, "peer")["messages_sent"], 1);
    assert_eq!(member(&foreman, "peer")["messages_to_user"], 0);
    assert_eq!(foreman["user"]["messages_sent"], 0);

    let user = fixture.call("group_activity", json!({"by":"user"}));
    assert_eq!(member(&user, "peer")["messages_sent"], 2);
    assert_eq!(user["user"]["messages_sent"], 1);
}

#[test]
fn activity_does_not_credit_a_former_member_to_a_new_one_with_the_same_id() {
    let fixture = Fixture::new();
    fixture.append(&fixture.message("peer", &["lead"], "former member"));
    fixture.call("actor_remove", json!({"actor_id":"peer","by":"user"}));
    fixture.call(
        "actor_add",
        json!({"actor_id":"peer","runtime":"custom","command":["sh"],"enabled":false,"by":"user"}),
    );
    fixture.append(&fixture.message("peer", &["lead"], "new member"));

    for by in ["lead", "user"] {
        let result = fixture.call("group_activity", json!({"by":by}));
        assert_eq!(member(&result, "peer")["messages_sent"], 1, "{by}");
    }
}

#[test]
fn activity_is_for_the_foreman_and_the_user() {
    let fixture = Fixture::new();
    assert!(fixture.request("group_activity", json!({"by":"user"})).ok);
    assert!(!fixture.request("group_activity", json!({"by":"peer"})).ok);
    for days in [json!(0), json!(366), json!("7")] {
        let result = fixture.request("group_activity", json!({"by":"lead","days":days}));
        assert_eq!(result.error.expect("error").code, "invalid_args");
    }
}
