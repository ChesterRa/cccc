#![cfg(target_os = "linux")]

use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, Registry};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};

const BLOCKED: &str = "owned-cleanup-blocker";

struct Fixture {
    home: HomeLayout,
    store: GroupStore,
    group_id: String,
    child: Child,
    _temp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("owned temporary home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("partial retirement", "").expect("group");
        let child = Command::new("setsid")
            .args(["/bin/sh", "-c", "printf 'fixture-ready\\n'; read -r _"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", temp.path())
            .env("CCCC_HOME", home.root())
            .env("CCCC_GROUP_ID", &group.group_id)
            .env("CCCC_ACTOR_ID", "owned-detached")
            .current_dir(temp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("own detached child");
        // Own cleanup before reading readiness or making any assertion.
        let mut fixture = Self {
            home,
            store,
            group_id: group.group_id,
            child,
            _temp: temp,
        };
        let output = fixture.child.stdout.take().expect("readiness pipe");
        let mut ready = String::new();
        BufReader::new(output)
            .read_line(&mut ready)
            .expect("readiness");
        assert_eq!(ready, "fixture-ready\n");
        fixture
    }

    /// Root/CAP_DAC_OVERRIDE bypass mode bits. Do not mistake a successful
    /// cleanup there for evidence that the permission-failure path was tested.
    fn block_cleanup(&self) -> bool {
        let sealed = self.group_directory().join(BLOCKED);
        std::fs::create_dir(&sealed).expect("blocker directory");
        std::fs::write(sealed.join("sentinel"), b"owned synthetic file").expect("sentinel");
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).expect("seal");
        if std::fs::read_dir(&sealed).is_ok() {
            eprintln!("permission-failure case skipped: this process bypasses directory mode bits");
            return false;
        }
        true
    }

    fn group_directory(&self) -> std::path::PathBuf {
        self.store
            .group_dir(&self.group_id)
            .expect("group directory")
    }

    fn call(&self, op: &str, by: &str, extra: Value) -> DaemonResponse {
        let mut args = extra.as_object().cloned().expect("args");
        args.insert("group_id".into(), json!(self.group_id));
        args.insert("by".into(), json!(by));
        cccc_daemon::handle_request(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args,
            },
        )
    }

    fn assert_retired(&self) {
        assert!(!self.group_directory().exists());
        assert!(
            !Registry::load(&self.home)
                .expect("registry")
                .groups
                .contains_key(&self.group_id)
        );
        let retry = self.call("group_delete", "user", json!({}));
        assert_eq!(
            retry.error.expect("retired Group cannot be loaded").code,
            "not_found"
        );
    }

    fn assert_child_exited(&mut self) {
        assert!(
            self.child.try_wait().expect("own child status").is_some(),
            "stopped Group must not strand its escaped child after file cleanup fails"
        );
    }

    fn unseal(&self) {
        let parent = self.home.groups_dir();
        let mut paths = vec![self.group_directory().join(BLOCKED)];
        if let Ok(entries) = std::fs::read_dir(parent) {
            let prefix = format!(".{}.deleting-", self.group_id);
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&prefix) {
                    paths.push(entry.path().join(BLOCKED));
                }
            }
        }
        for path in paths {
            if path.exists() {
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.unseal();
    }
}

#[test]
fn delete_file_cleanup_failure_does_not_skip_stopped_groups_escaped_child() {
    let mut fixture = Fixture::new();
    if !fixture.block_cleanup() {
        return;
    }
    let response = fixture.call("group_delete", "user", json!({}));
    let error = response
        .error
        .expect("filesystem cleanup failure remains visible");
    assert_eq!(error.code, "io_error");
    assert!(error.message.contains("rollback_failed"));
    fixture.assert_retired();
    fixture.assert_child_exited();
}

#[test]
fn reset_file_cleanup_failure_reports_reaped_child_and_keeps_replacement() {
    let mut fixture = Fixture::new();
    if !fixture.block_cleanup() {
        return;
    }
    let response = fixture.call("group_reset", "user", json!({"confirm":fixture.group_id}));
    assert!(
        response.ok,
        "Reset preserves its successfully created replacement"
    );
    assert_eq!(response.result["deleted_old"], false);
    assert!(
        response.result["old_delete_error"]
            .as_str()
            .is_some_and(|e| e.contains("rollback_failed"))
    );
    let new_group = response.result["new_group_id"]
        .as_str()
        .expect("replacement ID");
    assert_ne!(new_group, fixture.group_id);
    assert!(fixture.store.load(new_group).is_ok());
    fixture.assert_retired();
    fixture.assert_child_exited();
    let reports = response.result["old_reaped_processes"]
        .as_array()
        .expect("reap report");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["pid"], fixture.child.id());
    assert_eq!(reports[0]["actor_id"], "owned-detached");
    assert_eq!(reports[0]["signaled_term"], true);
    assert_eq!(reports[0]["signaled_kill"], false);
    assert_eq!(reports[0]["outcome"], "exited");
}

#[test]
fn missing_confirmation_or_group_authority_never_stops_the_attributed_child() {
    let mut fixture = Fixture::new();
    let group_path = fixture.group_directory().join("group.yaml");
    let before_group = std::fs::read(&group_path).expect("group bytes");
    let before_registry = std::fs::read(fixture.home.registry_path()).expect("registry bytes");
    let before_ledger = std::fs::read(
        fixture
            .store
            .ledger_path(&fixture.group_id)
            .expect("ledger path"),
    )
    .ok();
    for (op, by, extra) in [
        ("group_delete", "outsider", json!({})),
        (
            "group_reset",
            "outsider",
            json!({"confirm":fixture.group_id}),
        ),
        ("group_reset", "user", json!({})),
        ("group_reset", "user", json!({"confirm":"different-group"})),
    ] {
        assert!(!fixture.call(op, by, extra).ok);
        assert!(
            fixture
                .child
                .try_wait()
                .expect("own child status")
                .is_none()
        );
        assert_eq!(
            std::fs::read(&group_path).expect("group retained"),
            before_group
        );
        assert_eq!(
            std::fs::read(fixture.home.registry_path()).expect("registry retained"),
            before_registry
        );
        assert_eq!(
            std::fs::read(
                fixture
                    .store
                    .ledger_path(&fixture.group_id)
                    .expect("ledger path")
            )
            .ok(),
            before_ledger
        );
    }
}
