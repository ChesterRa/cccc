use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, DaemonRequest, DaemonResponse};
use cccc_core::profiles::ProfileStore;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Reads CCCC's first line and the message after it, each ended by the submit key.
const READER: &str = "stty -icanon -echo; IFS= read -r preamble; IFS= read -r message; \
    printf '%s\\n' \"$message\" > received; exec sleep 30";

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group_id: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let project = temp.path().join("project");
        std::fs::create_dir(&project).expect("project");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let mut fixture = Self {
            _temp: temp,
            home,
            group_id: String::new(),
        };
        let created = fixture.call("group_create", json!({"title":"Profile launch"}));
        fixture.group_id = created["group"]["group_id"]
            .as_str()
            .expect("group id")
            .into();
        fixture.call("attach", json!({"path":project,"by":"user"}));
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

    fn save_profile(&self, runtime: &str, command: &str, submit: &str) -> u64 {
        let saved = ProfileStore::new(self.home.clone())
            .expect("profiles")
            .upsert(
                json!({
                    "id": "shared",
                    "name": "Shared",
                    "runtime": runtime,
                    "command": ["sh", "-c", command],
                    "submit": submit,
                })
                .as_object()
                .cloned()
                .expect("profile"),
                None,
            )
            .expect("profile");
        saved["revision"].as_u64().expect("revision")
    }

    fn add_linked_member(&self) {
        self.call(
            "actor_add",
            json!({"actor_id":"member","profile_id":"shared","profile_scope":"global","by":"user"}),
        );
    }

    fn start_reader(&self) {
        self.call(
            "actor_add",
            json!({
                "actor_id":"member",
                "runtime":"custom",
                "submit":"newline",
                "command":["sh","-c",READER],
                "by":"user",
            }),
        );
        self.call("actor_start", json!({"actor_id":"member","by":"user"}));
    }

    fn save_for_next_launch(&self, patch: Value) {
        self.call(
            "actor_update",
            json!({"actor_id":"member","patch":patch,"by":"user"}),
        );
    }

    fn send(&self, text: &str) -> String {
        let sent = self.call(
            "send",
            json!({"by":"user","to":["member"],"text":text,"message_mode":"send"}),
        );
        sent["event"]["id"].as_str().expect("event id").to_owned()
    }

    /// The channel that recorded the message as accepted.
    fn accepted_on(&self, event_id: &str) -> String {
        let ledger_path = GroupStore::new(self.home.clone())
            .expect("store")
            .ledger_path(&self.group_id)
            .expect("ledger path");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let accepted = ledger::read_all(&ledger_path)
                .expect("ledger")
                .into_iter()
                .find(|event| {
                    event.kind == "runtime.delivery"
                        && event.data["source_event_id"] == event_id
                        && event.data["state"] == "accepted"
                });
            if let Some(accepted) = accepted {
                return accepted.data["transport"]
                    .as_str()
                    .expect("transport")
                    .to_owned();
            }
            assert!(Instant::now() < deadline, "no accepted delivery");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn received(&self) -> String {
        let path = self._temp.path().join("project/received");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(text) = std::fs::read_to_string(&path)
                && text.ends_with('\n')
            {
                return text;
            }
            assert!(Instant::now() < deadline, "the terminal read no message");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn member(&self) -> Actor {
        GroupStore::new(self.home.clone())
            .expect("store")
            .load(&self.group_id)
            .expect("group")
            .actors
            .into_iter()
            .find(|actor| actor.id == "member")
            .expect("member")
    }

    fn group_text(&self) -> String {
        let root = self.home.root().join("groups").join(&self.group_id);
        let mut text = String::new();
        for entry in std::fs::read_dir(root).expect("group directory") {
            let path = entry.expect("entry").path();
            if path.is_file() {
                text.push_str(&String::from_utf8_lossy(
                    &std::fs::read(path).expect("file"),
                ));
            }
        }
        text
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.request("actor_stop", json!({"actor_id":"member","by":"user"}));
    }
}

#[test]
fn a_member_takes_its_profile_at_the_launch_that_replaces_the_owner() {
    let fixture = Fixture::new();
    let first = fixture.save_profile("custom", "exec sleep 30", "enter");
    ProfileStore::new(fixture.home.clone())
        .expect("profiles")
        .update_secrets(
            "shared",
            json!({"PROFILE_ONLY":"never-in-the-group"})
                .as_object()
                .expect("private values"),
            &[],
            false,
        )
        .expect("private value");
    fixture.add_linked_member();
    fixture.call("actor_start", json!({"actor_id":"member","by":"user"}));
    let launched = fixture.member();
    assert_eq!(launched.profile_revision_applied, first);
    assert_eq!(launched.submit, ActorSubmit::Enter);

    // Saving the Profile leaves the running member alone, and so does a Start
    // that finds it running.
    let second = fixture.save_profile("custom", "exec sleep 31", "newline");
    assert_ne!(first, second);
    assert_eq!(fixture.member().command, launched.command);
    fixture.call("actor_start", json!({"actor_id":"member","by":"user"}));
    let untouched = fixture.member();
    assert_eq!(untouched.command, launched.command);
    assert_eq!(untouched.profile_revision_applied, first);

    fixture.call("actor_restart", json!({"actor_id":"member","by":"user"}));
    let adopted = fixture.member();
    assert_eq!(adopted.command, ["sh", "-c", "exec sleep 31"]);
    assert_eq!(adopted.submit, ActorSubmit::Newline);
    assert_eq!(adopted.profile_revision_applied, second);
    assert!(
        !fixture.group_text().contains("never-in-the-group"),
        "private Profile values stay out of the Group"
    );
}

#[test]
fn a_running_terminal_stays_reachable_until_a_launch_replaces_it() {
    let fixture = Fixture::new();
    fixture.call(
        "actor_add",
        json!({
            "actor_id":"member",
            "runtime":"custom",
            "command":["sh","-c","printf ready; exec sleep 30"],
            "by":"user",
        }),
    );
    fixture.call("actor_start", json!({"actor_id":"member","by":"user"}));

    // Saved for the next launch: nothing replaced the terminal yet.
    fixture.call(
        "actor_update",
        json!({"actor_id":"member","patch":{"runtime":"web_model"},"by":"user"}),
    );
    fixture.call("terminal_tail", json!({"actor_id":"member","by":"user"}));

    fixture.call("actor_restart", json!({"actor_id":"member","by":"user"}));
    let replaced = fixture.request("terminal_tail", json!({"actor_id":"member","by":"user"}));
    assert_eq!(replaced.error.expect("no terminal").code, "not_pty_actor");
}

#[test]
fn messages_reach_the_terminal_that_runs_whatever_was_saved_since() {
    for runtime in ["web_model", "deepseek"] {
        let fixture = Fixture::new();
        fixture.start_reader();
        fixture.save_for_next_launch(json!({"runtime":runtime}));

        let event = fixture.send("FOR_THE_RUNNING_TERMINAL");

        assert_eq!(fixture.accepted_on(&event), "pty", "saved as {runtime}");
        assert!(fixture.received().contains("FOR_THE_RUNNING_TERMINAL"));
    }
}

#[test]
fn a_terminal_keeps_the_submit_key_it_launched_with() {
    let fixture = Fixture::new();
    fixture.start_reader();
    fixture.save_for_next_launch(json!({"submit":"none"}));

    let event = fixture.send("SUBMITTED_AS_LAUNCHED");

    assert_eq!(fixture.accepted_on(&event), "pty");
    assert!(fixture.received().contains("SUBMITTED_AS_LAUNCHED"));
}

#[test]
fn messages_follow_a_profile_that_changed_runtime_before_start() {
    let fixture = Fixture::new();
    fixture.save_profile("deepseek", "exec sleep 30", "enter");
    fixture.add_linked_member();
    fixture.save_profile("custom", READER, "newline");
    fixture.call("actor_start", json!({"actor_id":"member","by":"user"}));

    let event = fixture.send("FOR_THE_NEW_TERMINAL");

    assert_eq!(fixture.accepted_on(&event), "pty");
    assert!(fixture.received().contains("FOR_THE_NEW_TERMINAL"));
}

#[test]
fn a_profile_that_became_a_web_model_starts_without_a_terminal() {
    let fixture = Fixture::new();
    fixture.save_profile("custom", "exec sleep 30", "enter");
    fixture.add_linked_member();
    fixture.save_profile("web_model", "exec sleep 30", "enter");

    fixture.call("actor_start", json!({"actor_id":"member","by":"user"}));

    assert_eq!(fixture.member().runtime, ActorRuntime::WebModel);
    let event = fixture.send("WAITS_FOR_ITS_CONNECTION");
    let listed = fixture.call("actor_list", json!({"by":"user"}));
    assert_eq!(listed["actors"][0]["runtime"], "web_model", "{listed}");
    let turn = fixture.call(
        "runtime_wait_next_turn",
        json!({"actor_id":"member","by":"member","transport":"web_model_browser"}),
    );
    assert_eq!(turn["turn"]["event_ids"], json!([event]), "{turn}");
    assert!(
        !fixture.group_text().contains("not running"),
        "a Web Model waiting for its connection is not reported as stopped"
    );
}

#[test]
fn a_launch_that_ends_a_web_model_settles_the_turn_it_held() {
    let fixture = Fixture::new();
    fixture.save_profile("web_model", "exec sleep 30", "enter");
    fixture.add_linked_member();
    fixture.call("actor_start", json!({"actor_id":"member","by":"user"}));
    let event = fixture.send("CLAIMED_BEFORE_THE_NEW_TERMINAL");
    let turn = fixture.call(
        "runtime_wait_next_turn",
        json!({"actor_id":"member","by":"member","transport":"web_model_browser"}),
    );
    assert_eq!(turn["status"], "work_available", "{turn}");
    fixture.save_profile("custom", READER, "newline");

    fixture.call("actor_start", json!({"actor_id":"member","by":"user"}));

    // Never submitted by the browser, so the terminal receives it.
    assert_eq!(fixture.member().runtime, ActorRuntime::Custom);
    assert_eq!(fixture.accepted_on(&event), "pty");
    assert!(
        fixture
            .received()
            .contains("CLAIMED_BEFORE_THE_NEW_TERMINAL")
    );
}

#[test]
fn a_pull_waits_for_the_terminal_that_still_runs() {
    let fixture = Fixture::new();
    fixture.start_reader();
    fixture.save_for_next_launch(json!({"runtime":"web_model"}));
    let pull = || {
        fixture.call(
            "runtime_wait_next_turn",
            json!({"actor_id":"member","by":"member","transport":"web_model_pull"}),
        )["status"]
            .clone()
    };

    assert_eq!(pull(), "stopped");

    fixture.call("actor_restart", json!({"actor_id":"member","by":"user"}));
    assert_eq!(pull(), "idle");
}
