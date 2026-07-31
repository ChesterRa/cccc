use cccc_contracts::{DaemonRequest, Event};
use cccc_core::profiles::ProfileStore;
use cccc_core::settings::{self, GlobalSettings};
use cccc_core::{GroupStore, HomeLayout, inbox, ledger};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn python_and_rust_share_persisted_control_plane_state() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("interop", "").expect("group");
    let group_id = group.group_id.as_str();

    settings::save(
        &home,
        &GlobalSettings {
            branding: object(json!({"title":"Rust title"})),
            remote_access: object(json!({"web_host":"127.0.0.1"})),
            ..GlobalSettings::default()
        },
    )
    .expect("Rust settings");
    call(
        &home,
        "actor_profile_upsert",
        json!({
            "profile_id":"shared",
            "name":"Rust profile",
            "runtime":"codex",
            "env":{"PUBLIC_VALUE":"from-rust"}
        }),
    );
    call(
        &home,
        "actor_profile_secret_update",
        json!({
            "profile_id":"shared",
            "set":{"PRIVATE_VALUE":"from-rust"}
        }),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "runtime":"codex",
            "by":"user",
            "env_private":{"ACTOR_SECRET":"from-rust"}
        }),
    );

    let mut event = Event::new("chat.message", group_id);
    event.by = "user".into();
    event.data = object(json!({"to":["peer"],"text":"interop"}));
    ledger::append(&groups.ledger_path(group_id).expect("ledger path"), &event)
        .expect("ledger event");
    inbox::mark_read(&home, group_id, "peer", &event.id).expect("Rust cursor");
    groups
        .mutate(group_id, |group| {
            group.automation = json!({
                "rules":[{
                    "id":"rust-rule",
                    "enabled":true,
                    "trigger":{"kind":"interval","every_seconds":1},
                    "action":{"kind":"notify","title":"interop","message":"interop"}
                }]
            })
            .as_object()
            .cloned()
            .expect("automation");
            Ok(())
        })
        .expect("automation rule");
    assert_eq!(
        cccc_core::automation::tick_group(&home, group_id, false)
            .expect("Rust automation tick")
            .notifications
            .len(),
        1
    );

    let capability_dir = home.root().join("state/capabilities");
    std::fs::create_dir_all(&capability_dir).expect("capability dir");
    std::fs::write(
        capability_dir.join("catalog.json"),
        serde_json::to_vec_pretty(&json!({
            "v":1,
            "records":{
                "skill:test:shared":{
                    "capability_id":"skill:test:shared",
                    "kind":"skill",
                    "name":"shared",
                    "description_short":"interop"
                }
            }
        }))
        .expect("catalog JSON"),
    )
    .expect("catalog");
    call(
        &home,
        "capability_enable",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "scope":"session",
            "ttl_seconds":3600,
            "capability_id":"skill:test:shared",
            "enabled":true
        }),
    );
    call(
        &home,
        "capability_block",
        json!({
            "group_id":group_id,
            "by":"user",
            "scope":"group",
            "reason":"interop",
            "capability_id":"skill:test:shared",
            "blocked":true
        }),
    );
    let allowlist = call(
        &home,
        "capability_allowlist_update",
        json!({
            "by":"user",
            "mode":"replace",
            "overlay":{"defaults":{"source_level":{"manual_import":"indexed"}}}
        }),
    );
    let rust_allowlist_revision = allowlist["revision"]
        .as_str()
        .expect("allowlist revision")
        .to_owned();
    call(
        &home,
        "group_space_bind",
        json!({
            "group_id":group_id,
            "provider":"notebooklm",
            "lane":"work",
            "remote_space_id":"nb-rust",
            "by":"user"
        }),
    );
    call(
        &home,
        "group_space_provider_credential_update",
        json!({
            "provider":"notebooklm",
            "by":"user",
            "auth_json":"{\"cookies\":[],\"origins\":[]}"
        }),
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.kernel.group import load_group
from cccc.kernel.inbox import get_cursor, set_cursor
from cccc.kernel.settings import load_settings, save_settings
from cccc.daemon.actors.actor_profile_store import (
    get_actor_profile,
    load_actor_profile_secrets,
    update_actor_profile_secrets,
    upsert_actor_profile,
)
from cccc.daemon.actors.private_env_ops import (
    load_actor_private_env,
    update_actor_private_env,
)
from cccc.daemon.ops.capability_ops._documents import _load_state_doc, _save_state_doc
from cccc.daemon.ops.capability_ops._policy import (
    _allowlist_effective_snapshot,
    handle_capability_allowlist_update,
)
from cccc.daemon.ops.capability_ops._state import _set_enabled_capability
from cccc.daemon.automation.engine import _load_state as load_automation_state
from cccc.util.fs import atomic_write_json
from cccc.daemon.space.group_space_store import (
    enqueue_space_job,
    get_space_binding,
    load_space_provider_secrets,
    upsert_space_binding,
)

group_id, rust_allowlist_revision = sys.argv[1], sys.argv[2]
settings = load_settings()
assert settings["web_branding"]["title"] == "Rust title"
settings["web_branding"]["subtitle"] = "from-python"
save_settings(settings)

profile = get_actor_profile("shared")
assert profile["env"] == {"PUBLIC_VALUE": "from-rust"}
assert load_actor_profile_secrets("shared") == {"PRIVATE_VALUE": "from-rust"}
upsert_actor_profile(
    {**profile, "name": "Python profile", "env": {"PUBLIC_VALUE": "from-python"}},
    expected_revision=1,
)
update_actor_profile_secrets(
    "shared",
    set_vars={"PYTHON_SECRET": "from-python"},
    unset_keys=[],
    clear=False,
)

assert load_actor_private_env(group_id, "peer") == {"ACTOR_SECRET": "from-rust"}
update_actor_private_env(
    group_id,
    "peer",
    set_vars={"PYTHON_ACTOR_SECRET": "from-python"},
    unset_keys=[],
    clear=False,
)

group = load_group(group_id)
assert group is not None
event_id, ts = get_cursor(group, "peer")
assert event_id
assert ts
set_cursor(group, "peer", event_id="python-cursor", ts="2999-01-01T00:00:00Z")

automation = load_automation_state(group)
assert automation["rules"]["rust-rule"]["last_fired_at"]
automation["rules"]["rust-rule"]["last_fired_at"] = "2999-01-01T00:00:00Z"
atomic_write_json(group.path / "state" / "automation.json", automation)

state_path, state = _load_state_doc()
assert state["session_enabled"][group_id]["peer"][0]["capability_id"] == "skill:test:shared"
assert state["group_blocked"][group_id]["skill:test:shared"]["reason"] == "interop"
_set_enabled_capability(
    state,
    group_id=group_id,
    actor_id="peer",
    scope="actor",
    capability_id="skill:test:shared",
    enabled=True,
    ttl_seconds=3600,
)
_save_state_doc(state_path, state)
snapshot = _allowlist_effective_snapshot()
assert snapshot["revision"] == rust_allowlist_revision
assert snapshot["overlay"]["defaults"]["source_level"]["manual_import"] == "indexed"
updated = handle_capability_allowlist_update({
    "by":"user",
    "mode":"patch",
    "patch":{"defaults":{"source_level":{"manual_import":"mounted"}}},
    "expected_revision":rust_allowlist_revision,
})
assert updated.ok, updated.error

assert get_space_binding(group_id, provider="notebooklm", lane="work")["remote_space_id"] == "nb-rust"
assert "NOTEBOOKLM_AUTH_JSON" in load_space_provider_secrets("notebooklm")
upsert_space_binding(
    group_id,
    provider="notebooklm",
    lane="memory",
    remote_space_id="nb-python",
    by="user",
)
enqueue_space_job(
    group_id=group_id,
    provider="notebooklm",
    lane="work",
    remote_space_id="nb-rust",
    kind="context_sync",
    payload={"title": "from-python"},
    idempotency_key="python-job",
)
"#,
        )
        .arg(group_id)
        .arg(&rust_allowlist_revision)
        .output()
        .expect("run Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let loaded_settings = settings::load(&home).expect("Rust reads Python settings");
    assert_eq!(loaded_settings.branding["subtitle"], "from-python");
    let profiles = ProfileStore::new(home.clone()).expect("profiles");
    let profile = profiles
        .get_ref("shared", "global", "")
        .expect("get profile")
        .expect("shared profile");
    assert_eq!(profile["name"], "Python profile");
    assert_eq!(profile["env"]["PUBLIC_VALUE"], "from-python");
    assert_eq!(
        profiles
            .secret_values_ref("shared", "global", "")
            .expect("profile secrets")["PYTHON_SECRET"],
        "from-python"
    );
    assert_eq!(
        inbox::cursor(&home, group_id, "peer").expect("Rust cursor"),
        Some("python-cursor".into())
    );
    assert!(
        cccc_core::automation::tick_group(&home, group_id, false)
            .expect("Rust reads Python automation")
            .notifications
            .is_empty()
    );
    let private = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"peer","by":"user"}),
    );
    assert_eq!(
        private["keys"],
        json!(["ACTOR_SECRET", "PYTHON_ACTOR_SECRET"])
    );
    let capabilities = call(
        &home,
        "capability_state",
        json!({"group_id":group_id,"actor_id":"peer"}),
    );
    assert_eq!(
        capabilities["enabled_capabilities"],
        json!([]),
        "Python actor enable remains masked by the Rust-created group block"
    );
    let allowlist = call(&home, "capability_allowlist_get", json!({}));
    assert_eq!(
        allowlist["overlay"]["defaults"]["source_level"]["manual_import"],
        "mounted"
    );
    let space = call(
        &home,
        "group_space_status",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    assert_eq!(space["bindings"]["memory"]["remote_space_id"], "nb-python");
    let jobs = call(
        &home,
        "group_space_jobs",
        json!({"group_id":group_id,"provider":"notebooklm","action":"list"}),
    );
    assert_eq!(jobs["jobs"][0]["idempotency_key"], "python-job");
    assert_eq!(jobs["jobs"][0]["payload"]["title"], "from-python");
}

#[test]
fn python_and_rust_accept_each_others_group_copy_packages() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("copy interop", "").expect("group");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group.group_id,
            "actor_id":"peer",
            "runtime":"codex",
            "env":{"PUBLIC_WILL_BE_SCRUBBED":"value"},
            "by":"user"
        }),
    );

    let rust_export = call(
        &home,
        "group_copy_export_file",
        json!({"group_id":group.group_id}),
    );
    let rust_package = rust_export["package_path"].as_str().expect("Rust package");
    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from cccc.daemon.ops.group_copy_ops import group_copy_export_file, group_copy_preview_import

group_id, rust_package = sys.argv[1], sys.argv[2]
preview = group_copy_preview_import({"package_path": rust_package})
assert preview.ok, preview.error
assert preview.result["preview"]["source_group_id"] == group_id
exported = group_copy_export_file({"group_id": group_id})
assert exported.ok, exported.error
print(json.dumps(exported.result))
"#,
        )
        .arg(&group.group_id)
        .arg(rust_package)
        .output()
        .expect("Python group copy");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_export: Value =
        serde_json::from_slice(&output.stdout).expect("Python export result");
    let rust_preview = call(
        &home,
        "group_copy_preview_import",
        json!({"package_path":python_export["package_path"]}),
    );
    assert_eq!(rust_preview["preview"]["source_group_id"], group.group_id);
    assert_eq!(rust_preview["preview"]["contains_secrets"], false);
}

fn call(home: &HomeLayout, op: &str, args: Value) -> Map<String, Value> {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        },
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    response.result
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("object")
}

fn python(repo: &Path, home: &Path) -> Command {
    let executable = std::env::var_os("CCCC_TEST_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo.join(if cfg!(windows) {
                ".venv/Scripts/python.exe"
            } else {
                ".venv/bin/python"
            })
        });
    let mut command = Command::new(executable);
    command
        .arg("-c")
        .env("CCCC_HOME", home)
        .env("PYTHONPATH", repo.join("src"))
        .current_dir(home);
    command
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
