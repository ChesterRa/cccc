//! Real dispatch, PTY and recovery registration; only the external Claude CLI is a fixture.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[path = "actors_confirmation_pause_tests.rs"]
mod pause_tests;
#[path = "actors_confirmation_retirement_tests.rs"]
mod retirement_tests;

fn isolated(test: &str) -> bool {
    const CHILD: &str = "CCCC_TEST_ACTOR_CONFIRMATION";
    if std::env::var(CHILD).ok().as_deref() == Some(test) {
        return false;
    }
    let temp = tempfile::tempdir().expect("isolated process");
    let launcher = temp.path().join("cccc");
    std::fs::write(&launcher, "#!/bin/sh\nexit 0\n").expect("fixture launcher");
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))
        .expect("launcher permissions");
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            &format!("ops::actors::confirmation_tests::{test}"),
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, test)
        .env("CCCC_LAUNCHER_PATH", launcher)
        .env("CCCC_HOME", temp.path().join("cccc-home"))
        .env("HOME", temp.path().join("provider-home"))
        .env("USERPROFILE", temp.path().join("provider-home"))
        .env("CLAUDE_CONFIG_DIR", temp.path().join("claude-config"))
        .output()
        .expect("isolated test");
    crate::ops::isolated_test::assert_one_passed(output, test);
    true
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let config = temp.path().join("claude-config");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&config).expect("private provider config");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let executable = temp.path().join("claude");
        std::fs::write(&executable, "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '2.1.286 (Claude Code)\\n'\n  exit 0\nfi\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"--bg\" ]; then\n    printf 'launch\\n' >> \"$CLAUDE_CONFIG_DIR/launches\"\n    printf 'Workspace not trusted\\n' >&2\n    exit 1\n  fi\ndone\nif [ -f \"$CLAUDE_CONFIG_DIR/immediate_exit\" ]; then exit 0; fi\nprintf 'Fixture workspace confirmation\\n'\nexec sleep 60\n").expect("fake Claude");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("executable permissions");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let created = store.create("Actor confirmation", "").expect("group");
        let mut group = group_scope::attach(
            &store,
            &created.group_id,
            cccc_core::scope::detect(&workspace).expect("scope"),
        )
        .expect("attach scope");
        let mut actor = Actor::new("claude");
        actor.runtime = ActorRuntime::Claude;
        actor.command = vec![executable.to_string_lossy().into_owned()];
        actor.env = BTreeMap::from([
            (
                "CLAUDE_CONFIG_DIR".into(),
                config.to_string_lossy().into_owned(),
            ),
            (
                "HOME".into(),
                temp.path()
                    .join("provider-home")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ]);
        group.actors.push(actor);
        group.running = true;
        store.save(&group).expect("save fixture");
        Self {
            _temp: temp,
            home,
            group,
            config,
        }
    }

    fn request(&self, op: &str, patch: Value) -> cccc_contracts::DaemonResponse {
        crate::handle_request(&self.home, &DaemonRequest {
            v: 1,
            op: op.into(),
            args: json!({"group_id":self.group.group_id,"actor_id":"claude","by":"user","patch":patch})
                .as_object().expect("args").clone(),
        })
    }

    fn stored(&self) -> GroupDoc {
        GroupStore::new(self.home.clone())
            .expect("store")
            .load(&self.group.group_id)
            .expect("stored Group")
    }

    fn listed(&self) -> Value {
        let result = self.request("actor_list", Value::Null);
        assert!(result.ok, "{:?}", result.error);
        result.result["actors"][0].clone()
    }

    fn fail_ledger(&self) {
        let store = GroupStore::new(self.home.clone()).expect("store");
        let ledger = store.ledger_path(&self.group.group_id).expect("ledger");
        std::fs::remove_file(&ledger).expect("remove synthetic ledger");
        std::fs::create_dir(&ledger).expect("inject failed event append");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        crate::ops::actor_delivery::shutdown_actor(&self.group.group_id, "claude");
        let _ = crate::ops::local_headless::stop(&self.group.group_id, "claude");
        let _ = cccc_runtime::stop(&self.group.group_id, "claude");
    }
}

#[test]
fn waiting_confirmation_is_visible_in_list_and_lifecycle_results_without_restart_on_save() {
    if isolated(
        "waiting_confirmation_is_visible_in_list_and_lifecycle_results_without_restart_on_save",
    ) {
        return;
    }
    let fixture = Fixture::new();
    assert_eq!(fixture.listed()["runtime_confirmation_required"], false);
    let started = fixture.request("actor_start", Value::Null);
    assert!(started.ok, "{:?}", started.error);
    assert_eq!(started.result["runtime_confirmation_required"], true);
    assert_eq!(
        started.result["actor"]["runtime_confirmation_required"],
        true
    );
    let before = actor_runtime::status(&fixture.group.group_id, "claude").expect("prompt");
    assert_eq!(fixture.listed()["runtime_confirmation_required"], true);
    assert_eq!(fixture.listed()["runtime_state_source"], "terminal");
    let updated = fixture.request(
        "actor_update",
        json!({"title":"Saved title","runtime_confirmation_required":false}),
    );
    assert!(updated.ok, "{:?}", updated.error);
    assert_eq!(updated.result["runtime_confirmation_required"], true);
    assert_eq!(
        updated.result["actor"]["runtime_confirmation_required"],
        true
    );
    assert_eq!(
        actor_runtime::status(&fixture.group.group_id, "claude")
            .expect("same prompt")
            .pid,
        before.pid
    );
    assert_eq!(
        std::fs::read_to_string(fixture.config.join("launches"))
            .expect("launch log")
            .lines()
            .count(),
        1
    );
    assert!(
        json!(fixture.stored().actors[0])
            .get("runtime_confirmation_required")
            .is_none(),
        "response metadata must not become Actor configuration"
    );
    let restarted = fixture.request("actor_restart", Value::Null);
    assert!(restarted.ok, "{:?}", restarted.error);
    assert_eq!(restarted.result["runtime_confirmation_required"], true);
    let stopped = fixture.request("actor_stop", Value::Null);
    assert!(stopped.ok, "{:?}", stopped.error);
    assert_eq!(stopped.result["runtime_confirmation_required"], false);
    assert_eq!(fixture.listed()["runtime_confirmation_required"], false);
}

#[test]
fn rollback_reports_waiting_instead_of_restored_and_preserves_the_prompt() {
    if isolated("rollback_reports_waiting_instead_of_restored_and_preserves_the_prompt") {
        return;
    }
    for operation in ["actor_update", "actor_restart"] {
        let fixture = Fixture::new();
        let started = fixture.request("actor_start", Value::Null);
        assert!(started.ok, "{:?}", started.error);
        let original = fixture.stored();
        fixture.fail_ledger();
        let response = fixture.request(operation, json!({"enabled":false}));
        assert!(!response.ok);
        let error = response.error.expect("failed rollback");
        assert_eq!(error.code, "rollback_failed", "{}", error.message);
        assert!(
            error.message.contains("waiting for confirmation"),
            "{}",
            error.message
        );
        assert!(actor_runtime::confirmation_required(
            &fixture.group.group_id,
            "claude"
        ));
        assert!(
            actor_runtime::status(&fixture.group.group_id, "claude")
                .expect("retained prompt")
                .running
        );
        let restored = fixture.stored();
        assert_eq!(json!(restored.actors), json!(original.actors));
        assert_eq!(restored.running, original.running);
        assert_eq!(restored.state, original.state);
    }
}

#[test]
fn rollback_stays_incomplete_after_the_confirmation_terminal_immediately_exits() {
    if isolated("rollback_stays_incomplete_after_the_confirmation_terminal_immediately_exits") {
        return;
    }
    for operation in ["actor_update", "actor_restart"] {
        let fixture = Fixture::new();
        let started = fixture.request("actor_start", Value::Null);
        assert!(started.ok, "{:?}", started.error);
        let original = fixture.stored();
        fixture.fail_ledger();
        std::fs::write(fixture.config.join("immediate_exit"), "").expect("exit next prompt");
        let response = fixture.request(operation, json!({"enabled":false}));
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("incomplete restore").code,
            "rollback_failed"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while crate::ops::local_headless::waiting_for_workspace_trust(
            &fixture.group.group_id,
            "claude",
        )
        .expect("registry")
        {
            assert!(
                std::time::Instant::now() < deadline,
                "exited prompt stayed registered"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!actor_runtime::confirmation_required(
            &fixture.group.group_id,
            "claude"
        ));
        let error = actor_runtime::require_restored_managed_actor(
            &fixture.group.group_id,
            &original.actors[0],
        )
        .expect_err("a vanished prompt must not imply a restored managed owner");
        assert_eq!(error.code, "runtime_restore_incomplete");
        assert_eq!(error.details["runtime_confirmation_required"], false);
        assert_eq!(json!(fixture.stored().actors), json!(original.actors));
        let mut other = original.actors[0].clone();
        other.runtime = ActorRuntime::Custom;
        actor_runtime::require_restored_managed_actor(&fixture.group.group_id, &other)
            .expect("other Runtime restore policy is unchanged");
    }
}
