//! PATH changes keep a stopped conversation, but never adopt or stop a live one.
use super::*;

const LIVE_SHORT: &str = "fedcba98";

impl Fixture {
    fn change_path(&mut self) -> String {
        let extra = self._temp.path().join("new-bin");
        std::fs::create_dir(&extra).expect("new search directory");
        let path = format!("{}:{}", extra.display(), self.actor.env["PATH"]);
        self.actor.env.insert("PATH".into(), path);
        self.group.actors[0] = self.actor.clone();
        GroupStore::new(self.home.clone())
            .expect("store")
            .save(&self.group)
            .expect("updated actor");
        extra.to_string_lossy().into_owned()
    }

    fn publish_live_at_launch(&self, busy: bool) {
        cccc_core::fs::write_json(
            &self.config.join("publish_saved_live.json"),
            &json!({
                "receipt_path": self.receipt_path(),
                "state": {
                "sessionId": SAVED, "daemonShort": LIVE_SHORT,
                "cwd": self.group.scopes[0].url,
                "tempo": if busy { "working" } else { "idle" },
                "inFlight": {"tasks": usize::from(busy), "queued": 0},
                "intent": "original work", "outcome": null,
                },
            }),
        )
        .expect("job to appear after prepare");
    }

    fn launch_arguments(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.config.join("argv.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("launch argv"))
            .collect()
    }

    fn assert_old_job_untouched(&self) {
        assert!(
            self.config.join("saved_live").exists(),
            "old worker still lives"
        );
        let requests = std::fs::read_to_string(self.config.join("control_requests.jsonl"))
            .expect("control operations");
        for line in requests.lines() {
            let request: Value = serde_json::from_str(line).expect("control request");
            assert!(
                request["short"] != LIVE_SHORT || request["op"] == "list",
                "old job received {request}"
            );
        }
    }
}

#[test]
fn changed_path_cold_resume_uses_the_saved_id_and_new_settings_without_a_notice() {
    if isolated(
        "path_only::changed_path_cold_resume_uses_the_saved_id_and_new_settings_without_a_notice",
    ) {
        return;
    }
    let mut f = Fixture::new("healthy");
    let extra = f.change_path();
    let started = f.lifecycle("actor_start");
    assert!(started.ok, "{:?}", started.error);
    assert_eq!(f.launch_arguments(), [vec!["--bg", "--resume", SAVED]]);
    let receipt = f.receipt();
    assert_eq!(receipt["provider_session_id"], SAVED);
    assert_eq!(receipt["captured_from"], "claude_agent_view_resume");
    assert!(receipt.get("replaced").is_none());
    let files = std::fs::read_dir(f.home.daemon_dir().join("claude-managed"))
        .expect("private settings")
        .map(|entry| entry.expect("settings file").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    let settings: Value = cccc_core::fs::read_json(&files[0]).expect("prepared settings");
    let context: Value = serde_json::from_str(
        std::fs::read_to_string(f.config.join("launch_contexts"))
            .expect("launch context")
            .lines()
            .next()
            .expect("one launch"),
    )
    .expect("context JSON");
    assert!(
        settings["env"]["PATH"]
            .as_str()
            .expect("settings PATH")
            .contains(&extra)
    );
    assert_eq!(settings["env"]["PATH"], context["path"]);
    let store = GroupStore::new(f.home.clone()).expect("store");
    let events =
        ledger::read_all(&store.ledger_path(&f.group.group_id).expect("ledger")).expect("events");
    assert!(
        !events.iter().any(|event| {
            event.kind == "system.notify"
                && event
                    .data
                    .get("context")
                    .is_some_and(|context| context["kind"] == "runtime_session_replaced")
        }),
        "resuming did not replace a conversation"
    );
    f.assert_no_private_output(&started);
}

#[test]
fn changed_path_does_not_adopt_or_stop_a_live_job_appearing_after_prepare() {
    if isolated("path_only::changed_path_does_not_adopt_or_stop_a_live_job_appearing_after_prepare")
    {
        return;
    }
    for busy in [false, true] {
        let mut f = Fixture::new("healthy");
        f.change_path();
        f.publish_live_at_launch(busy);
        let started = f.lifecycle("actor_start");
        assert!(started.ok, "{:?}", started.error);
        let arguments = f.launch_arguments();
        assert_eq!(arguments.len(), 1);
        assert!(!arguments[0].iter().any(|arg| arg == "--resume"));
        assert_ne!(f.receipt()["provider_session_id"], SAVED);
        assert_eq!(f.receipt()["replaced"]["reasons"], json!(["launch_path"]));
        f.assert_old_job_untouched();
        // Dropping this Actor stops only its new job, never the old one.
        let stopped = f.lifecycle("actor_stop");
        assert!(stopped.ok, "{:?}", stopped.error);
        f.assert_old_job_untouched();
    }
}

#[test]
fn fresh_failure_after_a_path_change_does_not_pause_the_old_conversation() {
    if isolated("path_only::fresh_failure_after_a_path_change_does_not_pause_the_old_conversation")
    {
        return;
    }
    let mut f = Fixture::new("startup_failure");
    f.change_path();
    f.publish_live_at_launch(false);
    let started = f.lifecycle("actor_start");
    assert!(!started.ok);
    let receipt = f.receipt();
    assert_eq!(receipt["provider_session_id"], SAVED);
    assert_eq!(receipt["status"], "usable");
    assert_eq!(receipt["failure_count"], 0);
    assert_eq!(receipt["last_resume_error"], "");
    assert_eq!(f.launch_arguments().len(), 1);
    assert!(!f.launch_arguments()[0].iter().any(|arg| arg == "--resume"));
    f.assert_old_job_untouched();
}

#[test]
fn a_path_change_does_not_hide_control_errors_or_copied_and_missing_history() {
    if isolated(
        "path_only::a_path_change_does_not_hide_control_errors_or_copied_and_missing_history",
    ) {
        return;
    }
    for mode in ["control_error", "copied", "missing_history"] {
        let mut f = Fixture::new(mode);
        f.change_path();
        if mode == "control_error" {
            std::fs::write(f.config.join("deny_control"), "deny").expect("control refusal");
        }
        let started = f.lifecycle("actor_start");
        assert!(!started.ok, "{mode} should fail");
        let receipt = f.receipt();
        assert_eq!(receipt["provider_session_id"], SAVED, "{mode}");
        assert_eq!(receipt["status"], "resume_failed", "{mode}");
        assert_eq!(receipt["failure_count"], 1, "{mode}");
        let arguments = f.launch_arguments();
        if mode == "control_error" {
            assert!(
                arguments.is_empty(),
                "a failed lookup must not launch fresh"
            );
        } else {
            assert_eq!(arguments, [vec!["--bg", "--resume", SAVED]]);
        }
        assert!(receipt.get("replaced").is_none(), "{mode}");
    }
}
