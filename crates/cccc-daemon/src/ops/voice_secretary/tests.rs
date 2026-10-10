use super::*;
use cccc_core::GroupStore;
use std::collections::BTreeMap;
#[cfg(unix)]
#[path = "fixture_tests.rs"]
mod fixture_tests;
#[cfg(unix)]
#[path = "interaction_fixture_tests.rs"]
mod interaction_fixture_tests;
#[cfg(unix)]
#[path = "structured_fixture_tests.rs"]
mod structured_fixture_tests;

// State/admission fixtures exercise the real startup reconciliation with workers
// paused. Execution fixtures below use their offline providers and normal workers.
fn start_unit_owner(home: &HomeLayout) -> io::Result<()> {
    let profile = settings::load(home)?.voice_secretary.profile_id;
    settings::update(home, |s| {
        s.voice_secretary.profile_id.clear();
        Ok(())
    })?;
    let started = start(home, None);
    if let Some(manager) = lookup(home) {
        let workers =
            std::mem::take(&mut *manager.workers.lock().unwrap_or_else(|e| e.into_inner()));
        block_on(async {
            for worker in workers {
                worker.abort();
                let _ = worker.await;
            }
        });
    }
    settings::update(home, |s| {
        s.voice_secretary.profile_id = profile;
        Ok(())
    })?;
    started?;
    lookup(home).expect("owner").reconcile_sources()
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("init");
        // Admission/state fixtures have no provider worker. Separate execution
        // tests run the complete lifecycle against an offline provider.
        let profile = ProfileStore::new(home.clone())
            .expect("profiles")
            .upsert(
                json!({"name":"Fixture","runtime":"codex","command":["fixture-codex"]})
                    .as_object()
                    .expect("profile")
                    .clone(),
                None,
            )
            .expect("profile");
        settings::update(&home, |s| {
            s.voice_secretary.profile_id = profile["id"].as_str().expect("id").into();
            Ok(())
        })
        .expect("settings");
        start_unit_owner(&home).expect("isolated daemon owner");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("A", "")
            .expect("group")
            .group_id;
        Self {
            _temp: temp,
            home,
            group,
        }
    }
    fn ask(&self, text: &str, key: &str) -> SecretaryTask {
        request(
            &self.home,
            "assistant_voice_document_instruction",
            json!({"group_id":self.group,"instruction":text,"input_append_id":key}),
        )
        .expect("Ask accepted");
        task_store(&self.home)
            .list()
            .expect("tasks")
            .pop()
            .expect("task")
    }
    fn grant(&self, task: &SecretaryTask) -> String {
        let manager = lookup(&self.home).expect("manager");
        let generation = uuid::Uuid::new_v4().simple().to_string();
        manager
            .store
            .update(&task.task_id, |t| {
                t.phase = SecretaryTaskPhase::Running;
                t.generation = generation.clone();
                Ok(())
            })
            .expect("admission");
        let (cancel, _receiver) = watch::channel(false);
        let execution = Arc::new(Execution {
            task_id: task.task_id.clone(),
            cancel,
            generation: OnceLock::from(generation),
            attempted: AtomicBool::new(true),
            session: Mutex::new(None),
            startup_cleanup: Mutex::new(None),
            quiescent: AtomicBool::new(false),
            progress: Mutex::new(terminal::Progress::default()),
            activity: Mutex::new(String::new()),
            submitted: Notify::new(),
        });
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        manager
            .grants
            .lock()
            .expect("grants")
            .insert(digest(token.as_bytes()), execution);
        token
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(manager) = lookup(&self.home) {
            manager.grants.lock().expect("grants").clear();
        }
        stop(&self.home).expect("cleanup isolated owner");
    }
}

fn request(home: &HomeLayout, op: &str, mut args: Value) -> OpResult {
    if op == "voice_secretary_task"
        && args.get("task_id").is_none()
        && let Some(token) = args["_cccc_secretary_token"].as_str()
        && let Some(owner) = lookup(home)
        && let Some(execution) = owner
            .grants
            .lock()
            .expect("grants")
            .get(&digest(token.as_bytes()))
    {
        args["task_id"] = json!(execution.task_id);
    }
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("args").clone(),
    };
    crate::ops::resolve_operation(&request)
        .expect("operation")
        .execute(home, &request)
}

fn assert_readiness(
    fixture: &Fixture,
    configured: bool,
    ready: bool,
    code: Option<&str>,
    lifecycle: &str,
) -> Value {
    let settings =
        request(&fixture.home, "voice_secretary_settings_get", json!({})).expect("settings read");
    let tasks = request(
        &fixture.home,
        "voice_secretary_tasks",
        json!({"group_id":fixture.group}),
    )
    .expect("tasks read");
    let health = group_projection(&fixture.home, &fixture.group).expect("health read");
    let runtime =
        request(&fixture.home, "voice_secretary_runtime", json!({})).expect("runtime read");
    let assistant = request(
        &fixture.home,
        "assistant_index",
        json!({"group_id":fixture.group}),
    )
    .expect("assistant read");
    for projection in [
        &settings,
        &tasks,
        health.as_object().expect("health object"),
        &runtime,
        assistant["assistant"]["health"]["secretary"]
            .as_object()
            .expect("assistant health"),
    ] {
        assert_eq!(projection["configured"], configured);
        assert_eq!(projection["ready"], ready);
        assert_eq!(projection["readiness_code"], json!(code));
        assert_eq!(projection["readiness_error"], settings["readiness_error"]);
    }
    assert_eq!(assistant["assistant"]["lifecycle"], lifecycle);
    json!({"settings":settings,"health":health,"runtime":runtime})
}

#[test]
fn readiness_separates_saved_configuration_from_owner_and_provider_phase() {
    let fixture = Fixture::new();
    let healthy = assert_readiness(&fixture, true, true, None, "ready");
    assert_eq!(healthy["runtime"]["phase"], "not_started");
    assert!(healthy["runtime"].get("generation").is_none());

    stop(&fixture.home).expect("stop owner");
    let unavailable = assert_readiness(
        &fixture,
        true,
        false,
        Some("owner_unavailable"),
        "unavailable",
    );
    assert_eq!(unavailable["health"]["status"], "unavailable");
    assert_eq!(unavailable["runtime"]["phase"], "unavailable");
    assert!(
        lookup(&fixture.home).is_none(),
        "reads cannot start an owner"
    );

    settings::update(&fixture.home, |settings| {
        settings.voice_secretary = Default::default();
        Ok(())
    })
    .expect("clear model configuration");
    let unconfigured = assert_readiness(
        &fixture,
        false,
        false,
        Some("not_configured"),
        "unconfigured",
    );
    assert_eq!(unconfigured["health"]["status"], "unconfigured");
    assert!(lookup(&fixture.home).is_none());
}

#[test]
fn readiness_reports_invalid_saved_configuration_without_private_values() {
    let fixture = Fixture::new();
    let profile = settings::load(&fixture.home)
        .expect("settings")
        .voice_secretary
        .profile_id;
    settings::update(&fixture.home, |settings| {
        settings.voice_secretary.profile_id = "missing-private-profile-marker".into();
        Ok(())
    })
    .expect("missing linked Profile");
    let invalid = assert_readiness(
        &fixture,
        true,
        false,
        Some("invalid_configuration"),
        "unavailable",
    );
    assert_eq!(invalid["health"]["status"], "unavailable");
    assert!(
        !invalid["settings"]["readiness_error"]
            .as_str()
            .expect("diagnostic")
            .contains("missing-private-profile-marker")
    );

    settings::update(&fixture.home, |settings| {
        settings.voice_secretary.profile_id = profile;
        Ok(())
    })
    .expect("restore linked Profile");
    let secret_path = fixture
        .home
        .root()
        .join("state/secrets/voice-secretary.json");
    std::fs::create_dir_all(secret_path.parent().expect("secret parent"))
        .expect("secret directory");
    let corrupt = br#"{"PRIVATE_KEY":{"synthetic-private-parser-value":true}}"#;
    std::fs::write(&secret_path, corrupt).expect("malformed private environment");
    let invalid = assert_readiness(
        &fixture,
        true,
        false,
        Some("invalid_configuration"),
        "unavailable",
    );
    let diagnostic = invalid["settings"]["readiness_error"]
        .as_str()
        .expect("diagnostic");
    assert!(diagnostic.contains("JSON"), "{diagnostic}");
    assert!(!diagnostic.contains("PRIVATE_KEY"), "{diagnostic}");
    assert!(!diagnostic.contains("synthetic-private"), "{diagnostic}");
    let update_error = request(
        &fixture.home,
        "voice_secretary_settings_update",
        json!({"settings":{"runtime":"codex"},"environment":{"clear":true}}),
    )
    .expect_err("private parser failures stay safe on writes too");
    assert_eq!(update_error.code, "invalid_configuration");
    assert_eq!(update_error.message, diagnostic);
    assert_eq!(std::fs::read(&secret_path).expect("private bytes"), corrupt);
    assert!(lookup(&fixture.home).is_some(), "reads preserve the owner");
}

#[test]
fn queued_configuration_failure_keeps_private_values_out_of_task_diagnostics() {
    let fixture = Fixture::new();
    let accepted = fixture.ask("Keep accepted work", "private-admission-source");
    let secret_path = fixture
        .home
        .root()
        .join("state/secrets/voice-secretary.json");
    std::fs::create_dir_all(secret_path.parent().expect("secret parent"))
        .expect("secret directory");
    let marker = "synthetic-private-queued-configuration-value";
    let corrupt = serde_json::to_vec(marker).expect("synthetic private fixture");
    std::fs::write(&secret_path, &corrupt).expect("malformed private environment");

    // Resume the actual owner after work was accepted. Runtime resolution must
    // fail before any provider launch, without publishing parser-quoted values.
    let manager = lookup(&fixture.home).expect("owner");
    let owner = Arc::clone(&manager);
    manager
        .workers
        .lock()
        .expect("workers")
        .push(managed_runtime().spawn(owner.run()));
    manager.notify();
    let stored = block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let task = manager
                    .store
                    .load(&accepted.task_id)
                    .expect("accepted task");
                if !task.diagnostic.is_empty() {
                    break task;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("owner records the configuration failure")
    });
    assert!(!stored.diagnostic.contains(marker));
    assert!(stored.diagnostic.contains("JSON"));
    assert!(
        stored
            .diagnostic
            .contains("resolving saved Runtime or Profile")
    );
    assert_eq!(stored.phase, SecretaryTaskPhase::Queued);
    assert!(
        stored.generation.is_empty(),
        "no provider launch was admitted"
    );

    let response = request(
        &fixture.home,
        "voice_secretary_tasks",
        json!({"group_id":fixture.group}),
    )
    .expect("retained task remains readable");
    assert_eq!(response["ready"], false);
    assert_eq!(response["readiness_code"], "invalid_configuration");
    let visible = response["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["task_id"] == accepted.task_id)
        .expect("accepted task remains visible");
    assert!(
        !serde_json::to_string(visible)
            .expect("task projection")
            .contains(marker)
    );
    assert!(
        !response["readiness_error"]
            .as_str()
            .expect("readiness diagnostic")
            .contains(marker)
    );
    assert_eq!(
        std::fs::read(&secret_path).expect("preserved private bytes"),
        corrupt
    );
}

#[test]
fn readiness_keeps_saved_settings_visible_when_the_profile_catalog_is_corrupt() {
    let fixture = Fixture::new();
    let path = fixture
        .home
        .root()
        .join("state/actor_profiles/profiles.json");
    let original = std::fs::read(&path).expect("Profile catalog");
    let corrupt = br#"{"profiles":"synthetic-private-catalog-parser-value"}"#;
    std::fs::write(&path, corrupt).expect("corrupt Profile catalog");
    let invalid = assert_readiness(
        &fixture,
        true,
        false,
        Some("invalid_configuration"),
        "unavailable",
    );
    assert!(invalid["settings"]["settings"]["profile_id"].is_string());
    assert_eq!(invalid["settings"]["profiles"], json!([]));
    let diagnostic = invalid["settings"]["readiness_error"]
        .as_str()
        .expect("diagnostic");
    assert!(diagnostic.contains("JSON"), "{diagnostic}");
    assert!(!diagnostic.contains("synthetic-private"), "{diagnostic}");
    assert_eq!(std::fs::read(&path).expect("retained catalog"), corrupt);
    std::fs::write(&path, original).expect("restore fixture");
}

#[test]
fn unreadable_settings_fail_safely_without_returning_default_configuration() {
    let fixture = Fixture::new();
    let retained_task = fixture.ask("Retain accepted work", "bad-settings-retained-task");
    let path = fixture.home.root().join("settings.yaml");
    let original = std::fs::read(&path).expect("saved settings");
    let corrupt = b"voice_secretary:\n  runtime: synthetic-private-settings-parser-value\n";
    std::fs::write(&path, corrupt).expect("corrupt settings");
    let error = request(&fixture.home, "voice_secretary_settings_get", json!({}))
        .expect_err("no default settings may replace unreadable authority");
    assert_eq!(error.code, "invalid_configuration");
    assert!(error.message.contains("YAML"), "{}", error.message);
    assert!(!error.message.contains("synthetic-private"));
    let assistant_error = request(
        &fixture.home,
        "assistant_index",
        json!({"group_id":fixture.group}),
    )
    .expect_err("assistant preferences cannot be invented");
    assert_eq!(assistant_error.code, "invalid_configuration");
    assert_eq!(assistant_error.message, error.message);
    for args in [
        json!({"settings":{"runtime":"codex"}}),
        json!({"preferences":{"recognition_language":"en-US"}}),
    ] {
        let update_error = request(&fixture.home, "voice_secretary_settings_update", args)
            .expect_err("unreadable settings cannot be overwritten");
        assert_eq!(update_error.code, "invalid_configuration");
        assert_eq!(update_error.message, error.message);
    }
    let health = group_projection(&fixture.home, &fixture.group).expect("health remains readable");
    let tasks = request(
        &fixture.home,
        "voice_secretary_tasks",
        json!({"group_id":fixture.group}),
    )
    .expect("tasks remain readable");
    let runtime = request(&fixture.home, "voice_secretary_runtime", json!({}))
        .expect("runtime remains readable");
    for projection in [health.as_object().expect("health"), &tasks, &runtime] {
        assert_eq!(projection["configured"], false);
        assert_eq!(projection["ready"], false);
        assert_eq!(projection["readiness_code"], "invalid_configuration");
        assert_eq!(projection["readiness_error"], error.message);
    }
    assert_eq!(health["status"], "queued");
    assert_eq!(tasks["tasks"][0]["task_id"], retained_task.task_id);
    assert_eq!(tasks["tasks"][0]["preview"], "Retain accepted work");
    assert_eq!(std::fs::read(&path).expect("preserved settings"), corrupt);
    std::fs::write(&path, original).expect("restore fixture");
}

#[test]
fn readiness_reflects_a_closing_owner_without_blocking_source_capture() {
    let fixture = Fixture::new();
    let owner = lookup(&fixture.home).expect("owner");
    owner.closing.store(true, Ordering::Release);
    let stopping = assert_readiness(
        &fixture,
        true,
        false,
        Some("owner_unavailable"),
        "unavailable",
    );
    assert!(
        stopping["settings"]["readiness_error"]
            .as_str()
            .expect("diagnostic")
            .contains("stopping")
    );
    assert_eq!(stopping["runtime"]["phase"], "stopping");
    let saved = request(&fixture.home, "assistant_voice_document_instruction", json!({
        "group_id":fixture.group,"instruction":"Preserve while stopping","input_append_id":"closing-owner-source"
    })).expect("source capture stays successful");
    assert_eq!(saved["input_event_created"], true);
    assert_eq!(saved["secretary_processing_deferred"], true);
    assert!(
        saved["secretary_processing_error"]
            .as_str()
            .expect("safe error")
            .contains("stopping")
    );
    owner.closing.store(false, Ordering::Release);
    assert_readiness(&fixture, true, true, None, "ready");
}

#[test]
fn voice_workflow_does_not_create_an_actor_or_group_configuration() {
    let fixture = Fixture::new();
    let group = GroupStore::new(fixture.home.clone())
        .expect("store")
        .load(&fixture.group)
        .expect("group");
    assert!(group.actors.is_empty());
    assert!(group.extra.get("assistants").is_none());
    assert_eq!(
        cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("global view")["assistant"]
            ["enabled"],
        true
    );
}

#[test]
fn retry_of_an_unconfirmed_source_does_not_create_another_task() {
    let fixture = Fixture::new();
    let task = fixture.ask("Read the weather", "same-source");
    let store = task_store(&fixture.home);
    store
        .update(&task.task_id, |t| {
            t.phase = SecretaryTaskPhase::Unconfirmed;
            Ok(())
        })
        .expect("unconfirmed");
    let id = enqueue(&fixture.home, &fixture.group, &task.inputs[0]).expect("retry");
    assert_eq!(id.as_deref(), Some(task.task_id.as_str()));
    assert_eq!(store.list().expect("tasks").len(), 1);
}

#[test]
fn task_context_and_terminal_receipt_are_bound_to_the_original_request() {
    let fixture = Fixture::new();
    let task = fixture.ask("Question for A", "a");
    let token = fixture.grant(&task);
    let context = request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,"action":"context"}),
    )
    .expect("context");
    assert_eq!(context["target"]["group_id"], fixture.group);
    assert_eq!(context["target"]["request_id"], task.target.request_id);
    assert!(request(&fixture.home,"voice_secretary_task",json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"wrong Group","group_id":"B"})).is_err());
    let receipt = request(&fixture.home,"voice_secretary_task",json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"Answer for A"})).expect("report");
    assert_eq!(receipt["receipt"]["output"]["reply_text"], "Answer for A");
    let saved = task_store(&fixture.home)
        .load(&task.task_id)
        .expect("saved");
    super::super::assistants::secretary_project_result(&fixture.home, &saved).expect("projection");
    let state = cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("state");
    assert!(state["ask_requests"].as_array().expect("asks").iter().any(|r|r["request_id"]==task.target.request_id && r["reply_text"]=="Answer for A"));
    let feedback = state["ask_requests"]
        .as_array()
        .expect("asks")
        .iter()
        .find(|item| item["request_id"] == task.target.request_id)
        .expect("matching feedback");
    assert_eq!(feedback["secretary_task_id"], task.task_id);
    let status = request(
        &fixture.home,
        "assistant_state",
        json!({"group_id":fixture.group,"view":"voice_status"}),
    )
    .expect("read-only status");
    assert_eq!(status["ask_requests"][0]["secretary_task_id"], task.task_id);
}

#[test]
fn cancellation_revokes_the_capability_before_provider_cleanup() {
    let fixture = Fixture::new();
    let task = fixture.ask("Question", "a");
    let token = fixture.grant(&task);
    request(
        &fixture.home,
        "voice_secretary_task_cancel",
        json!({"group_id":fixture.group,"task_id":task.task_id}),
    )
    .expect("cancel");
    let result = request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"late answer"}),
    );
    assert_eq!(
        result.expect_err("old grant rejected").code,
        "secretary_task_expired"
    );
    assert!(
        task_store(&fixture.home)
            .load(&task.task_id)
            .expect("task")
            .receipt
            .is_none()
    );
}

#[test]
fn clearing_ask_history_does_not_cancel_accepted_global_work() {
    let fixture = Fixture::new();
    let task = fixture.ask("Keep the accepted investigation", "clear-display");
    let token = fixture.grant(&task);
    request(
        &fixture.home,
        "assistant_voice_ask_requests_clear",
        json!({"group_id":fixture.group,"keep_active":false}),
    )
    .expect("hide history");
    request(&fixture.home,"voice_secretary_task",json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"Investigation complete"})).expect("accepted work remains valid");
    let store = task_store(&fixture.home);
    store
        .update(&task.task_id, |t| {
            t.phase = SecretaryTaskPhase::Done;
            t.cleanup_confirmed = true;
            Ok(())
        })
        .expect("completion");
    super::super::assistants::secretary_project_result(
        &fixture.home,
        &store.load(&task.task_id).expect("task"),
    )
    .expect("project feedback");
    let state = cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("state");
    assert_eq!(
        state["ask_requests"][0]["reply_text"],
        "Investigation complete"
    );
    assert!(state["ask_requests"][0].get("cleared_at").is_none());
}

#[test]
fn accepted_ask_backlog_outlives_the_completed_history_limit() {
    let fixture = Fixture::new();
    let tasks = (0..31)
        .map(|index| fixture.ask("Queued investigation", &format!("backlog-{index}")))
        .collect::<Vec<_>>();
    let store = task_store(&fixture.home);
    for task in &tasks {
        super::super::assistants::secretary_validate_request(&fixture.home, task)
            .expect("accepted request authority must survive the display history limit");
        let token = fixture.grant(task);
        request(
            &fixture.home,
            "voice_secretary_task",
            json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"Queued answer"}),
        )
        .expect("report accepted work");
        store
            .update(&task.task_id, |task| {
                task.phase = SecretaryTaskPhase::Done;
                task.cleanup_confirmed = true;
                Ok(())
            })
            .expect("completed task");
        super::super::assistants::secretary_project_result(
            &fixture.home,
            &store.load(&task.task_id).expect("task"),
        )
        .expect("project the accepted answer");
    }
    let state = cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("state");
    let asks = state["ask_requests"].as_array().expect("asks");
    assert_eq!(asks.len(), 30, "only completed history is bounded");
    assert!(asks.iter().all(|ask| ask["reply_text"] == "Queued answer"));
    assert_eq!(store.list().expect("durable task receipts").len(), 31);
}

#[test]
fn fresh_continuations_keep_the_questions_and_previously_confirmed_answers() {
    let fixture = Fixture::new();
    let original = fixture.ask("Plan an event", "clarification-chain");
    let store = task_store(&fixture.home);
    let mut current = original.clone();
    for (question, answer) in [
        ("Choose morning or evening", "Evening"),
        ("Which date?", "October 6"),
    ] {
        let token = fixture.grant(&current);
        request(
            &fixture.home,
            "voice_secretary_task",
            json!({"_cccc_secretary_token":token,
            "action":"report","status":"needs_user","reply_text":question}),
        )
        .expect("clarification question");
        store
            .update(&current.task_id, |task| {
                task.phase = SecretaryTaskPhase::NeedsUser;
                task.cleanup_confirmed = true;
                Ok(())
            })
            .expect("terminal cleanup");
        let result = request(
            &fixture.home,
            "voice_secretary_task_retry",
            json!({
            "group_id":fixture.group,"task_id":current.task_id,"followup":answer}),
        )
        .expect("confirmed continuation");
        let repeated = request(
            &fixture.home,
            "voice_secretary_task_retry",
            json!({
            "group_id":fixture.group,"task_id":current.task_id,"followup":answer}),
        )
        .expect("same confirmation");
        assert_eq!(repeated["task"]["task_id"], result["task"]["task_id"]);
        let changed = request(&fixture.home, "voice_secretary_task_retry", json!({
            "group_id":fixture.group,"task_id":current.task_id,"followup":format!("{answer}, changed")})).expect_err("changed answer must not appear accepted");
        assert_eq!(changed.code, "secretary_continuation_changed");
        current = store
            .load(result["task"]["task_id"].as_str().expect("successor"))
            .expect("task");
    }
    assert_eq!(current.target, original.target);
    assert_eq!(current.inputs, original.inputs);
    let token = fixture.grant(&current);
    let context = request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,
        "action":"context"}),
    )
    .expect("new process context");
    let confirmed = context["followup"].as_str().expect("dialogue");
    for text in [
        "Choose morning or evening",
        "Evening",
        "Which date?",
        "October 6",
    ] {
        assert!(
            confirmed.contains(text),
            "missing confirmed context: {text}"
        );
    }
    assert!(
        confirmed.find("Evening").expect("first answer")
            < confirmed.find("Which date?").expect("second question")
    );
}

#[test]
fn deleting_or_resetting_one_group_revokes_only_its_tasks() {
    for operation in ["group_delete", "group_reset"] {
        let fixture = Fixture::new();
        let running = fixture.ask("Running work", "delete-running");
        let token = fixture.grant(&running);
        let queued = fixture.ask("Queued work", "delete-queued");
        let other = GroupStore::new(fixture.home.clone())
            .expect("store")
            .create("B", "")
            .expect("other group")
            .group_id;
        let b = request(
            &fixture.home,
            "assistant_voice_document_instruction",
            json!({"group_id":other,"instruction":"B work","input_append_id":"b"}),
        )
        .expect("B input");
        request(
            &fixture.home,
            operation,
            json!({"group_id":fixture.group,"by":"user","confirm":fixture.group}),
        )
        .expect("retire isolated A");
        assert_eq!(
            request(
                &fixture.home,
                "voice_secretary_task",
                json!({"_cccc_secretary_token":token,"action":"context"})
            )
            .expect_err("grant revoked")
            .code,
            "secretary_task_expired"
        );
        let store = task_store(&fixture.home);
        assert_eq!(
            store.load(&queued.task_id).expect("A retained").phase,
            SecretaryTaskPhase::Cancelled
        );
        assert_eq!(
            store
                .load(b["secretary_task_id"].as_str().expect("B id"))
                .expect("B retained")
                .phase,
            SecretaryTaskPhase::Queued
        );
    }
}

#[test]
fn ask_and_prompt_receipts_keep_origin_scope_and_project_once() {
    let fixture = Fixture::new();
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    store
        .mutate(&fixture.group, |g| {
            g.scopes = vec!["scope-a", "scope-b"]
                .into_iter()
                .map(|key| cccc_core::Scope {
                    scope_key: key.into(),
                    url: fixture._temp.path().to_string_lossy().into_owned(),
                    label: key.into(),
                    git_remote: String::new(),
                })
                .collect();
            g.active_scope_key = "scope-a".into();
            Ok(())
        })
        .expect("scopes");
    let ask = fixture.ask("Origin A question", "scope-ask");
    let token = fixture.grant(&ask);
    let prompt=request(&fixture.home,"assistant_voice_input_append",json!({"group_id":fixture.group,"kind":"prompt_refine","composer_text":"Original prompt","request_id":"scope-prompt","input_append_id":"scope-input"})).expect("prompt");
    let jobs = task_store(&fixture.home);
    let prompt = jobs
        .load(prompt["secretary_task_id"].as_str().expect("prompt id"))
        .expect("job");
    let prompt_token = fixture.grant(&prompt);
    store
        .mutate(&fixture.group, |g| {
            g.active_scope_key = "scope-b".into();
            Ok(())
        })
        .expect("change active scope");
    request(&fixture.home,"voice_secretary_task",json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"A answer"})).expect("Ask receipt");
    request(&fixture.home,"voice_secretary_task",json!({"_cccc_secretary_token":prompt_token,"action":"draft","draft_text":"Refined prompt"})).expect("Prompt receipt");
    for original in [&ask, &prompt] {
        jobs.update(&original.task_id, |t| {
            t.phase = SecretaryTaskPhase::Done;
            t.cleanup_confirmed = true;
            Ok(())
        })
        .expect("complete");
        let saved = jobs.load(&original.task_id).expect("saved");
        super::super::assistants::secretary_project_result(&fixture.home, &saved).expect("project");
        block_on(lookup(&fixture.home).expect("owner").project(&saved));
    }
    request(
        &fixture.home,
        "assistant_voice_prompt_draft_ack",
        json!({"group_id":fixture.group,"request_id":"scope-prompt","status":"applied"}),
    )
    .expect("apply");
    request(
        &fixture.home,
        "voice_secretary_task_retry",
        json!({"group_id":fixture.group,"task_id":prompt.task_id}),
    )
    .expect("duplicate sync is read-only");
    assert!(
        cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("state")
            ["prompt_draft"]
            .is_null()
    );
    let events = cccc_core::ledger::read_all(&store.ledger_path(&fixture.group).expect("ledger"))
        .expect("events");
    for original in [&ask, &prompt] {
        let receipts = events
            .iter()
            .filter(|e| {
                e.data.get("secretary_task_id").and_then(Value::as_str)
                    == Some(original.task_id.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].scope_key, "scope-a");
    }
}

#[test]
fn startup_reconstructs_unclaimed_global_sources_but_never_replays_admitted_work() {
    let fixture = Fixture::new();
    let task = fixture.ask("Durable source", "durable");
    stop(&fixture.home).expect("stop owner");
    let store = task_store(&fixture.home);
    std::fs::remove_dir_all(store.directory(&task.task_id).expect("job dir"))
        .expect("simulate crash before job publication");
    start_unit_owner(&fixture.home).expect("recover acceptance");
    let recovered = store.list().expect("tasks");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].target, task.target);
    assert_eq!(recovered[0].inputs, task.inputs);
    store
        .update(&recovered[0].task_id, |t| {
            t.phase = SecretaryTaskPhase::Running;
            t.cleanup_confirmed = false;
            Ok(())
        })
        .expect("admitted");
    stop(&fixture.home).expect("stop");
    start_unit_owner(&fixture.home).expect("restart");
    let tasks = store.list().expect("tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].phase, SecretaryTaskPhase::Unconfirmed);
}

#[test]
fn unavailable_owner_retains_acceptance_without_creating_an_unlocked_manager() {
    let fixture = Fixture::new();
    stop(&fixture.home).expect("stop owner");
    let result = request(&fixture.home, "assistant_voice_document_instruction", json!({
        "group_id":fixture.group,"instruction":"Retain this source","input_append_id":"unavailable"
    })).expect("capture succeeds while owner is unavailable");
    assert!(
        result["secretary_processing_error"]
            .as_str()
            .expect("scheduling diagnostic")
            .contains("saved inputs are retained")
    );
    assert!(lookup(&fixture.home).is_none());
    let store = task_store(&fixture.home);
    assert!(
        store
            .list()
            .expect("no prematurely published work")
            .is_empty()
    );
    start_unit_owner(&fixture.home).expect("daemon startup recovery");
    let tasks = store.list().expect("recovered acceptance");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].target.group_id, fixture.group);
    assert!(
        tasks[0].inputs[0]["text"]
            .as_str()
            .expect("source")
            .contains("Retain this source")
    );
}

#[test]
fn unchanged_document_submission_confirms_sources_without_incrementing_revision() {
    let fixture = Fixture::new();
    request(
        &fixture.home,
        "assistant_voice_document_save",
        json!({
            "group_id":fixture.group,"document_path":"notes.md","content":"# Original\n"
        }),
    )
    .expect("registered document");
    let before = request(
        &fixture.home,
        "assistant_index",
        json!({"group_id":fixture.group}),
    )
    .expect("before");
    let result = request(&fixture.home,"assistant_voice_document_instruction",json!({
        "group_id":fixture.group,"document_path":"notes.md","instruction":"Keep the content unchanged","input_append_id":"unchanged"
    })).expect("accepted");
    let store = task_store(&fixture.home);
    let id = result["secretary_task_id"].as_str().expect("id");
    let task = store.load(id).expect("task");
    store
        .update(id, |t| {
            t.phase = SecretaryTaskPhase::Starting;
            Ok(())
        })
        .expect("starting");
    let file =
        super::super::assistants::secretary_document_file(&fixture.home, &task).expect("file");
    store.prepare_document(id, &file).expect("workcopy");
    let token = fixture.grant(&task);
    let base = store.load(id).expect("prepared").base_version;
    request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,"action":"commit","base_version":base}),
    )
    .expect("commit");
    store
        .update(id, |t| {
            t.phase = SecretaryTaskPhase::Done;
            t.cleanup_confirmed = true;
            Ok(())
        })
        .expect("cleanup");
    let task = store.load(id).expect("done");
    super::super::assistants::secretary_project_result(&fixture.home, &task).expect("project");
    super::super::assistants::secretary_project_result(&fixture.home, &task)
        .expect("repeat projection");
    let after = request(
        &fixture.home,
        "assistant_index",
        json!({"group_id":fixture.group}),
    )
    .expect("after");
    assert_eq!(
        after["documents"][0]["revision_count"],
        before["documents"][0]["revision_count"]
    );
    assert_eq!(after["documents"][0]["last_secretary_task_id"], id);
}

#[test]
fn cancel_queued_work_projects_failure_and_explicit_continuation_is_distinct() {
    let fixture = Fixture::new();
    let task = fixture.ask("Queued source", "queued");
    request(
        &fixture.home,
        "voice_secretary_task_cancel",
        json!({"group_id":fixture.group,"task_id":task.task_id}),
    )
    .expect("cancel");
    let store = task_store(&fixture.home);
    let saved = store.load(&task.task_id).expect("saved");
    assert_eq!(saved.phase, SecretaryTaskPhase::Cancelled);
    assert!(!saved.projected_at.is_empty());
    let state = cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("state");
    assert_eq!(state["ask_requests"][0]["secretary_task_id"], task.task_id);
    let next = request(
        &fixture.home,
        "voice_secretary_task_retry",
        json!({"group_id":fixture.group,"task_id":task.task_id}),
    )
    .expect("explicit retry");
    assert_ne!(next["task"]["task_id"], task.task_id);
    let again = request(
        &fixture.home,
        "voice_secretary_task_retry",
        json!({"group_id":fixture.group,"task_id":task.task_id}),
    )
    .expect("same action");
    assert_eq!(next["task"]["task_id"], again["task"]["task_id"]);
    let state = cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("state");
    assert_eq!(
        state["ask_requests"][0]["secretary_task_id"], task.task_id,
        "a queued retry must not relabel its predecessor's feedback"
    );
    let successor = store
        .load(next["task"]["task_id"].as_str().expect("successor ID"))
        .expect("successor");
    let token = fixture.grant(&successor);
    request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"New answer"}),
    )
    .expect("successor receipt");
    super::super::assistants::secretary_project_result(
        &fixture.home,
        &store.load(&successor.task_id).expect("completed successor"),
    )
    .expect("successor projection");
    let state = cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("state");
    assert_eq!(
        state["ask_requests"][0]["secretary_task_id"],
        successor.task_id
    );
    assert_eq!(state["ask_requests"][0]["reply_text"], "New answer");
}

#[test]
fn startup_ignores_a_missing_group_without_removing_its_registry_entry() {
    let fixture = Fixture::new();
    stop(&fixture.home).expect("stop owner");
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    let missing = groups.create("Missing Group", "").expect("group");
    std::fs::remove_dir_all(groups.group_dir(&missing.group_id).expect("path"))
        .expect("remove fixture Group directory");

    start_unit_owner(&fixture.home).expect("start despite stale registry entry");
    assert!(lookup(&fixture.home).is_some());
    assert!(
        groups
            .list()
            .expect("registry")
            .iter()
            .any(|group| group.group_id == missing.group_id),
        "startup must not silently edit the Group registry"
    );
    let result = request(&fixture.home, "assistant_voice_document_instruction", json!({
        "group_id":fixture.group,"instruction":"Refine this material","input_append_id":"valid-after-missing-group"
    })).expect("accepted");
    assert!(result["secretary_task_id"].is_string());
    assert_eq!(task_store(&fixture.home).list().expect("tasks").len(), 1);
}

#[test]
fn startup_does_not_parse_unrelated_removed_runtime_configuration() {
    let fixture = Fixture::new();
    stop(&fixture.home).expect("stop owner");
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    let old = groups.create("Stopped legacy Group", "").expect("group");
    let path = groups
        .group_dir(&old.group_id)
        .expect("path")
        .join("group.yaml");
    let mut value = serde_json::to_value(&old).expect("value");
    value["state"] = json!("stopped");
    value["actors"] = json!([{"id":"legacy-peer","runtime":"gemini","enabled":false}]);
    cccc_core::fs::write_yaml(&path, &value).expect("old Runtime fixture");
    let original = std::fs::read(&path).expect("saved configuration");
    assert!(
        groups.load(&old.group_id).is_err(),
        "Gemini is not being restored as a supported Runtime"
    );
    groups
        .mutate(&fixture.group, |group| {
            let mut actor = cccc_contracts::Actor::new("old-secretary");
            actor.internal_kind = Some("voice_secretary".into());
            group.actors.push(actor);
            Ok(())
        })
        .expect("obsolete owner in a valid Group");

    start_unit_owner(&fixture.home).expect("unrelated Runtime does not block global owner");
    assert!(lookup(&fixture.home).is_some());
    assert!(
        groups
            .load(&fixture.group)
            .expect("valid Group")
            .actors
            .is_empty()
    );
    assert_eq!(
        std::fs::read(&path).expect("preserved configuration"),
        original
    );
    assert!(groups.load(&old.group_id).is_err());
    assert!(
        groups
            .list()
            .expect("registry")
            .iter()
            .any(|group| group.group_id == old.group_id)
    );
    let result = request(&fixture.home, "assistant_voice_document_instruction", json!({
        "group_id":fixture.group,"instruction":"Refine this material","input_append_id":"valid-after-removed-runtime"
    })).expect("accepted");
    assert!(result["secretary_task_id"].is_string());
}

#[test]
fn startup_does_not_hide_corrupt_group_state_and_projects_unavailable_execution() {
    let fixture = Fixture::new();
    stop(&fixture.home).expect("stop owner");
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    let invalid = groups.create("Corrupt Group", "").expect("group");
    std::fs::write(
        groups
            .group_dir(&invalid.group_id)
            .expect("path")
            .join("group.yaml"),
        "actors: [\n",
    )
    .expect("corrupt fixture");
    let failure = start(&fixture.home, None).expect_err("invalid YAML still fails startup");
    let diagnostic = failure.to_string();
    assert!(diagnostic.contains("startup failed"), "{diagnostic}");
    assert!(diagnostic.contains(&invalid.group_id), "{diagnostic}");
    assert!(diagnostic.contains("YAML"), "{diagnostic}");
    assert!(lookup(&fixture.home).is_none());
    let result = request(&fixture.home, "assistant_voice_document_instruction", json!({
        "group_id":fixture.group,"instruction":"Preserve this request","input_append_id":"owner-unavailable"
    })).expect("source retained");
    assert!(
        result["input_event_created"]
            .as_bool()
            .expect("source saved")
    );
    assert_eq!(result["secretary_processing_deferred"], true);
    assert_eq!(
        result["secretary_processing_error"],
        format!("Processing deferred; source saved: {diagnostic}")
    );
    assert!(task_store(&fixture.home).list().expect("tasks").is_empty());
    let health = group_projection(&fixture.home, &fixture.group).expect("health");
    assert_eq!(health["configured"], true);
    assert_eq!(health["ready"], false);
    assert_eq!(health["readiness_code"], "owner_unavailable");
    assert_eq!(health["readiness_error"], diagnostic);
    let view = request(
        &fixture.home,
        "voice_secretary_tasks",
        json!({"group_id":fixture.group}),
    )
    .expect("task read remains available");
    assert_eq!(view["readiness_error"], diagnostic);
    let settings_view = request(&fixture.home, "voice_secretary_settings_get", json!({}))
        .expect("settings remain readable");
    assert_eq!(settings_view["readiness_error"], diagnostic);
    assert!(
        lookup(&fixture.home).is_none(),
        "reads must not create an execution owner"
    );
}

#[test]
fn startup_reports_invalid_retiring_group_without_exposing_yaml_values() {
    let fixture = Fixture::new();
    stop(&fixture.home).expect("stop owner");
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    let invalid = groups.create("Retiring invalid Group", "").expect("group");
    let path = groups
        .group_dir(&invalid.group_id)
        .expect("path")
        .join("group.yaml");
    let original = std::fs::read(&path).expect("original");
    let mut value = serde_json::to_value(&invalid).expect("value");
    value["actors"] = json!([
        {"id":"voice-secretary","runtime":"codex","enabled":false},
        {"id":"invalid-peer","runtime":"synthetic-private-value-must-not-be-exposed"}
    ]);
    cccc_core::fs::write_yaml(&path, &value).expect("invalid retiring Group");
    let before = std::fs::read(&path).expect("fixture");

    let error = start(&fixture.home, None).expect_err("unresolved cleanup must fail");
    let diagnostic = error.to_string();
    assert!(diagnostic.contains(&invalid.group_id), "{diagnostic}");
    assert!(diagnostic.contains("YAML"), "{diagnostic}");
    assert!(
        !diagnostic.contains("synthetic-private-value"),
        "{diagnostic}"
    );
    assert_eq!(
        configuration::readiness_error(&fixture.home),
        Some(diagnostic)
    );
    assert_eq!(std::fs::read(&path).expect("preserved fixture"), before);
    assert!(lookup(&fixture.home).is_none());

    std::fs::write(&path, original).expect("repair fixture");
    start_unit_owner(&fixture.home).expect("explicit startup after repair");
    assert!(configuration::readiness_error(&fixture.home).is_none());
}

#[test]
fn unavailable_owner_exposes_safe_task_failure_without_rereading_corrupt_history() {
    let fixture = Fixture::new();
    let task = fixture.ask("Retain accepted work", "corrupt-history-source");
    stop(&fixture.home).expect("stop owner");
    let store = SecretaryTaskStore::new(fixture.home.clone());
    let path = store
        .directory(&task.task_id)
        .expect("directory")
        .join("task.json");
    let original = std::fs::read(&path).expect("original");
    let mut value = serde_json::to_value(&task).expect("task");
    value["phase"] = json!("synthetic-private-phase-must-not-be-exposed");
    std::fs::write(&path, serde_json::to_vec(&value).expect("fixture")).expect("corrupt task");
    let before = std::fs::read(&path).expect("fixture bytes");

    let diagnostic = start(&fixture.home, None)
        .expect_err("startup fails closed")
        .to_string();
    let view = request(
        &fixture.home,
        "voice_secretary_tasks",
        json!({"group_id":fixture.group}),
    )
    .expect("failure state remains readable");
    let health = group_projection(&fixture.home, &fixture.group).expect("health remains readable");
    let runtime =
        request(&fixture.home, "voice_secretary_runtime", json!({})).expect("runtime read");
    let settings =
        request(&fixture.home, "voice_secretary_settings_get", json!({})).expect("settings read");
    assert!(diagnostic.contains(&task.task_id), "{diagnostic}");
    assert!(diagnostic.contains("JSON"), "{diagnostic}");
    assert!(!diagnostic.contains("synthetic-private"), "{diagnostic}");
    for projection in [&view, health.as_object().expect("health object"), &settings] {
        assert_eq!(projection["configured"], true);
        assert_eq!(projection["ready"], false);
        assert_eq!(projection["readiness_code"], "owner_unavailable");
        assert_eq!(projection["readiness_error"], diagnostic);
    }
    assert_eq!(runtime["phase"], "unavailable");
    assert_eq!(runtime["diagnostic"], diagnostic);
    assert_eq!(view["tasks"], json!([]));
    assert_eq!(health["tasks"], json!([]));
    assert!(
        lookup(&fixture.home).is_none(),
        "reads must not recreate an owner"
    );
    let saved = request(&fixture.home, "assistant_voice_document_instruction", json!({
        "group_id":fixture.group,"instruction":"Preserve new input","input_append_id":"corrupt-history-deferred"
    })).expect("new source remains durable");
    assert_eq!(saved["secretary_processing_deferred"], true);
    assert_eq!(std::fs::read(&path).expect("retained bytes"), before);

    std::fs::write(&path, original).expect("explicit fixture repair");
    start_unit_owner(&fixture.home).expect("explicit start after repair");
    assert!(configuration::readiness_error(&fixture.home).is_none());
    assert!(task_store(&fixture.home).load(&task.task_id).is_ok());
}

#[test]
fn startup_retires_obsolete_actor_resources_and_keeps_sources_and_other_actors() {
    let fixture = Fixture::new();
    stop(&fixture.home).expect("stop global owner");
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    groups
        .mutate(&fixture.group, |group| {
            let mut actor = cccc_contracts::Actor::new("voice-secretary");
            actor.internal_kind = Some("voice_secretary".into());
            group.actors.push(actor);
            let mut peer = cccc_contracts::Actor::new("peer");
            peer.enabled = false;
            group.actors.push(peer);
            for key in [
                "runtime_states",
                "web_model_browser_targets",
                "web_model_delivery_preferences",
            ] {
                group.extra.insert(
                    key.into(),
                    json!({"voice-secretary":{"fixture":"retire"},"peer":{"fixture":"keep"}}),
                );
            }
            Ok(())
        })
        .expect("old actor");
    super::super::actor_secrets::replace(
        &fixture.home,
        &fixture.group,
        "voice-secretary",
        BTreeMap::from([("FIXTURE_PRIVATE_VALUE".into(), "synthetic".into())]),
    )
    .expect("old private environment");
    let runner = fixture
        .home
        .groups_dir()
        .join(&fixture.group)
        .join("state/runners/headless/voice-secretary.json");
    cccc_core::fs::write_json(&runner, &json!({"fixture":"retire"})).expect("old runner state");
    let raw = fixture
        .home
        .root()
        .join("voice-secretary")
        .join(&fixture.group)
        .join("input_events.jsonl");
    cccc_core::fs::atomic_write(&raw, b"{\"seq\":1,\"text\":\"Old source\"}\n")
        .expect("old source");
    let lock = fixture
        .home
        .groups_dir()
        .join(&fixture.group)
        .join("group.yaml.lock");
    std::fs::remove_file(&lock).expect("lock");
    std::fs::create_dir(&lock).expect("block retirement");
    assert!(start(&fixture.home, None).is_err());
    let diagnostic = configuration::readiness_error(&fixture.home).expect("retirement failure");
    assert!(diagnostic.contains(&fixture.group), "{diagnostic}");
    assert!(diagnostic.contains("removing"), "{diagnostic}");
    assert!(
        groups
            .load(&fixture.group)
            .expect("retained roster")
            .actors
            .iter()
            .any(|actor| actor.id == "voice-secretary")
    );
    assert!(
        request(
            &fixture.home,
            "actor_start",
            json!({"group_id":fixture.group,"actor_id":"voice-secretary"})
        )
        .is_err(),
        "obsolete owner must never restart"
    );
    std::fs::remove_dir(&lock).expect("unblock");
    start_unit_owner(&fixture.home).expect("retire resources");
    start_unit_owner(&fixture.home).expect("idempotent startup");
    assert!(configuration::readiness_error(&fixture.home).is_none());
    let group = groups.load(&fixture.group).expect("group");
    assert_eq!(group.actors.len(), 1);
    assert_eq!(group.actors[0].id, "peer");
    for key in [
        "runtime_states",
        "web_model_browser_targets",
        "web_model_delivery_preferences",
    ] {
        assert!(group.extra[key].get("voice-secretary").is_none());
        assert_eq!(group.extra[key]["peer"], json!({"fixture":"keep"}));
    }
    assert!(
        super::super::actor_secrets::values(&fixture.home, &fixture.group, "voice-secretary")
            .expect("retired private environment")
            .is_empty()
    );
    assert!(
        !runner.exists(),
        "the retired Actor cannot resume its runner"
    );
    assert_eq!(
        super::super::assistants::secretary_global_inputs(&fixture.home, &fixture.group)
            .expect("new-source intents")
            .len(),
        0
    );
    assert_eq!(
        std::fs::read_to_string(raw).expect("source retained"),
        "{\"seq\":1,\"text\":\"Old source\"}\n"
    );
    assert!(
        task_store(&fixture.home).list().expect("tasks").is_empty(),
        "obsolete input has no accepted intent and must not replay"
    );
}

#[test]
fn secretary_profile_is_a_service_consumer_and_requires_explicit_detach() {
    let fixture = Fixture::new();
    let profile = request(
        &fixture.home,
        "actor_profile_upsert",
        json!({"name":"Secretary","runtime":"codex"}),
    )
    .expect("profile");
    let id = profile["profile"]["id"].as_str().expect("id");
    settings::update(&fixture.home, |settings| {
        settings.voice_secretary.profile_id = id.into();
        Ok(())
    })
    .expect("select");
    let usage = ProfileStore::new(fixture.home.clone())
        .expect("profiles")
        .usage(id)
        .expect("usage");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0]["consumer"], "voice_secretary");
    assert_eq!(
        request(
            &fixture.home,
            "actor_profile_delete",
            json!({"profile_id":id})
        )
        .expect_err("in use")
        .code,
        "profile_in_use"
    );
    request(
        &fixture.home,
        "actor_profile_delete",
        json!({"profile_id":id,"force_detach":true}),
    )
    .expect("explicit detach");
    assert!(
        settings::load(&fixture.home)
            .expect("settings")
            .voice_secretary
            .profile_id
            .is_empty()
    );
    assert!(
        GroupStore::new(fixture.home.clone())
            .expect("store")
            .load(&fixture.group)
            .expect("group")
            .actors
            .is_empty()
    );
}

#[test]
fn confirmed_handoff_is_fixed_to_the_group_and_recorded_only_once() {
    let fixture = Fixture::new();
    GroupStore::new(fixture.home.clone())
        .expect("store")
        .mutate(&fixture.group, |group| {
            let mut peer = cccc_contracts::Actor::new("worker");
            peer.runtime = ActorRuntime::Custom;
            group.actors.push(peer);
            Ok(())
        })
        .expect("peer");
    let task = fixture.ask("Ask worker to investigate", "handoff");
    let token = fixture.grant(&task);
    request(&fixture.home,"voice_secretary_task",json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"Proposed task for worker","handoff_target":"worker","handoff_text":"Investigate the issue"})).expect("proposal");
    let store = task_store(&fixture.home);
    store
        .update(&task.task_id, |task| {
            task.phase = SecretaryTaskPhase::Done;
            task.cleanup_confirmed = true;
            Ok(())
        })
        .expect("completion");
    let args = json!({"group_id":fixture.group,"task_id":task.task_id});
    let first =
        request(&fixture.home, "voice_secretary_task_handoff", args.clone()).expect("confirm");
    let second =
        request(&fixture.home, "voice_secretary_task_handoff", args).expect("same confirmation");
    assert_eq!(first["notify_event"]["id"], second["notify_event"]["id"]);
    assert_eq!(first["notify_event"]["by"], "user");
    assert!(
        !store
            .load(&task.task_id)
            .expect("task")
            .forwarded_event_id
            .is_empty()
    );
    assert!(
        request(
            &fixture.home,
            "voice_secretary_task_handoff",
            json!({"group_id":"other","task_id":task.task_id})
        )
        .is_err()
    );
}

#[test]
fn unconfigured_and_full_queues_keep_capture_success_and_recover_deferred_sources() {
    let fixture = Fixture::new();
    let profile = settings::load(&fixture.home)
        .expect("settings")
        .voice_secretary
        .profile_id;
    settings::update(&fixture.home, |s| {
        s.voice_secretary.profile_id.clear();
        Ok(())
    })
    .expect("unconfigure");
    let result=request(&fixture.home,"assistant_voice_document_instruction",json!({"group_id":fixture.group,"instruction":"Saved without execution","input_append_id":"no-config"})).expect("saved");
    assert!(result["secretary_task_id"].is_null());
    assert!(task_store(&fixture.home).list().expect("jobs").is_empty());
    assert_eq!(
        group_projection(&fixture.home, &fixture.group).expect("health")["deferred_sources"],
        1
    );
    settings::update(&fixture.home, |s| {
        s.voice_secretary.profile_id = profile;
        Ok(())
    })
    .expect("configure acceptance");
    let manager = lookup(&fixture.home).expect("owner");
    manager.reconcile_sources().expect("drain");
    for index in 1..64 {
        fixture.ask(&format!("Question {index}"), &format!("question-{index}"));
    }
    let result=request(&fixture.home,"assistant_voice_document_instruction",json!({"group_id":fixture.group,"instruction":"Saved beyond capacity","input_append_id":"overflow"})).expect("capture success");
    assert!(
        result["input_event_created"]
            .as_bool()
            .expect("source saved")
    );
    assert!(result["secretary_task_id"].is_null());
    assert_eq!(manager.store.queued().expect("queue").len(), 64);
    assert_eq!(deferred_count(&fixture.home, &fixture.group), 1);
    let first = manager.store.queued().expect("queue")[0].clone();
    request(
        &fixture.home,
        "voice_secretary_task_cancel",
        json!({"group_id":fixture.group,"task_id":first.task_id}),
    )
    .expect("release capacity");
    manager.reconcile_sources().expect("drain deferred");
    assert_eq!(deferred_count(&fixture.home, &fixture.group), 0);
    assert_eq!(manager.store.queued().expect("queue").len(), 64);
    let count = manager.store.list().expect("tasks").len();
    stop(&fixture.home).expect("stop");
    start_unit_owner(&fixture.home).expect("restart");
    assert_eq!(
        task_store(&fixture.home).list().expect("no replay").len(),
        count
    );
}

#[test]
fn document_recovery_is_bounded_and_never_replays_user_cancellation_or_prepared_commits() {
    let fixture = Fixture::new();
    request(
        &fixture.home,
        "assistant_voice_document_save",
        json!({"group_id":fixture.group,"document_path":"notes.md","content":"# Original\n"}),
    )
    .expect("doc");
    let result=request(&fixture.home,"assistant_voice_transcript_append",json!({"group_id":fixture.group,"session_id":"asr","segment_id":"one","document_path":"notes.md","text":"Budget 42","is_final":true,"flush":true})).expect("source");
    let id = result["secretary_task_id"].as_str().expect("id");
    let manager = lookup(&fixture.home).expect("owner");
    for (phase, reason, journal, expected) in [
        (SecretaryTaskPhase::Cancelled, "user", "", 0),
        (SecretaryTaskPhase::Unconfirmed, "", "prepared", 0),
        (SecretaryTaskPhase::Cancelled, "shutdown", "", 1),
    ] {
        manager
            .store
            .update(id, |t| {
                t.phase = phase;
                t.isolated_document_writes = true;
                t.cancellation_reason = reason.into();
                t.prepared_version = journal.into();
                Ok(())
            })
            .expect("outcome");
        let old = manager.store.load(id).expect("task");
        block_on(manager.recover_document(&old)).expect("bounded recovery");
        assert_eq!(
            manager.store.successor(id).expect("successor").is_some(),
            expected == 1
        );
    }
    let next = manager
        .store
        .successor(id)
        .expect("successor")
        .expect("retry");
    assert_eq!(next.recovery_attempts, 1);
    assert_eq!(next.inputs, manager.store.load(id).expect("old").inputs);
    assert!(next.base_version.is_empty());
    manager
        .store
        .update(&next.task_id, |t| {
            t.phase = SecretaryTaskPhase::Conflict;
            Ok(())
        })
        .expect("second conflict");
    block_on(manager.recover_document(&manager.store.load(&next.task_id).expect("next")))
        .expect("no retry loop");
    assert!(
        manager
            .store
            .successor(&next.task_id)
            .expect("bounded")
            .is_none()
    );
    let health = group_projection(&fixture.home, &fixture.group).expect("gap remains visible");
    assert_eq!(health["unprocessed_document_sources"], 1);
    assert_eq!(health["pending"], true);
    assert_eq!(health["status"], "conflict");
}

#[test]
fn asking_about_a_document_uses_report_and_cannot_commit() {
    let fixture = Fixture::new();
    request(
        &fixture.home,
        "assistant_voice_document_save",
        json!({"group_id":fixture.group,"document_path":"notes.md","content":"Budget is 42."}),
    )
    .expect("doc");
    let result=request(&fixture.home,"assistant_voice_document_instruction",json!({"group_id":fixture.group,"document_path":"notes.md","task_kind":"ask","instruction":"What is the budget?","input_append_id":"doc-question"})).expect("ask");
    let task = task_store(&fixture.home)
        .load(result["secretary_task_id"].as_str().expect("id"))
        .expect("task");
    assert_eq!(task.target.kind, SecretaryTaskKind::Ask);
    let token = fixture.grant(&task);
    let source = request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,"action":"read","resource":"document"}),
    )
    .expect("read reference");
    assert_eq!(source["result"]["content"], "Budget is 42.");
    assert!(
        request(
            &fixture.home,
            "voice_secretary_task",
            json!({"_cccc_secretary_token":token,"action":"commit","base_version":"x"})
        )
        .is_err()
    );
    request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"42"}),
    )
    .expect("answer channel");
    super::super::assistants::secretary_project_result(
        &fixture.home,
        &task_store(&fixture.home)
            .load(&task.task_id)
            .expect("receipt"),
    )
    .expect("project answer");
    let state = cccc_core::assistant_state::load(&fixture.home, &fixture.group).expect("state");
    assert!(
        state["ask_requests"]
            .as_array()
            .expect("asks")
            .iter()
            .any(|r| r["request_id"] == task.target.request_id && r["reply_text"] == "42")
    );
}

#[test]
fn scoped_reads_follow_acceptance_scope_and_reject_private_and_outside_paths() {
    let fixture = Fixture::new();
    let a = fixture._temp.path().join("workspace-a");
    let b = fixture._temp.path().join("workspace-b");
    std::fs::create_dir(&a).expect("A");
    std::fs::create_dir(&b).expect("B");
    std::fs::write(a.join("facts.md"), "A budget 42").expect("A facts");
    std::fs::write(b.join("facts.md"), "B budget 75").expect("B facts");
    std::fs::write(a.join(".env"), "SYNTHETIC_PRIVATE_VALUE").expect("fixture private file");
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    store
        .mutate(&fixture.group, |g| {
            g.scopes = vec![("a", &a), ("b", &b)]
                .into_iter()
                .map(|(key, path)| cccc_core::Scope {
                    scope_key: key.into(),
                    url: path.to_string_lossy().into_owned(),
                    label: key.into(),
                    git_remote: String::new(),
                })
                .collect();
            g.active_scope_key = "a".into();
            Ok(())
        })
        .expect("scopes");
    let task = fixture.ask("How does A work?", "scope-read");
    let token = fixture.grant(&task);
    store
        .mutate(&fixture.group, |g| {
            g.active_scope_key = "b".into();
            Ok(())
        })
        .expect("switch view");
    let result = request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,"action":"read","resource":"file","path":"facts.md"}),
    )
    .expect("read A");
    assert_eq!(result["result"]["content"], "A budget 42");
    for path in [
        ".env",
        "../workspace-b/facts.md",
        b.join("facts.md").to_str().expect("absolute"),
    ] {
        assert!(
            request(
                &fixture.home,
                "voice_secretary_task",
                json!({"_cccc_secretary_token":token,"action":"read","resource":"file","path":path})
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(b.join("facts.md"), a.join("outside.md")).expect("outside link");
        assert!(request(&fixture.home,"voice_secretary_task",json!({"_cccc_secretary_token":token,"action":"read","resource":"file","path":"outside.md"})).is_err());
    }
    let result = request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token,"action":"read","resource":"search","query":"budget"}),
    )
    .expect("search A");
    assert_eq!(result["result"]["matches"][0]["text"], "A budget 42");
}

#[test]
fn deleting_an_idle_group_removes_its_task_and_source_artifacts() {
    let fixture = Fixture::new();
    let task = fixture.ask("Own Group only", "delete-files");
    let directory = task_store(&fixture.home)
        .directory(&task.task_id)
        .expect("directory");
    let source = fixture
        .home
        .root()
        .join("voice-secretary")
        .join(&fixture.group);
    assert!(directory.exists() && source.exists());
    request(
        &fixture.home,
        "group_delete",
        json!({"group_id":fixture.group,"by":"user"}),
    )
    .expect("delete");
    assert!(!directory.exists() && !source.exists());
}

#[test]
fn general_ask_references_only_active_documents_in_its_accepted_scope() {
    let fixture = Fixture::new();
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    let a = fixture._temp.path().join("a");
    let b = fixture._temp.path().join("b");
    std::fs::create_dir(&a).expect("a");
    std::fs::create_dir(&b).expect("b");
    store
        .mutate(&fixture.group, |g| {
            g.scopes = vec![("a", &a), ("b", &b)]
                .into_iter()
                .map(|(key, path)| cccc_core::Scope {
                    scope_key: key.into(),
                    url: path.to_string_lossy().into_owned(),
                    label: key.into(),
                    git_remote: String::new(),
                })
                .collect();
            g.active_scope_key = "a".into();
            Ok(())
        })
        .expect("scopes");
    request(
        &fixture.home,
        "assistant_voice_document_save",
        json!({"group_id":fixture.group,"document_path":"notes.md","content":"A budget 42"}),
    )
    .expect("doc A");
    let general = |key: &str| {
        request(&fixture.home,"assistant_voice_document_instruction",json!({"group_id":fixture.group,"instruction":"What is the budget?","input_append_id":key,"trigger":{"current_document_path":"notes.md"}})).expect("Ask")
    };
    let result = general("same-scope");
    let task = task_store(&fixture.home)
        .load(result["secretary_task_id"].as_str().expect("id"))
        .expect("task");
    assert_eq!(task.target.document_path, "notes.md");
    assert_eq!(task.target.scope_key, "a");
    assert_eq!(task.target.kind, SecretaryTaskKind::Ask);
    store
        .mutate(&fixture.group, |g| {
            g.active_scope_key = "b".into();
            Ok(())
        })
        .expect("view B");
    let result = general("different-scope");
    let task = task_store(&fixture.home)
        .load(result["secretary_task_id"].as_str().expect("id"))
        .expect("task");
    assert_eq!(task.target.scope_key, "b");
    assert!(
        task.target.document_path.is_empty(),
        "old document reference cannot switch the new Ask to A"
    );
    let result=request(&fixture.home,"assistant_voice_document_instruction",json!({"group_id":fixture.group,"instruction":"Question about A","input_append_id":"explicit-reference","task_kind":"ask","document_path":"notes.md"})).expect("explicit doc question");
    let task = task_store(&fixture.home)
        .load(result["secretary_task_id"].as_str().expect("id"))
        .expect("task");
    assert_eq!(task.target.scope_key, "a");
    assert_eq!(task.target.document_path, "notes.md");
}

#[test]
fn recovered_document_sources_precede_newer_queued_batches() {
    let fixture = Fixture::new();
    request(
        &fixture.home,
        "assistant_voice_document_save",
        json!({"group_id":fixture.group,"document_path":"notes.md","content":"# Notes"}),
    )
    .expect("doc");
    let source = |segment: &str| {
        request(&fixture.home,"assistant_voice_transcript_append",json!({"group_id":fixture.group,"session_id":"asr-order","segment_id":segment,"document_path":"notes.md","text":segment,"is_final":true,"flush":true})).expect("source")["secretary_task_id"].as_str().expect("id").to_owned()
    };
    let old = source("old-budget");
    let manager = lookup(&fixture.home).expect("owner");
    manager
        .store
        .update(&old, |t| {
            t.phase = SecretaryTaskPhase::Running;
            Ok(())
        })
        .expect("running");
    let new = source("new-budget");
    manager
        .store
        .update(&old, |t| {
            t.phase = SecretaryTaskPhase::Conflict;
            t.isolated_document_writes = true;
            Ok(())
        })
        .expect("conflict");
    block_on(manager.recover_document(&manager.store.load(&old).expect("old"))).expect("recover");
    let retry = manager
        .store
        .successor(&old)
        .expect("successor")
        .expect("retry");
    let queue = manager.store.queued().expect("queue");
    assert_eq!(
        queue.iter().map(|t| t.task_id.as_str()).collect::<Vec<_>>(),
        vec![retry.task_id.as_str(), new.as_str()]
    );
    manager
        .store
        .update(&retry.task_id, |t| {
            t.phase = SecretaryTaskPhase::Failed;
            Ok(())
        })
        .expect("failed");
    assert_eq!(
        manager.store.queued().expect("no stale queue entry").len(),
        1
    );
}

#[test]
fn startup_retires_deleted_group_sources_even_when_no_task_was_allocated() {
    let fixture = Fixture::new();
    settings::update(&fixture.home, |s| {
        s.voice_secretary.profile_id.clear();
        Ok(())
    })
    .expect("unconfigure");
    request(&fixture.home,"assistant_voice_document_instruction",json!({"group_id":fixture.group,"instruction":"Saved source","input_append_id":"delete-unconfigured"})).expect("save only");
    assert!(
        task_store(&fixture.home)
            .list()
            .expect("no jobs")
            .is_empty()
    );
    GroupStore::new(fixture.home.clone())
        .expect("groups")
        .delete(&fixture.group)
        .expect("simulate committed delete before source cleanup");
    let path = fixture
        .home
        .root()
        .join("voice-secretary")
        .join(&fixture.group);
    assert!(path.exists());
    stop(&fixture.home).expect("stop");
    start_unit_owner(&fixture.home).expect("recover cleanup");
    assert!(!path.exists());
}

#[test]
fn profiles_match_analyst_structured_runtimes_and_exclude_native_tui() {
    use cccc_contracts::RuntimeMode;
    let fixture = Fixture::new();
    let profiles = ProfileStore::new(fixture.home.clone()).expect("profiles");
    let mut expected = Vec::new();
    for runtime in [
        ActorRuntime::Codex,
        ActorRuntime::Claude,
        ActorRuntime::Grok,
        ActorRuntime::Opencode,
        ActorRuntime::Kilo,
        ActorRuntime::Antigravity,
        ActorRuntime::Copilot,
        ActorRuntime::Devin,
        ActorRuntime::Cursor,
    ] {
        let mode = if runtime.supports_acp_mode() {
            RuntimeMode::Acp
        } else {
            RuntimeMode::Default
        };
        let profile = profiles.upsert(json!({"name":format!("{runtime:?} structured"), "runtime":runtime, "runtime_mode":mode})
            .as_object().expect("profile").clone(), None).expect("profile");
        expected.push(profile["id"].clone());
        settings::update(&fixture.home, |s| {
            s.voice_secretary.profile_id = profile["id"].as_str().expect("id").into();
            Ok(())
        })
        .expect("select");
        let resolved = resolve_runtime(&fixture.home).expect("resolve all supported adapters");
        assert_eq!(resolved.runtime, runtime);
        assert_eq!(resolved.runtime_mode, mode);
        // Reuse the existing Analyst resolver to enforce alignment.
        let analyst = cccc_core::codex_voice_settings::resolve(
            &fixture.home,
            &cccc_contracts::CodexVoiceAnalystSettings {
                profile_id: profile["id"].as_str().expect("id").into(),
                ..Default::default()
            },
            &BTreeMap::new(),
        )
        .expect("Analyst parity");
        assert_eq!(analyst.runtime, resolved.runtime);
        if runtime.supports_acp_mode() {
            profiles
                .upsert(
                    json!({"name":format!("{runtime:?} TUI"), "runtime":runtime})
                        .as_object()
                        .expect("profile")
                        .clone(),
                    None,
                )
                .expect("TUI profile");
        }
    }
    profiles
        .upsert(
            json!({"name":"Amp native", "runtime":"amp"})
                .as_object()
                .expect("profile")
                .clone(),
            None,
        )
        .expect("native profile");
    let state = request(&fixture.home, "voice_secretary_settings_get", json!({})).expect("list");
    let ids: Vec<_> = state["profiles"]
        .as_array()
        .expect("profiles")
        .iter()
        .map(|p| p["id"].clone())
        .collect();
    assert_eq!(ids.len(), expected.len() + 1); // Original Codex fixture remains selectable.
    assert!(expected.iter().all(|id| ids.contains(id)));
    assert!(
        state["profiles"].as_array().expect("profiles").iter().all(
            |p| p["name"] != "Amp native" && !p["name"].as_str().unwrap_or("").ends_with("TUI")
        )
    );
}

#[test]
fn unisolated_document_work_is_not_automatically_replayed_even_with_codex_selected() {
    let fixture = Fixture::new();
    request(
        &fixture.home,
        "assistant_voice_document_save",
        json!({"group_id":fixture.group,"document_path":"notes.md","content":"Original"}),
    )
    .expect("document");
    let source=request(&fixture.home,"assistant_voice_transcript_append",json!({"group_id":fixture.group,"session_id":"asr","segment_id":"one","document_path":"notes.md","text":"Budget 42","is_final":true,"flush":true})).expect("source");
    let id = source["secretary_task_id"].as_str().expect("task");
    let manager = lookup(&fixture.home).expect("owner");
    for (phase, reason) in [
        (SecretaryTaskPhase::Unconfirmed, ""),
        (SecretaryTaskPhase::Failed, ""),
        (SecretaryTaskPhase::Conflict, ""),
        (SecretaryTaskPhase::Cancelled, "shutdown"),
    ] {
        manager
            .store
            .update(id, |t| {
                t.phase = phase;
                t.cancellation_reason = reason.into();
                t.isolated_document_writes = false;
                Ok(())
            })
            .expect("outcome");
        block_on(manager.recover_document(&manager.store.load(id).expect("task"))).expect("check");
        assert!(manager.store.successor(id).expect("successor").is_none());
    }
    assert_eq!(
        group_projection(&fixture.home, &fixture.group).expect("visible gap")["unprocessed_document_sources"],
        1
    );
    manager
        .store
        .update(id, |t| {
            t.phase = SecretaryTaskPhase::Unconfirmed;
            Ok(())
        })
        .expect("unconfirmed");
    let rejected = request(
        &fixture.home,
        "voice_secretary_task_retry",
        json!({"group_id":fixture.group,"task_id":id}),
    );
    assert!(
        rejected.is_err(),
        "uncertain work requires explicit acknowledgement"
    );
    request(
        &fixture.home,
        "voice_secretary_task_retry",
        json!({"group_id":fixture.group,"task_id":id,"confirm_unconfirmed":true}),
    )
    .expect("explicit retry");
}

#[test]
fn invalid_source_target_does_not_block_later_valid_sources() {
    let fixture = Fixture::new();
    let profile = settings::load(&fixture.home)
        .expect("settings")
        .voice_secretary
        .profile_id;
    settings::update(&fixture.home, |settings| {
        settings.voice_secretary.profile_id.clear();
        Ok(())
    })
    .expect("pause admission");
    for id in ["bad-source", "good-source"] {
        request(
            &fixture.home,
            "assistant_voice_document_instruction",
            json!({"group_id":fixture.group,"instruction":id,"input_append_id":id}),
        )
        .expect("saved input");
    }
    let manager = lookup(&fixture.home).expect("owner");
    let key = manager
        .pending_sources
        .lock()
        .expect("sources")
        .keys()
        .next()
        .expect("first")
        .clone();
    manager
        .pending_sources
        .lock()
        .expect("sources")
        .get_mut(&key)
        .expect("source")["secretary_target"] = json!({"group_id":fixture.group,"bad_schema":true});
    settings::update(&fixture.home, |settings| {
        settings.voice_secretary.profile_id = profile;
        Ok(())
    })
    .expect("configure");
    manager.reconcile_sources().expect("later sources continue");
    assert_eq!(manager.store.queued().expect("queue").len(), 1);
    let counts = source_counts(&fixture.home, &fixture.group).expect("coverage");
    assert_eq!(counts.invalid, 1);
    assert_eq!(
        manager.pending_sources.lock().expect("source retained")[&key]["text"],
        "Task:\nbad-source"
    );
    manager
        .reconcile_sources()
        .expect("quarantined input is not retried");
    assert_eq!(manager.store.queued().expect("one accepted task").len(), 1);
    let result = request(
        &fixture.home,
        "voice_secretary_tasks",
        json!({"group_id":fixture.group}),
    )
    .expect("visible diagnostic");
    assert_eq!(result["invalid_sources"], 1);
    let input_file = fixture
        .home
        .root()
        .join("voice-secretary")
        .join(&fixture.group)
        .join("input_events.jsonl");
    let saved = std::fs::read_to_string(&input_file).expect("canonical source");
    let mut rows = saved
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("row"))
        .collect::<Vec<_>>();
    rows[0]["secretary_target"] = json!({"group_id":fixture.group,"bad_schema":true});
    let corrupt = rows
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>();
    std::fs::write(&input_file, &corrupt).expect("isolated invalid schema");
    stop(&fixture.home).expect("stop");
    start_unit_owner(&fixture.home).expect("restart despite invalid row");
    assert_eq!(
        source_counts(&fixture.home, &fixture.group)
            .expect("restored diagnostic")
            .invalid,
        1
    );
    assert_eq!(
        std::fs::read_to_string(&input_file).expect("original retained"),
        corrupt
    );
    assert_eq!(
        task_store(&fixture.home)
            .list()
            .expect("no duplicate")
            .len(),
        1
    );
}

#[test]
fn explicit_backlog_hold_survives_restart_and_accepts_new_input() {
    let fixture = Fixture::new();
    let manager = lookup(&fixture.home).expect("owner");
    // Stop only this fixture's empty workers so settings assertions cannot race
    // execution. The configured command is an unavailable fixture, never Codex.
    let workers = std::mem::take(&mut *manager.workers.lock().expect("workers"));
    block_on(async {
        for worker in workers {
            worker.abort();
            let _ = worker.await;
        }
    });
    let profile = settings::load(&fixture.home)
        .expect("settings")
        .voice_secretary
        .profile_id;
    settings::update(&fixture.home, |settings| {
        settings.voice_secretary.profile_id.clear();
        Ok(())
    })
    .expect("unconfigure");
    request(
        &fixture.home,
        "assistant_voice_document_instruction",
        json!({"group_id":fixture.group,"instruction":"Saved old work","input_append_id":"old"}),
    )
    .expect("saved");
    let args = json!({"settings":{"profile_id":profile}});
    let error = request(
        &fixture.home,
        "voice_secretary_settings_update",
        args.clone(),
    )
    .expect_err("choice required");
    assert_eq!(error.code, "secretary_backlog_choice_required");
    assert!(
        settings::load(&fixture.home)
            .expect("no partial save")
            .voice_secretary
            .profile_id
            .is_empty()
    );
    let mut hold = args.clone();
    hold["backlog_action"] = json!("hold");
    let result = request(&fixture.home, "voice_secretary_settings_update", hold).expect("hold");
    assert_eq!(result["held_sources"], 1);
    manager.reconcile_sources().expect("held boundary");
    assert!(manager.store.list().expect("no old task").is_empty());
    fixture.ask("New work", "new");
    assert_eq!(manager.store.list().expect("new work accepted").len(), 1);
    stop(&fixture.home).expect("shutdown");
    start_unit_owner(&fixture.home).expect("restore");
    assert_eq!(
        source_counts(&fixture.home, &fixture.group)
            .expect("retained")
            .held,
        1
    );
    let mut release = args;
    release["backlog_action"] = json!("process");
    request(&fixture.home, "voice_secretary_settings_update", release)
        .expect("release saved input");
    lookup(&fixture.home)
        .expect("owner")
        .reconcile_sources()
        .expect("drain");
    assert_eq!(
        source_counts(&fixture.home, &fixture.group)
            .expect("released")
            .held,
        0
    );
    let tasks = task_store(&fixture.home).list().expect("jobs");
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().any(|task| {
        task.inputs[0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Saved old work"))
    }));
}

#[test]
fn custom_secretary_runtime_and_private_environment_are_independent_from_analyst() {
    let fixture = Fixture::new();
    let analyst_before = settings::load(&fixture.home)
        .expect("settings")
        .codex_voice
        .analyst;
    cccc_core::codex_voice_settings::replace_private_environment(
        &fixture.home,
        &BTreeMap::from([("ANALYST_FIXTURE".into(), "analyst-only".into())]),
    )
    .expect("analyst env");
    let saved = request(&fixture.home, "voice_secretary_settings_update", json!({
        "settings":{"runtime":"copilot","runtime_mode":"acp","command":["copilot","--model","fixture-model","--allow-all"]},
        "environment":{"set":{"SECRETARY_FIXTURE":"secretary-only"}}
    })).expect("custom settings");
    assert_eq!(saved["settings"]["runtime"], "copilot");
    assert_eq!(saved["environment_keys"], json!(["SECRETARY_FIXTURE"]));
    assert!(
        !serde_json::to_string(&saved)
            .expect("public settings")
            .contains("secretary-only")
    );
    let resolved = resolve_runtime(&fixture.home).expect("custom runtime");
    assert_eq!(resolved.runtime, cccc_contracts::ActorRuntime::Copilot);
    assert_eq!(resolved.runtime_mode, cccc_contracts::RuntimeMode::Acp);
    assert!(resolved.command.contains(&"fixture-model".to_owned()));
    assert!(resolved.environment.contains_key("SECRETARY_FIXTURE"));
    assert!(!resolved.environment.contains_key("ANALYST_FIXTURE"));
    assert_eq!(
        settings::load(&fixture.home)
            .expect("settings")
            .codex_voice
            .analyst,
        analyst_before
    );
    let rejected = request(
        &fixture.home,
        "voice_secretary_settings_update",
        json!({
            "settings":{"runtime":"copilot","runtime_mode":"default"},
            "environment":{"clear":true}
        }),
    );
    assert!(rejected.is_err());
    assert_eq!(
        cccc_core::voice_secretary_settings::private_environment(&fixture.home)
            .expect("preserved env")
            .len(),
        1
    );
    assert_eq!(
        resolve_runtime(&fixture.home)
            .expect("preserved runtime")
            .runtime_mode,
        cccc_contracts::RuntimeMode::Acp
    );
}

#[test]
fn preferences_only_save_does_not_release_held_input_or_require_a_backlog_choice() {
    let fixture = Fixture::new();
    let profile = settings::load(&fixture.home)
        .expect("settings")
        .voice_secretary
        .profile_id;
    settings::update(&fixture.home, |s| {
        s.voice_secretary.profile_id.clear();
        Ok(())
    })
    .expect("pause");
    request(&fixture.home, "assistant_voice_document_instruction", json!({"group_id":fixture.group,"instruction":"Retain old input","input_append_id":"held-global-prefs"})).expect("source saved");
    request(
        &fixture.home,
        "voice_secretary_settings_update",
        json!({"settings":{"profile_id":profile},"backlog_action":"hold"}),
    )
    .expect("hold");
    let mut setting = settings::load(&fixture.home)
        .expect("settings")
        .voice_secretary;
    setting.config.recognition_language = "ja-JP".into();
    let saved = request(
        &fixture.home,
        "voice_secretary_settings_update",
        json!({"settings":setting}),
    )
    .expect("preferences saved");
    assert_eq!(saved["held_sources"], 1);
    assert!(
        task_store(&fixture.home)
            .list()
            .expect("no implicit work")
            .is_empty()
    );
}

#[test]
fn preference_patches_preserve_runtime_environment_and_other_preferences() {
    let fixture = Fixture::new();
    let prior = settings::load(&fixture.home).expect("settings");
    let private = BTreeMap::from([("SECRETARY_FIXTURE".into(), "synthetic-only".into())]);
    cccc_core::voice_secretary_settings::replace_private_environment(&fixture.home, &private)
        .expect("private fixture");
    request(
        &fixture.home,
        "voice_secretary_settings_update",
        json!({
            "preferences":{"recognition_language":"ja-JP","guidance":"Keep every budget fact"}
        }),
    )
    .expect("preferences without provider availability");
    let saved = request(
        &fixture.home,
        "voice_secretary_settings_update",
        json!({
            "preferences":{"auto_document_max_window_seconds":null}
        }),
    )
    .expect("disable periodic checkpoints");
    assert_eq!(saved["settings"]["config"]["recognition_language"], "ja-JP");
    assert_eq!(
        saved["settings"]["config"]["guidance"],
        "Keep every budget fact"
    );
    assert!(saved["settings"]["config"]["auto_document_max_window_seconds"].is_null());
    let latest = settings::load(&fixture.home).expect("latest");
    assert_eq!(
        latest.voice_secretary.runtime_settings(),
        prior.voice_secretary.runtime_settings()
    );
    assert_eq!(latest.codex_voice, prior.codex_voice);
    assert_eq!(
        cccc_core::voice_secretary_settings::private_environment(&fixture.home).expect("private"),
        private
    );
    assert!(
        task_store(&fixture.home)
            .list()
            .expect("no task")
            .is_empty()
    );

    for args in [
        json!({"preferences":{"runtime":"cursor"}}),
        json!({"preferences":{"recognition_backend":"invalid"}}),
        json!({"preferences":{"auto_document_max_window_seconds":9}}),
        json!({"preferences":{"recognition_language":"en-US"},"environment":{"clear":true}}),
        json!({"preferences":{"recognition_language":"en-US"},"settings":{}}),
        json!({"preferences":{"recognition_language":"en-US"},"backlog_action":"process"}),
    ] {
        request(&fixture.home, "voice_secretary_settings_update", args).expect_err("invalid patch");
        assert_eq!(
            settings::load(&fixture.home)
                .expect("unchanged")
                .voice_secretary,
            latest.voice_secretary
        );
        assert_eq!(
            cccc_core::voice_secretary_settings::private_environment(&fixture.home)
                .expect("unchanged private"),
            private
        );
    }

    let mut runtime = serde_json::to_value(&latest.voice_secretary).expect("runtime");
    runtime.as_object_mut().expect("object").remove("config");
    request(
        &fixture.home,
        "voice_secretary_settings_update",
        json!({"settings":runtime}),
    )
    .expect("save only runtime");
    assert_eq!(
        settings::load(&fixture.home)
            .expect("preserved")
            .voice_secretary
            .config,
        latest.voice_secretary.config
    );
}

#[test]
fn task_progress_is_bounded_redacted_and_fenced_to_its_turn() {
    let fixture = Fixture::new();
    let task = fixture.ask("Read the requested file", "progress-fence");
    let token = fixture.grant(&task);
    let owner = lookup(&fixture.home).expect("owner");
    let execution = owner
        .grants
        .lock()
        .expect("grants")
        .get(&digest(token.as_bytes()))
        .expect("execution")
        .clone();
    let config = ResolvedAgentRuntime {
        runtime: ActorRuntime::Copilot,
        runtime_mode: cccc_contracts::RuntimeMode::Acp,
        command: vec![],
        environment: BTreeMap::from([(
            "PRIVATE_TEST_VALUE".into(),
            "fixture-private-value".into(),
        )]),
    };
    execution.observe_progress(&json!({"method":"item/agentMessage/delta","params":{"turnId":"old-turn","delta":"wrong target"}}), "turn-1", &config, &token);
    assert!(
        execution
            .progress
            .lock()
            .expect("progress")
            .snapshot()
            .is_empty()
    );
    execution.observe_progress(&json!({"method":"item/agentMessage/delta","params":{"turnId":"turn-1","delta":format!("{} {} {}", "語".repeat(4000), token, "fixture-private-value")}}), "turn-1", &config, &token);
    let progress = execution.progress.lock().expect("progress").snapshot();
    assert!(progress.len() <= 8192);
    assert!(!progress.contains(&token));
    assert!(!progress.contains("fixture-private-value"));
    execution.observe_progress(&json!({"method":"cccc/toolActivity","params":{"turnId":"turn-1","title":"Checking fixture-private-value"}}), "turn-1", &config, &token);
    assert_eq!(
        *execution.activity.lock().expect("activity"),
        "Checking [redacted]"
    );
}

#[test]
fn task_progress_withholds_secret_prefixes_across_every_stream_split() {
    let fixture = Fixture::new();
    let task = fixture.ask("Read only the task materials", "split-redaction");
    let token = fixture.grant(&task);
    let owner = lookup(&fixture.home).expect("owner");
    let execution = owner
        .grants
        .lock()
        .expect("grants")
        .get(&digest(token.as_bytes()))
        .expect("execution")
        .clone();
    let config = ResolvedAgentRuntime {
        runtime: ActorRuntime::Copilot,
        runtime_mode: cccc_contracts::RuntimeMode::Acp,
        command: vec![],
        environment: BTreeMap::from([
            ("PRIVATE_TEST_VALUE".into(), "fixture-private-value".into()),
            ("PRIVATE_OVERLAP".into(), "fixture-private".into()),
            ("PRIVATE_UNICODE".into(), "秘密の値🔑".into()),
        ]),
    };
    let observe = |method: &str, text: &str| {
        let params = if method == "item/completed" {
            json!({"turnId":"turn-1","item":{"type":"agentMessage","text":text}})
        } else {
            json!({"turnId":"turn-1","delta":text})
        };
        execution.observe_progress(
            &json!({"method":method,"params":params}),
            "turn-1",
            &config,
            &token,
        );
    };
    for secret in config.environment.values().chain(std::iter::once(&token)) {
        for (split, _) in secret.char_indices().skip(1) {
            observe("item/completed", "");
            observe(
                "item/agentMessage/delta",
                &format!("Output: {}", &secret[..split]),
            );
            assert!(
                execution.progress.lock().expect("progress").snapshot() == "Output: ",
                "secret prefixes must not be published"
            );
            observe("item/agentMessage/delta", &format!("{}|", &secret[split..]));
            assert!(
                execution.progress.lock().expect("progress").snapshot() == "Output: [redacted]|"
            );
        }
    }
    observe("item/completed", "");
    observe("item/agentMessage/delta", "fix");
    assert!(
        execution
            .progress
            .lock()
            .expect("progress")
            .snapshot()
            .is_empty()
    );
    observe("item/agentMessage/delta", "ation");
    assert!(execution.progress.lock().expect("progress").snapshot() == "fixation");
    observe("item/completed", "fix");
    assert!(execution.progress.lock().expect("progress").snapshot() == "fix");
    observe("item/agentMessage/delta", "ture-private-value|");
    assert!(
        execution.progress.lock().expect("progress").snapshot() == "ture-private-value|",
        "the next message must not reconstruct a private value from a completed preview"
    );
}

#[test]
fn task_display_uses_user_input_and_matching_draft_ack_without_starting_work() {
    let fixture = Fixture::new();
    let ask = fixture.ask("本日の会議の予算は？", "display-ask");
    assert_eq!(ask.preview(), "本日の会議の予算は？");
    let created = request(
        &fixture.home,
        "assistant_voice_input_append",
        json!({
            "group_id":fixture.group, "kind":"prompt_refine", "composer_text":"请整理这段话",
            "request_id":"display-prompt", "input_append_id":"display-input"
        }),
    )
    .expect("prompt");
    let jobs = task_store(&fixture.home);
    let task = jobs
        .load(created["secretary_task_id"].as_str().expect("id"))
        .expect("task");
    assert_eq!(task.preview(), "请整理这段话");
    assert!(
        task.inputs[0]["text"]
            .as_str()
            .expect("provider input")
            .contains("Target: composer")
    );
    let token = fixture.grant(&task);
    request(
        &fixture.home,
        "voice_secretary_task",
        json!({"_cccc_secretary_token":token, "action":"draft", "draft_text":"整理后的内容"}),
    )
    .expect("draft");
    jobs.update(&task.task_id, |t| {
        t.phase = SecretaryTaskPhase::Done;
        t.cleanup_confirmed = true;
        Ok(())
    })
    .expect("done");
    block_on(
        lookup(&fixture.home)
            .expect("owner")
            .project(&jobs.load(&task.task_id).expect("task")),
    );
    let read = || {
        let result = request(
            &fixture.home,
            "voice_secretary_tasks",
            json!({"group_id":fixture.group}),
        )
        .expect("read");
        result["tasks"]
            .as_array()
            .expect("tasks")
            .iter()
            .find(|t| t["task_id"] == task.task_id)
            .expect("task view")
            .clone()
    };
    assert_eq!(read()["prompt_draft_status"], "pending");
    request(
        &fixture.home,
        "assistant_voice_prompt_draft_ack",
        json!({"group_id":fixture.group,"request_id":"display-prompt","status":"applied"}),
    )
    .expect("ack");
    assert_eq!(read()["prompt_draft_status"], "applied");
    jobs.retire_artifacts(&task.task_id).expect("retire");
    assert_eq!(read()["preview"], "请整理这段话");
    cccc_core::assistant_state::update(&fixture.home, &fixture.group, |state| {
        state["voice_prompt_drafts"]["display-prompt"]["secretary_task_id"] =
            json!("replacement-task");
        Ok(())
    })
    .expect("replacement draft");
    assert!(
        read()["prompt_draft_status"].is_null(),
        "another attempt's ack is not this task's ack"
    );
    assert_eq!(
        jobs.list().expect("tasks").len(),
        2,
        "reads cannot create tasks"
    );
    let mut without_summary = jobs.load(&task.task_id).expect("task");
    without_summary.inputs[0]["metadata"] = Value::Null;
    assert!(
        projection(&without_summary)["preview"]
            .as_str()
            .expect("preview")
            .is_empty(),
        "never expose internal instructions as a title"
    );
}

#[test]
fn reused_session_grant_requires_the_exact_current_task_id() {
    let fixture = Fixture::new();
    let old = fixture.ask("old input", "old-task");
    let token = fixture.grant(&old);
    let next = fixture.ask("new input", "new-task");
    let next_token = fixture.grant(&next);
    let owner = lookup(&fixture.home).expect("owner");
    let next_execution = owner
        .grants
        .lock()
        .expect("grants")
        .remove(&digest(next_token.as_bytes()))
        .expect("next grant");
    owner
        .grants
        .lock()
        .expect("grants")
        .insert(digest(token.as_bytes()), next_execution);
    for id in [None, Some(old.task_id.as_str())] {
        let mut args = json!({"_cccc_secretary_token":token,"action":"report","status":"done","reply_text":"late old answer"});
        if let Some(id) = id {
            args["task_id"] = json!(id);
        }
        let req = DaemonRequest {
            v: 1,
            op: "voice_secretary_task".into(),
            args: args
                .as_object()
                .expect("isolated fixture invariant")
                .clone(),
        };
        assert_eq!(
            task_actions::call(&fixture.home, &req)
                .expect_err("stale or missing ID")
                .code,
            "secretary_task_expired"
        );
    }
    assert!(
        owner
            .store
            .load(&next.task_id)
            .expect("isolated fixture invariant")
            .receipt
            .is_none()
    );
    request(&fixture.home,"voice_secretary_task",json!({"_cccc_secretary_token":token,"task_id":next.task_id,"action":"report","status":"done","reply_text":"current answer"})).expect("current task");
    assert_eq!(
        owner
            .store
            .load(&next.task_id)
            .expect("isolated fixture invariant")
            .receipt
            .expect("isolated fixture invariant")
            .output["reply_text"],
        "current answer"
    );
}
