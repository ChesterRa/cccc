use cccc_contracts::{Actor, DaemonRequest, DaemonResponse, Event};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::profiles::ProfileStore;
use cccc_core::settings::{self, GlobalSettings};
use cccc_core::{
    GroupStore, HomeLayout, active, im_state, inbox, ledger, space_credentials,
    web_model_connectors,
};
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn python_interop_share_the_deepseek_manual_restart_gate() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("DeepSeek gate interop", "").expect("group");
    let actor = Actor::new("deepseek");
    group.actors.push(actor.clone());
    groups.save(&group).expect("save actor");

    cccc_core::deepseek_restart_gate::record_running_generation(
        &home,
        &group.group_id,
        &actor.id,
        &actor.created_at,
        "rust-launch",
    )
    .expect("record Rust generation");
    assert!(
        cccc_core::deepseek_restart_gate::require_manual_restart(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            "rust-launch",
            "credential_unavailable",
        )
        .expect("close Rust gate")
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from pathlib import Path
from cccc.daemon.actors.deepseek_restart_gate import (
    manual_restart_required,
    record_running_generation,
    require_manual_restart,
)

group_path, group_id, actor_id, actor_created_at = sys.argv[1:5]
assert manual_restart_required(
    group_path=Path(group_path),
    group_id=group_id,
    actor_id=actor_id,
    actor_created_at=actor_created_at,
)
record_running_generation(
    group_path=Path(group_path),
    group_id=group_id,
    actor_id=actor_id,
    actor_created_at=actor_created_at,
    generation="python-launch",
)
assert require_manual_restart(
    group_path=Path(group_path),
    group_id=group_id,
    actor_id=actor_id,
    actor_created_at=actor_created_at,
    expected_generation="python-launch",
    reason_code="context_window_exceeded",
)
"#,
        )
        .arg(home.groups_dir().join(&group.group_id))
        .arg(&group.group_id)
        .arg(&actor.id)
        .arg(&actor.created_at)
        .output()
        .expect("run Python DeepSeek gate handoff");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        cccc_core::deepseek_restart_gate::manual_restart_required(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
        )
        .expect("Rust reads Python gate")
    );
    assert!(
        !cccc_core::deepseek_restart_gate::require_manual_restart(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            "rust-launch",
            "stale_failure",
        )
        .expect("reject stale Rust generation")
    );
}

#[test]
fn python_interop_voice_recording_lease_waits_for_the_shared_rust_lock() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("Voice lease interop", "").expect("group");
    groups
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "assistants".into(),
                json!({
                    "voice_secretary": {
                        "enabled": true,
                        "config": {"recognition_backend":"assistant_service_local_asr"}
                    }
                }),
            );
            Ok(())
        })
        .expect("enable Voice Secretary");

    let lock_path = home
        .root()
        .join("state/voice_secretary_recording_lease.json.lock");
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let lock_holder = thread::spawn(move || {
        cccc_core::fs::with_exclusive_lock(&lock_path, || {
            locked_tx.send(()).expect("signal held lock");
            release_rx.recv().expect("release held lock");
            Ok(())
        })
        .expect("hold Rust lease lock");
    });
    locked_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("Rust lease lock was not acquired");

    let mut child = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_recording_lease

group_id = sys.argv[1]
print("ready", flush=True)
sys.stdin.readline()
response = handle_assistant_voice_recording_lease({
    "group_id": group_id,
    "action": "acquire",
    "owner_id": "python-tab",
    "capture_mode": "prompt",
    "recognition_backend": "assistant_service_local_asr",
    "dispatch_target": "composer",
    "by": "user",
})
print(json.dumps(response.model_dump(exclude_none=True)), flush=True)
"#,
        )
        .arg(&group.group_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn Python lease writer");
    let mut child_stdin = child.stdin.take().expect("Python stdin");
    let mut child_stdout = BufReader::new(child.stdout.take().expect("Python stdout"));
    let mut ready = String::new();
    child_stdout
        .read_line(&mut ready)
        .expect("Python ready line");
    assert_eq!(ready.trim(), "ready");

    let (result_tx, result_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut line = String::new();
        child_stdout
            .read_line(&mut line)
            .expect("Python result line");
        result_tx.send(line).expect("send Python result");
    });
    child_stdin.write_all(b"go\n").expect("start Python write");
    let early_result = result_rx.recv_timeout(Duration::from_millis(150)).ok();
    let python_writer_was_blocked = early_result.is_none();
    release_tx.send(()).expect("release Rust lease lock");
    let result = early_result.unwrap_or_else(|| {
        result_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Python writer finishes after Rust releases the lock")
    });
    reader.join().expect("Python result reader");
    drop(child_stdin);
    let status = child.wait().expect("wait for Python");
    lock_holder.join().expect("Rust lock holder");

    assert!(status.success());
    assert!(
        python_writer_was_blocked,
        "Python mutation bypassed the Rust-held recording lease lock"
    );
    let result: Value = serde_json::from_str(result.trim()).expect("Python response JSON");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["result"]["lease"]["owner_id"], "python-tab");
}

#[test]
fn python_interop_share_the_voice_recording_lease_lifecycle() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("Voice lease handoff", "").expect("group");
    groups
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "assistants".into(),
                json!({
                    "voice_secretary": {
                        "enabled": true,
                        "config": {"recognition_backend":"assistant_service_local_asr"}
                    }
                }),
            );
            Ok(())
        })
        .expect("enable Voice Secretary");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_recording_lease

response = handle_assistant_voice_recording_lease({
    "group_id": sys.argv[1],
    "action": "acquire",
    "owner_id": "python-tab",
    "capture_mode": "prompt",
    "recognition_backend": "assistant_service_local_asr",
    "dispatch_target": "composer",
    "by": "user",
})
assert response.ok, response
print(json.dumps(response.result))
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python acquires shared lease");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_acquired: Value =
        serde_json::from_slice(&output.stdout).expect("Python lease result");
    let python_lease_id = python_acquired["lease_id"]
        .as_str()
        .expect("Python private lease ID");
    let python_lease = cccc_core::voice_recording_lease::validate(
        &home,
        &group.group_id,
        "python-tab",
        python_lease_id,
    )
    .expect("Rust validates Python lease");
    assert_eq!(python_lease["dispatch_target"], "composer");
    assert!(
        cccc_core::voice_recording_lease::renew(
            &home,
            &group.group_id,
            "Voice lease handoff",
            "python-tab",
            python_lease_id,
        )
        .expect("Rust renews Python lease")
    );
    assert!(
        cccc_core::voice_recording_lease::release(
            &home,
            &group.group_id,
            "python-tab",
            python_lease_id,
        )
        .expect("Rust releases Python lease")
    );

    let rust_acquired = cccc_core::voice_recording_lease::update(
        &home,
        &group.group_id,
        "Voice lease handoff",
        &json!({
            "action":"acquire",
            "owner_id":"rust-tab",
            "capture_mode":"document",
            "recognition_backend":"assistant_service_local_asr",
            "dispatch_target":"document",
            "by":"user"
        }),
    )
    .expect("Rust acquires shared lease");
    let rust_lease_id = rust_acquired["lease_id"]
        .as_str()
        .expect("Rust private lease ID");
    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_recording_lease

group_id, lease_id = sys.argv[1:3]
status = handle_assistant_voice_recording_lease({
    "group_id": group_id,
    "action": "status",
    "by": "user",
})
assert status.ok and status.result["lease"]["owner_id"] == "rust-tab", status
heartbeat = handle_assistant_voice_recording_lease({
    "group_id": group_id,
    "action": "heartbeat",
    "owner_id": "rust-tab",
    "lease_id": lease_id,
    "by": "user",
})
assert heartbeat.ok and heartbeat.result["acquired"], heartbeat
assert heartbeat.result["lease"]["capture_mode"] == "document", heartbeat
assert heartbeat.result["lease"]["recognition_backend"] == "assistant_service_local_asr", heartbeat
assert heartbeat.result["lease"]["dispatch_target"] == "document", heartbeat
released = handle_assistant_voice_recording_lease({
    "group_id": group_id,
    "action": "release",
    "owner_id": "rust-tab",
    "lease_id": lease_id,
    "by": "user",
})
assert released.ok and released.result["released"], released
print(json.dumps(released.result))
"#,
        )
        .arg(&group.group_id)
        .arg(rust_lease_id)
        .output()
        .expect("Python consumes Rust lease");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        cccc_core::voice_recording_lease::current(&home).expect("final lease state"),
        json!({})
    );
}

#[test]
fn python_interop_serialize_access_token_mutations_with_the_shared_lock() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = AccessTokenStore::new(home).expect("store");
    let mut child = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.kernel.access_tokens import (
    _access_tokens_lock_path,
    _save_access_tokens_unlocked,
    load_access_tokens,
)
from cccc.util.file_lock import acquire_lockfile, release_lockfile

lock = acquire_lockfile(_access_tokens_lock_path(), blocking=True)
try:
    tokens = load_access_tokens()
    tokens["acc_python_locked"] = {
        "user_id": "python-admin",
        "allowed_groups": [],
        "is_admin": True,
        "created_at": "2026-08-11T00:00:00Z",
        "updated_at": "2026-08-11T00:00:00Z",
    }
    print("locked", flush=True)
    sys.stdin.readline()
    _save_access_tokens_unlocked(tokens)
finally:
    release_lockfile(lock)
"#,
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn Python lock holder");
    let mut child_stdin = child.stdin.take().expect("Python stdin");
    let mut child_stdout = BufReader::new(child.stdout.take().expect("Python stdout"));
    let mut ready = String::new();
    child_stdout
        .read_line(&mut ready)
        .expect("Python ready line");
    assert_eq!(ready.trim(), "locked");

    let (sent, received) = mpsc::channel();
    let rust_store = store.clone();
    let writer = thread::spawn(move || {
        sent.send(rust_store.create("rust-admin", Vec::new(), true, Some("acc_rust_waiting")))
            .expect("send Rust result");
    });
    let early_result = received.recv_timeout(Duration::from_millis(100)).ok();
    let rust_writer_was_blocked = early_result.is_none();
    child_stdin.write_all(b"release\n").expect("release Python");
    drop(child_stdin);
    let status = child.wait().expect("wait for Python");
    let rust_result = early_result.unwrap_or_else(|| {
        received
            .recv_timeout(Duration::from_secs(3))
            .expect("Rust writer finishes after Python releases the lock")
    });
    writer.join().expect("Rust writer");

    assert!(status.success());
    assert!(
        rust_writer_was_blocked,
        "Rust mutation bypassed the Python-held access token lock"
    );
    rust_result.expect("Rust create");
    assert_eq!(store.list().expect("shared tokens").len(), 2);
    assert!(store.lookup("acc_python_locked").expect("lookup").is_some());
    assert!(store.lookup("acc_rust_waiting").expect("lookup").is_some());
}

#[test]
fn python_interop_share_access_token_creation_and_revocation() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = AccessTokenStore::new(home.clone()).expect("store");
    let rust_token = store
        .create("rust-admin", Vec::new(), true, Some("acc_rust_interop"))
        .expect("Rust token");

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.kernel.access_tokens import create_access_token, delete_access_token, lookup_access_token

rust = lookup_access_token("acc_rust_interop")
assert rust is not None and rust["user_id"] == "rust-admin", rust
created = create_access_token(
    "python-admin",
    is_admin=True,
    custom_token="acc_python_interop",
)
assert created["token"] == "acc_python_interop", created
assert delete_access_token("acc_rust_interop")
"#,
        )
        .output()
        .expect("Python token interop");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(store.lookup(&rust_token.token).expect("Rust lookup"), None);
    let python_token = store
        .lookup("acc_python_interop")
        .expect("Rust lookup")
        .expect("Python token");
    assert_eq!(python_token.user_id, "python-admin");
    assert_eq!(
        store
            .delete(&python_token.token_id())
            .expect("Rust revocation"),
        Some(python_token)
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.kernel.access_tokens import list_access_tokens, lookup_access_token

assert lookup_access_token("acc_python_interop") is None
assert list_access_tokens() == []
"#,
        )
        .output()
        .expect("Python confirms Rust revocation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_share_nomcp_sessions_messages_and_revocation() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let scope_root = temp.path().join("scope");
    std::fs::create_dir_all(&scope_root).expect("scope");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("No-MCP interop", "").expect("group");
    cccc_core::group_scope::attach(
        &groups,
        &group.group_id,
        cccc_core::Scope {
            scope_key: "scope_repo".into(),
            url: scope_root.to_string_lossy().into_owned(),
            label: "repo".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    let store = cccc_core::nomcp::Store::new(home.clone()).expect("No-MCP store");
    let rust_created = store
        .create(cccc_core::nomcp::CreateSpec {
            group_id: group.group_id.clone(),
            title: "Rust session".into(),
            brief: "interop".into(),
            reply_to_event_id: String::new(),
            recipient: "user".into(),
            scope_key: "scope_repo".into(),
            allowed_paths: Vec::new(),
            expires_in_seconds: 600,
        })
        .expect("Rust session");

    let output = python(&repo, home.root())
        .env("RUST_SID", &rust_created.session.sid)
        .env("RUST_SECRET", &rust_created.secret)
        .env("GROUP_ID", &group.group_id)
        .arg(
            r#"
import json
import os
from cccc.kernel.nomcp_sessions import (
    authorize_nomcp_session,
    create_nomcp_session,
    send_nomcp_advisory,
)

rust = authorize_nomcp_session(os.environ["RUST_SID"], os.environ["RUST_SECRET"])
assert rust["group_id"] == os.environ["GROUP_ID"], rust
sent = send_nomcp_advisory(
    os.environ["RUST_SID"],
    os.environ["RUST_SECRET"],
    msg_id="python-msg",
    text="Python advisory",
)
assert sent["status"] == "accepted", sent
created = create_nomcp_session(
    group_id=os.environ["GROUP_ID"],
    title="Python session",
    scope_key="scope_repo",
)
print(json.dumps({"sid": created["sid"], "secret": created["secret"]}))
"#,
        )
        .output()
        .expect("Python No-MCP interop");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_created: Value =
        serde_json::from_slice(&output.stdout).expect("Python session result");
    let python_sid = python_created["sid"].as_str().expect("Python sid");
    let python_secret = python_created["secret"].as_str().expect("Python secret");

    assert!(
        store
            .get(&rust_created.session.sid)
            .expect("Rust reads Python update")
            .expect("Rust session")
            .sent_message_ids
            .contains("python-msg")
    );
    store
        .authorize_advisory(python_sid, python_secret)
        .expect("Rust authorizes Python session")
        .record_message("rust-msg")
        .expect("Rust records advisory");
    assert!(
        store
            .revoke(python_sid)
            .expect("Rust revokes Python session")
    );

    let output = python(&repo, home.root())
        .env("PYTHON_SID", python_sid)
        .env("PYTHON_SECRET", python_secret)
        .arg(
            r#"
import json
import os
from cccc.kernel.nomcp_sessions import NomcpSessionError, authorize_nomcp_session
from cccc.paths import ensure_home

sid = os.environ["PYTHON_SID"]
doc = json.loads((ensure_home() / "state" / "nomcp_sessions" / f"{sid}.json").read_text())
assert "rust-msg" in doc["sent_message_ids"], doc
try:
    authorize_nomcp_session(sid, os.environ["PYTHON_SECRET"])
except NomcpSessionError as error:
    assert error.code == "revoked", error
else:
    raise AssertionError("Rust revocation was not visible to Python")
"#,
        )
        .output()
        .expect("Python confirms Rust No-MCP mutation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_retire_web_model_connectors_at_actor_generation_boundaries() {
    let repo = workspace_root();

    let rust_to_python = tempfile::tempdir().expect("Rust to Python home");
    let rust_home = HomeLayout::from_path(rust_to_python.path()).expect("home");
    let rust_group = GroupStore::new(rust_home.clone())
        .expect("groups")
        .create("Rust to Python", "")
        .expect("group");
    call(
        &rust_home,
        "actor_add",
        json!({
            "group_id":rust_group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );
    web_model_connectors::replace_active(
        &rust_home,
        &json!({
            "connector_id":"wmc_rust_generation",
            "group_id":rust_group.group_id,
            "actor_id":"web1",
            "secret":"wmcs_rust_generation",
            "created_at":"2026-08-11T00:00:00Z",
            "updated_at":"2026-08-11T00:00:00Z",
            "revoked":false
        }),
    )
    .expect("Rust connector");
    let output = python(&repo, rust_to_python.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
from cccc.kernel.web_model_connectors import verify_web_model_connector_secret
import sys

group_id = sys.argv[1]
def call(op, args):
    response, _ = handle_request(DaemonRequest.model_validate({"op": op, "args": args}))
    assert response.ok, (op, response)
    return response

assert verify_web_model_connector_secret("wmc_rust_generation", "wmcs_rust_generation")
call("actor_remove", {"group_id": group_id, "actor_id": "web1", "by": "user"})
assert verify_web_model_connector_secret("wmc_rust_generation", "wmcs_rust_generation") is None
call("actor_add", {"group_id": group_id, "actor_id": "web1", "runtime": "web_model", "by": "user"})
assert verify_web_model_connector_secret("wmc_rust_generation", "wmcs_rust_generation") is None
"#,
        )
        .arg(&rust_group.group_id)
        .output()
        .expect("Python retires Rust connector");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let python_to_rust = tempfile::tempdir().expect("Python to Rust home");
    let python_home = HomeLayout::from_path(python_to_rust.path()).expect("home");
    let python_group = GroupStore::new(python_home.clone())
        .expect("groups")
        .create("Python to Rust", "")
        .expect("group");
    let output = python(&repo, python_to_rust.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
from cccc.kernel.web_model_connectors import create_web_model_connector
import json
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "actor_add",
    "args": {"group_id": group_id, "actor_id": "web1", "runtime": "web_model", "by": "user"},
}))
assert response.ok, response
connector = create_web_model_connector(group_id=group_id, actor_id="web1", provider="chatgpt")
print(json.dumps({"id": connector["connector_id"], "secret": connector["secret"]}))
"#,
        )
        .arg(&python_group.group_id)
        .output()
        .expect("Python connector fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created: Value = serde_json::from_slice(&output.stdout).expect("connector identity");
    call(
        &python_home,
        "actor_remove",
        json!({
            "group_id":python_group.group_id,
            "actor_id":"web1",
            "by":"user"
        }),
    );
    let connectors = web_model_connectors::load(&python_home).expect("shared connectors");
    let connector = connectors
        .iter()
        .find(|item| item["connector_id"] == created["id"])
        .expect("Python connector");
    assert!(connector["revoked"].as_bool().unwrap_or(false));
    call(
        &python_home,
        "actor_add",
        json!({
            "group_id":python_group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );
    assert!(
        web_model_connectors::load(&python_home)
            .expect("recreated state")
            .iter()
            .find(|item| item["connector_id"] == created["id"])
            .expect("old connector")["revoked"]
            .as_bool()
            .unwrap_or(false)
    );
}

#[test]
fn python_interop_retire_web_model_connectors_at_group_deletion_boundaries() {
    let repo = workspace_root();

    let rust_to_python = tempfile::tempdir().expect("Rust to Python home");
    let rust_home = HomeLayout::from_path(rust_to_python.path()).expect("home");
    let rust_group = GroupStore::new(rust_home.clone())
        .expect("groups")
        .create("Rust connector deleted by Python", "")
        .expect("group");
    call(
        &rust_home,
        "actor_add",
        json!({
            "group_id":rust_group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );
    web_model_connectors::replace_active(
        &rust_home,
        &json!({
            "connector_id":"wmc_rust_group",
            "group_id":rust_group.group_id,
            "actor_id":"web1",
            "secret":"wmcs_rust_group",
            "created_at":"2026-08-11T00:00:00Z",
            "updated_at":"2026-08-11T00:00:00Z",
            "revoked":false
        }),
    )
    .expect("Rust connector");
    let output = python(&repo, rust_to_python.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
from cccc.kernel.web_model_connectors import verify_web_model_connector_secret
import sys

group_id = sys.argv[1]
assert verify_web_model_connector_secret("wmc_rust_group", "wmcs_rust_group")
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "group_delete",
    "args": {"group_id": group_id, "by": "user"},
}))
assert response.ok, response
assert verify_web_model_connector_secret("wmc_rust_group", "wmcs_rust_group") is None
"#,
        )
        .arg(&rust_group.group_id)
        .output()
        .expect("Python deletes Rust connector group");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        web_model_connectors::load(&rust_home)
            .expect("shared connectors")
            .iter()
            .find(|item| item["connector_id"] == "wmc_rust_group")
            .expect("Rust connector")["revoked"]
            .as_bool()
            .unwrap_or(false)
    );

    let python_to_rust = tempfile::tempdir().expect("Python to Rust home");
    let python_home = HomeLayout::from_path(python_to_rust.path()).expect("home");
    let python_group = GroupStore::new(python_home.clone())
        .expect("groups")
        .create("Python connector deleted by Rust", "")
        .expect("group");
    let output = python(&repo, python_to_rust.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
from cccc.kernel.web_model_connectors import create_web_model_connector
import json
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "actor_add",
    "args": {"group_id": group_id, "actor_id": "web1", "runtime": "web_model", "by": "user"},
}))
assert response.ok, response
connector = create_web_model_connector(group_id=group_id, actor_id="web1", provider="chatgpt")
print(json.dumps({"id": connector["connector_id"], "secret": connector["secret"]}))
"#,
        )
        .arg(&python_group.group_id)
        .output()
        .expect("Python connector fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created: Value = serde_json::from_slice(&output.stdout).expect("connector identity");
    call(
        &python_home,
        "group_delete",
        json!({"group_id":python_group.group_id,"by":"user"}),
    );
    assert!(
        web_model_connectors::load(&python_home)
            .expect("shared connectors")
            .iter()
            .find(|item| item["connector_id"] == created["id"])
            .expect("Python connector")["revoked"]
            .as_bool()
            .unwrap_or(false)
    );
    let output = python(&repo, python_to_rust.path())
        .arg(
            r#"
from cccc.kernel.web_model_connectors import verify_web_model_connector_secret
import sys

assert verify_web_model_connector_secret(sys.argv[1], sys.argv[2]) is None
"#,
        )
        .arg(created["id"].as_str().expect("connector id"))
        .arg(created["secret"].as_str().expect("connector secret"))
        .output()
        .expect("Python observes Rust retirement");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_share_web_model_preference_without_crossing_actor_generation() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("Web Model preference handoff", "")
        .expect("group");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );
    call(
        &home,
        "web_model_delivery_preferences_update",
        json!({
            "group_id":group.group_id,
            "actor_id":"web1",
            "mode":"image_compat",
            "by":"user"
        }),
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
import sys

group_id = sys.argv[1]
def call(op, args):
    response, _ = handle_request(DaemonRequest.model_validate({"op": op, "args": args}))
    assert response.ok, (op, response)
    return response.result

preference = call("web_model_delivery_preferences_get", {
    "group_id": group_id,
    "actor_id": "web1",
})["preference"]
assert preference["mode"] == "image_compat", preference
call("actor_remove", {"group_id": group_id, "actor_id": "web1", "by": "user"})
call("actor_add", {
    "group_id": group_id,
    "actor_id": "web1",
    "runtime": "web_model",
    "enabled": False,
    "by": "user",
})
preference = call("web_model_delivery_preferences_get", {
    "group_id": group_id,
    "actor_id": "web1",
})["preference"]
assert preference == {"mode": "standard", "updated_at": "", "updated_by": ""}, preference
call("web_model_delivery_preferences_update", {
    "group_id": group_id,
    "actor_id": "web1",
    "mode": "image_compat",
    "by": "user",
})
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python preference handoff");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let preference = call(
        &home,
        "web_model_delivery_preferences_get",
        json!({"group_id":group.group_id,"actor_id":"web1"}),
    );
    assert_eq!(preference["preference"]["mode"], "image_compat");
}

#[test]
fn python_interop_share_web_model_browser_target_state() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store.create("Web Model target handoff", "").expect("group");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state
import sys

record_chatgpt_browser_state(sys.argv[1], "web1", {
    "conversation_url": "https://chatgpt.com/c/python-target",
    "target_saved_at": "2026-08-11T01:00:00Z",
    "new_chat_bound_at": "2026-08-11T01:00:00Z",
    "last_delivery_at": "2026-08-11T01:01:00Z",
    "last_delivery_id": "python-delivery",
    "last_delivery_status": "submitted",
    "last_submission_evidence": "python-evidence",
    "last_send_selector": "python-selector",
    "last_turn_id": "python-turn",
    "last_event_ids": ["python-event"],
    "bootstrap_seed_delivered_at": "2026-08-11T01:00:30Z",
    "bootstrap_seed_version": "python-seed-v1",
    "bootstrap_seed_digest": "python-digest",
    "bootstrap_seed_conversation_url": "https://chatgpt.com/c/python-target",
})
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python target write");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let target = &store
        .load(&group.group_id)
        .expect("Rust reads Python target")
        .extra["web_model_browser_targets"]["web1"];
    assert_eq!(target["url"], "https://chatgpt.com/c/python-target");
    assert_eq!(target["last_delivery_id"], "python-delivery");
    assert_eq!(target["last_delivery_turn_id"], "python-turn");
    assert_eq!(target["last_delivery_event_ids"], json!(["python-event"]));
    assert_eq!(target["bootstrap_seed_digest"], "python-digest");

    cccc_core::integration_state::group_update(
        &store,
        &group.group_id,
        "web_model_browser_targets",
        |value| {
            value.as_object_mut().expect("target map").insert(
                "web1".into(),
                json!({
                    "state":"bound_existing_chat",
                    "kind":"existing_chat",
                    "url":"https://chatgpt.com/c/rust-target",
                    "saved_at":"2026-08-11T02:00:00Z",
                    "bound_at":"2026-08-11T02:00:00Z",
                    "next_delivery":"existing_chat",
                    "last_delivery_at":"2026-08-11T02:01:00Z",
                    "last_delivery_id":"rust-delivery",
                    "last_delivery_turn_id":"rust-turn",
                    "last_delivery_event_ids":["rust-event"],
                    "last_delivery_status":"submission_ambiguous",
                    "last_submission_evidence":{
                        "submission_evidence":"rust-evidence",
                        "send_selector":"rust-selector"
                    },
                    "bootstrap_seed_delivered_at":"2026-08-11T02:00:30Z",
                    "bootstrap_seed_version":"rust-seed-v1",
                    "bootstrap_seed_digest":"rust-digest",
                    "bootstrap_seed_conversation_url":"https://chatgpt.com/c/rust-target"
                }),
            );
            Ok(())
        },
    )
    .expect("Rust target write");

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state
import sys

state = read_chatgpt_browser_state(sys.argv[1], "web1")
assert state["conversation_url"] == "https://chatgpt.com/c/rust-target", state
assert state["last_delivery_id"] == "rust-delivery", state
assert state["last_delivery_status"] == "ambiguous", state
assert state["last_submission_evidence"] == "rust-evidence", state
assert state["last_send_selector"] == "rust-selector", state
assert state["last_turn_id"] == "rust-turn", state
assert state["last_event_ids"] == ["rust-event"], state
assert state["bootstrap_seed_version"] == "rust-seed-v1", state
assert state["bootstrap_seed_digest"] == "rust-digest", state
assert state["bootstrap_seed_conversation_url"] == "https://chatgpt.com/c/rust-target", state
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python target read");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_share_voice_secretary_semantic_input_and_cursor() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store
        .create("Voice Secretary input handoff", "")
        .expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "assistants".into(),
                json!({"voice_secretary":{"enabled":true,"config":{}}}),
            );
            Ok(())
        })
        .expect("enable Voice Secretary");

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_voice_input_append",
    "args": {
        "group_id": group_id,
        "kind": "prompt_refine",
        "request_id": "python-request",
        "input_append_id": "python-input",
        "voice_transcript": "Python input",
        "by": "user",
    },
}))
assert response.ok, response
assert response.result["input_event_created"] is True, response
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python appends shared input");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let read = call(
        &home,
        "assistant_voice_document_input_read",
        json!({"group_id":group.group_id,"by":"voice-secretary"}),
    );
    assert_eq!(read["item_count"], 1);
    assert!(
        read["input_text"]
            .as_str()
            .is_some_and(|text| text.contains("Python input")),
        "{read:?}"
    );

    let appended = call(
        &home,
        "assistant_voice_input_append",
        json!({
            "group_id":group.group_id,
            "kind":"prompt_refine",
            "request_id":"rust-request",
            "input_append_id":"rust-input",
            "voice_transcript":"Rust input",
            "by":"user"
        }),
    );
    assert_eq!(appended["input_event_created"], true);

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_voice_document_input_read",
    "args": {"group_id": group_id, "by": "voice-secretary"},
}))
assert response.ok, response
assert response.result["item_count"] == 1, response
assert "Rust input" in response.result["input_text"], response
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python reads Rust input");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_share_voice_secretary_document_transcript_projection() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("docs/voice-secretary")).expect("document dir");
    std::fs::write(workspace.join("docs/voice-secretary/shared.md"), "").expect("document");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store
        .create("Voice Secretary transcript handoff", "")
        .expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "assistants".into(),
                json!({"voice_secretary":{"enabled":true,"config":{}}}),
            );
            group.scopes.push(cccc_core::Scope {
                scope_key: "workspace".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "workspace".into();
            Ok(())
        })
        .expect("enable Voice Secretary");

    let rust_append = call(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group.group_id,
            "session_id":"rust-session",
            "segment_id":"rust-segment",
            "document_path":"docs/voice-secretary/shared.md",
            "text":"Rust transcript",
            "is_final":true,
            "flush":false,
            "by":"user"
        }),
    );
    assert_eq!(rust_append["segment"]["text"], "Rust transcript");

    let output = python(&repo, home.root())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
import sys

group_id = sys.argv[1]
view, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_state",
    "args": {
        "group_id": group_id,
        "assistant_id": "voice_secretary",
        "view": "voice_session",
        "document_path": "docs/voice-secretary/shared.md",
        "suppress_retry_notify": True,
    },
}))
assert view.ok, view
assert [item["text"] for item in view.result["session"]["segments"]] == ["Rust transcript"], view

append, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_voice_transcript_append",
    "args": {
        "group_id": group_id,
        "session_id": "python-session",
        "segment_id": "python-segment",
        "document_path": "docs/voice-secretary/shared.md",
        "text": "Python transcript",
        "is_final": True,
        "flush": False,
        "by": "user",
    },
}))
assert append.ok, append
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python transcript handoff");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rust_view = call(
        &home,
        "assistant_state",
        json!({
            "group_id":group.group_id,
            "assistant_id":"voice_secretary",
            "view":"voice_session",
            "document_path":"docs/voice-secretary/shared.md",
            "suppress_retry_notify":true
        }),
    );
    assert_eq!(rust_view["session"]["source"], "document_transcript");
    assert_eq!(
        rust_view["session"]["segments"]
            .as_array()
            .expect("segments")
            .iter()
            .map(|item| item["text"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["Rust transcript", "Python transcript"]
    );
}

#[test]
fn python_interop_migrate_split_voice_input_without_skipping_unread_work() {
    let repo = workspace_root();

    let rust_temp = tempfile::tempdir().expect("Rust migration home");
    let rust_home = HomeLayout::from_path(rust_temp.path()).expect("home");
    let rust_group = GroupStore::new(rust_home.clone())
        .expect("groups")
        .create("Rust migrates split input", "")
        .expect("group");
    let rust_voice_root = rust_home
        .root()
        .join("voice-secretary")
        .join(&rust_group.group_id);
    std::fs::create_dir_all(&rust_voice_root).expect("voice root");
    std::fs::write(
        rust_voice_root.join("input_events.jsonl"),
        serde_json::to_vec(&json!({
            "schema":1,"seq":1,"kind":"prompt_refine","text":"already read Python input",
            "session_id":"voice-secretary-prompt-refine","segment_id":"python-segment",
            "created_at":"2026-08-11T01:00:00Z"
        }))
        .expect("canonical event")
        .into_iter()
        .chain([b'\n'])
        .collect::<Vec<_>>(),
    )
    .expect("canonical input");
    std::fs::write(
        rust_voice_root.join("input_state.json"),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"group_id":rust_group.group_id,"latest_seq":1,
            "secretary_read_cursor":1,"secretary_delivery_cursor":1
        }))
        .expect("canonical state"),
    )
    .expect("canonical state");
    std::fs::write(
        rust_voice_root.join("inputs.jsonl"),
        serde_json::to_vec(&json!({
            "schema":1,"seq":1,"kind":"prompt_refine","text":"unread Rust input",
            "session_id":"voice-secretary-prompt-refine","segment_id":"rust-segment",
            "created_at":"2026-08-11T02:00:00Z"
        }))
        .expect("legacy event")
        .into_iter()
        .chain([b'\n'])
        .collect::<Vec<_>>(),
    )
    .expect("legacy input");
    cccc_core::assistant_state::update(&rust_home, &rust_group.group_id, |state| {
        state.insert("input_latest_seq".into(), json!(1));
        state.insert("input_read_cursor".into(), json!(0));
        Ok(())
    })
    .expect("legacy cursor");

    let read = call(
        &rust_home,
        "assistant_voice_document_input_read",
        json!({"group_id":rust_group.group_id,"by":"voice-secretary"}),
    );
    assert_eq!(read["item_count"], 1, "{read:?}");
    assert!(read["input_text"] == "unread Rust input", "{read:?}");
    assert!(!rust_voice_root.join("inputs.jsonl").exists());
    let raw: Value = serde_json::from_slice(
        &std::fs::read(
            rust_home
                .groups_dir()
                .join(&rust_group.group_id)
                .join("state/assistants.json"),
        )
        .expect("assistant state"),
    )
    .expect("assistant state JSON");
    assert!(raw["rust_state"].get("input_latest_seq").is_none());
    assert!(raw["rust_state"].get("input_read_cursor").is_none());

    let python_temp = tempfile::tempdir().expect("Python migration home");
    let python_home = HomeLayout::from_path(python_temp.path()).expect("home");
    let python_group = GroupStore::new(python_home.clone())
        .expect("groups")
        .create("Python migrates Rust input", "")
        .expect("group");
    let python_voice_root = python_home
        .root()
        .join("voice-secretary")
        .join(&python_group.group_id);
    std::fs::create_dir_all(&python_voice_root).expect("voice root");
    std::fs::write(
        python_voice_root.join("inputs.jsonl"),
        serde_json::to_vec(&json!({
            "schema":1,"seq":1,"kind":"prompt_refine","text":"legacy input for Python",
            "session_id":"voice-secretary-prompt-refine","segment_id":"legacy-segment",
            "created_at":"2026-08-11T03:00:00Z"
        }))
        .expect("legacy event")
        .into_iter()
        .chain([b'\n'])
        .collect::<Vec<_>>(),
    )
    .expect("legacy input");
    cccc_core::assistant_state::update(&python_home, &python_group.group_id, |state| {
        state.insert("input_latest_seq".into(), json!(1));
        state.insert("input_read_cursor".into(), json!(0));
        Ok(())
    })
    .expect("legacy cursor");
    let output = python(&repo, python_temp.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
from cccc.paths import ensure_home
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_voice_document_input_read",
    "args": {"group_id": group_id, "by": "voice-secretary"},
}))
assert response.ok, response
assert response.result["item_count"] == 1, response
assert "legacy input for Python" in response.result["input_text"], response
assert not (ensure_home() / "voice-secretary" / group_id / "inputs.jsonl").exists()
"#,
        )
        .arg(&python_group.group_id)
        .output()
        .expect("Python migrates legacy Rust input");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_share_voice_secretary_documents_and_active_selection() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store
        .create("Voice Secretary document handoff", "")
        .expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.scopes.push(cccc_core::Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("scope");

    let output = python(&repo, home.root())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
import json
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_voice_document_save",
    "args": {
        "group_id": group_id,
        "create_new": True,
        "title": "Python document",
        "content": "Python content",
        "by": "user",
    },
}))
assert response.ok, response
print(json.dumps({"path": response.result["document"]["document_path"]}))
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python creates document");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_document: Value =
        serde_json::from_slice(&output.stdout).expect("Python document path");
    let python_path = python_document["path"].as_str().expect("path");

    let listed = call(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group.group_id}),
    );
    assert!(
        listed["documents"]
            .as_array()
            .is_some_and(|documents| documents
                .iter()
                .any(|item| item["document_path"] == python_path)),
        "{listed:?}"
    );
    assert_eq!(listed["active_document_path"], python_path);

    call(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group.group_id,
            "document_path":"docs/voice-secretary/rust.md",
            "title":"Rust document",
            "content":"Rust content",
            "by":"user"
        }),
    );
    let output = python(&repo, home.root())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_state",
    "args": {"group_id": group_id, "assistant_id": "voice_secretary", "suppress_retry_notify": True},
}))
assert response.ok, response
paths = {item["document_path"] for item in response.result["documents"]}
assert "docs/voice-secretary/rust.md" in paths, response
assert len(paths) == 2, response
assert response.result["active_document_path"] == "docs/voice-secretary/rust.md", response
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python reads Rust document selection");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_discover_workspace_voice_documents() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let workspace = temp.path().join("workspace");
    let document_path = "docs/voice-secretary/direct.md";
    std::fs::create_dir_all(workspace.join("docs/voice-secretary")).expect("document dir");
    std::fs::write(
        workspace.join(document_path),
        "# Direct actor document\n\nCreated without a daemon save call.\n",
    )
    .expect("workspace document");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store
        .create("Voice Secretary workspace discovery", "")
        .expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.scopes.push(cccc_core::Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("scope");

    let output = python(&repo, home.root())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_voice_document_list",
    "args": {"group_id": group_id},
}))
assert response.ok, response
assert any(item["document_path"] == "docs/voice-secretary/direct.md" for item in response.result["documents"]), response
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python discovers workspace document");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let listed = call(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group.group_id}),
    );
    assert!(
        listed["documents"].as_array().is_some_and(|documents| {
            documents
                .iter()
                .any(|item| item["document_path"] == document_path)
        }),
        "{listed:?}"
    );
}

#[test]
fn python_interop_retire_legacy_voice_document_state_after_migration() {
    let repo = workspace_root();

    let rust_temp = tempfile::tempdir().expect("Rust migration home");
    let rust_workspace = rust_temp.path().join("workspace");
    std::fs::create_dir_all(rust_workspace.join("docs/voice-secretary")).expect("workspace");
    std::fs::write(
        rust_workspace.join("docs/voice-secretary/python.md"),
        "Python content",
    )
    .expect("Python document");
    std::fs::write(
        rust_workspace.join("docs/voice-secretary/rust.md"),
        "Rust content",
    )
    .expect("Rust document");
    let rust_home = HomeLayout::from_path(rust_temp.path().join("home")).expect("home");
    let rust_store = GroupStore::new(rust_home.clone()).expect("groups");
    let rust_group = rust_store
        .create("Rust document migration", "")
        .expect("group");
    rust_store
        .mutate(&rust_group.group_id, |group| {
            group.scopes.push(cccc_core::Scope {
                scope_key: "scope".into(),
                url: rust_workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("scope");
    let index_path = rust_home
        .root()
        .join("voice-secretary")
        .join(&rust_group.group_id)
        .join("documents/index.json");
    std::fs::create_dir_all(index_path.parent().expect("index parent")).expect("index parent");
    std::fs::write(
        &index_path,
        serde_json::to_vec_pretty(&json!({
            "schema":1,
            "group_id":rust_group.group_id,
            "active_document_id":"python-doc",
            "documents":{
                "python-doc":{
                    "document_id":"python-doc","workspace_path":"docs/voice-secretary/python.md",
                    "title":"Python","status":"active","storage_kind":"workspace",
                    "created_at":"2026-08-11T01:00:00Z","updated_at":"2026-08-11T01:00:00Z"
                }
            }
        }))
        .expect("index"),
    )
    .expect("index");
    cccc_core::assistant_state::update(&rust_home, &rust_group.group_id, |state| {
        state.insert(
            "documents".into(),
            json!([{
                "document_id":"rust-doc","document_path":"docs/voice-secretary/rust.md",
                "workspace_path":"docs/voice-secretary/rust.md","title":"Rust","status":"active",
                "storage_kind":"workspace","created_at":"2026-08-11T02:00:00Z",
                "updated_at":"2026-08-11T02:00:00Z"
            }]),
        );
        state.insert("active_document_id".into(), json!("rust-doc"));
        state.insert(
            "active_document_path".into(),
            json!("docs/voice-secretary/rust.md"),
        );
        Ok(())
    })
    .expect("legacy Rust documents");
    let listed = call(
        &rust_home,
        "assistant_voice_document_list",
        json!({"group_id":rust_group.group_id}),
    );
    assert_eq!(listed["documents"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        listed["active_document_path"],
        "docs/voice-secretary/python.md"
    );
    let raw: Value = serde_json::from_slice(
        &std::fs::read(
            rust_home
                .groups_dir()
                .join(&rust_group.group_id)
                .join("state/assistants.json"),
        )
        .expect("assistant state"),
    )
    .expect("assistant state JSON");
    assert!(raw["rust_state"].get("documents").is_none());
    assert!(raw["rust_state"].get("active_document_id").is_none());

    let python_temp = tempfile::tempdir().expect("Python migration home");
    let python_workspace = python_temp.path().join("workspace");
    std::fs::create_dir_all(python_workspace.join("docs/voice-secretary")).expect("workspace");
    std::fs::write(
        python_workspace.join("docs/voice-secretary/legacy.md"),
        "Legacy Rust content",
    )
    .expect("legacy document");
    let python_home = HomeLayout::from_path(python_temp.path().join("home")).expect("home");
    let python_store = GroupStore::new(python_home.clone()).expect("groups");
    let python_group = python_store
        .create("Python document migration", "")
        .expect("group");
    python_store
        .mutate(&python_group.group_id, |group| {
            group.scopes.push(cccc_core::Scope {
                scope_key: "scope".into(),
                url: python_workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("scope");
    cccc_core::assistant_state::update(&python_home, &python_group.group_id, |state| {
        state.insert(
            "documents".into(),
            json!([{
                "document_id":"legacy-doc","document_path":"docs/voice-secretary/legacy.md",
                "workspace_path":"docs/voice-secretary/legacy.md","title":"Legacy","status":"active",
                "storage_kind":"workspace","created_at":"2026-08-11T03:00:00Z",
                "updated_at":"2026-08-11T03:00:00Z"
            }]),
        );
        state.insert("active_document_id".into(), json!("legacy-doc"));
        state.insert(
            "active_document_path".into(),
            json!("docs/voice-secretary/legacy.md"),
        );
        Ok(())
    })
    .expect("legacy Rust documents");
    let output = python(&repo, python_home.root())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
from cccc.daemon.assistants.assistant_ops import _load_runtime_state
from cccc.kernel.group import load_group
import sys

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "assistant_state",
    "args": {"group_id": group_id, "assistant_id": "voice_secretary", "suppress_retry_notify": True},
}))
assert response.ok, response
assert response.result["active_document_path"] == "docs/voice-secretary/legacy.md", response
assert {item["document_path"] for item in response.result["documents"]} == {"docs/voice-secretary/legacy.md"}, response
group = load_group(group_id)
state = _load_runtime_state(group)
assert "documents" not in state["rust_state"], state
assert "active_document_id" not in state["rust_state"], state
"#,
        )
        .arg(&python_group.group_id)
        .output()
        .expect("Python migrates legacy Rust documents");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_retire_each_others_web_model_runtime_state_on_recreate() {
    let repo = workspace_root();

    let rust_to_python = tempfile::tempdir().expect("Rust to Python home");
    let rust_home = HomeLayout::from_path(rust_to_python.path()).expect("home");
    let rust_group = GroupStore::new(rust_home.clone())
        .expect("groups")
        .create("Rust runtime state deleted by Python", "")
        .expect("group");
    call(
        &rust_home,
        "actor_add",
        json!({
            "group_id":rust_group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );
    call(
        &rust_home,
        "group_set_state",
        json!({"group_id":rust_group.group_id,"state":"paused","by":"user"}),
    );
    call(
        &rust_home,
        "headless_set_status",
        json!({
            "group_id":rust_group.group_id,
            "actor_id":"web1",
            "status":"working",
            "task_id":"old-rust-task"
        }),
    );
    let rust_headless_path = rust_home
        .groups_dir()
        .join(&rust_group.group_id)
        .join("state/runners/headless/web1.json");
    std::fs::create_dir_all(rust_headless_path.parent().expect("headless state parent"))
        .expect("headless state parent");
    std::fs::write(
        &rust_headless_path,
        serde_json::to_vec_pretty(&json!({
            "v":1,
            "kind":"headless",
            "group_id":rust_group.group_id,
            "actor_id":"web1",
            "status":"working",
            "task_id":"old-python-task"
        }))
        .expect("headless state"),
    )
    .expect("headless state");
    let output = python(&repo, rust_to_python.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.runner_state_ops import headless_state_path
from cccc.daemon.server import handle_request
from cccc.kernel.group import load_group
import sys

group_id = sys.argv[1]
def call(op, args):
    response, _ = handle_request(DaemonRequest.model_validate({"op": op, "args": args}))
    assert response.ok, (op, response)

group = load_group(group_id)
assert group.doc["runtime_states"]["web1"]["task_id"] == "old-rust-task", group.doc
assert headless_state_path(group_id, "web1").exists()
call("actor_remove", {"group_id": group_id, "actor_id": "web1", "by": "user"})
call("actor_add", {
    "group_id": group_id,
    "actor_id": "web1",
    "runtime": "web_model",
    "by": "user",
})
group = load_group(group_id)
assert "web1" not in (group.doc.get("runtime_states") or {}), group.doc
assert not headless_state_path(group_id, "web1").exists()
"#,
        )
        .arg(&rust_group.group_id)
        .output()
        .expect("Python retires Rust runtime state");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rust_state = call(
        &rust_home,
        "headless_status",
        json!({"group_id":rust_group.group_id,"actor_id":"web1"}),
    );
    assert_ne!(rust_state["state"]["status"], "working");
    assert_eq!(rust_state["state"]["task_id"], Value::Null);

    let python_to_rust = tempfile::tempdir().expect("Python to Rust home");
    let python_home = HomeLayout::from_path(python_to_rust.path()).expect("home");
    let python_group = GroupStore::new(python_home.clone())
        .expect("groups")
        .create("Python runtime state deleted by Rust", "")
        .expect("group");
    call(
        &python_home,
        "actor_add",
        json!({
            "group_id":python_group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );
    call(
        &python_home,
        "group_set_state",
        json!({"group_id":python_group.group_id,"state":"paused","by":"user"}),
    );
    let python_headless_path = python_home
        .groups_dir()
        .join(&python_group.group_id)
        .join("state/runners/headless/web1.json");
    std::fs::create_dir_all(
        python_headless_path
            .parent()
            .expect("headless state parent"),
    )
    .expect("headless state parent");
    std::fs::write(
        &python_headless_path,
        serde_json::to_vec_pretty(&json!({
            "v":1,
            "kind":"headless",
            "group_id":python_group.group_id,
            "actor_id":"web1",
            "status":"working",
            "task_id":"old-python-task"
        }))
        .expect("headless state"),
    )
    .expect("headless state");
    GroupStore::new(python_home.clone())
        .expect("groups")
        .mutate(&python_group.group_id, |group| {
            group.extra.insert(
                "runtime_states".into(),
                json!({"web1":{"status":"working","task_id":"old-rust-task"}}),
            );
            Ok(())
        })
        .expect("Rust state fixture");
    call(
        &python_home,
        "actor_remove",
        json!({"group_id":python_group.group_id,"actor_id":"web1","by":"user"}),
    );
    call(
        &python_home,
        "actor_add",
        json!({
            "group_id":python_group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );
    let output = python(&repo, python_to_rust.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.runner_state_ops import headless_state_path
from cccc.daemon.server import handle_request
from cccc.kernel.group import load_group
import sys

group_id = sys.argv[1]
group = load_group(group_id)
assert "web1" not in (group.doc.get("runtime_states") or {}), group.doc
assert not headless_state_path(group_id, "web1").exists()
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "actor_list",
    "args": {"group_id": group_id, "by": "user"},
}))
assert response.ok, response
actor = next(item for item in response.result["actors"] if item["id"] == "web1")
assert actor["running"] is False, actor
assert actor["effective_working_state"] == "stopped", actor
"#,
        )
        .arg(&python_group.group_id)
        .output()
        .expect("Python observes Rust runtime retirement");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_rust_actor_add_blocks_legacy_web_model_target_resurrection() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("legacy browser shadow", "")
        .expect("group");
    assert!(
        !group.extra.contains_key("web_model_browser_targets"),
        "fixture must predate the canonical target store"
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.ports.web_model_browser_sidecar import _write_state, chatgpt_browser_actor_state_root
import sys

group_id = sys.argv[1]
_write_state(chatgpt_browser_actor_state_root(group_id, "web1"), {
    "conversation_url": "https://chatgpt.com/c/legacy-shadow-must-not-return",
    "last_delivery_id": "legacy-delivery",
    "last_delivery_status": "submitted",
})
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("legacy Python shadow");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    call(
        &home,
        "actor_add",
        json!({
            "group_id":group.group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "by":"user"
        }),
    );
    assert_eq!(
        GroupStore::new(home.clone())
            .expect("groups")
            .load(&group.group_id)
            .expect("group")
            .extra["web_model_browser_targets"],
        json!({})
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.kernel.group import load_group
from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state
import sys

group_id = sys.argv[1]
state = read_chatgpt_browser_state(group_id, "web1")
assert state.get("conversation_url") == "", state
assert state.get("last_delivery_id") == "", state
group = load_group(group_id)
assert group is not None
assert group.doc.get("web_model_browser_targets") == {}, group.doc
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python reads canonical empty target");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_credential_clear_retires_the_legacy_rust_notebooklm_secret() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    std::fs::write(
        temp.path().join("space-credentials.json"),
        serde_json::to_vec_pretty(&json!({
            "providers":{
                "notebooklm":{"auth_json":"{\"cookies\":[{\"name\":\"SID\",\"value\":\"legacy\"}]}"}
            }
        }))
        .expect("legacy credential JSON"),
    )
    .expect("legacy credential fixture");

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.daemon.space.group_space_store import update_space_provider_secrets

assert update_space_provider_secrets(
    "notebooklm",
    set_vars={},
    unset_keys=[],
    clear=True,
) == {}
"#,
        )
        .output()
        .expect("Python clears NotebookLM credential");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let status = space_credentials::status(&home, "notebooklm")
        .expect("Rust reads cleared NotebookLM credential");
    assert_eq!(
        status["configured"], false,
        "a retired legacy credential must not be imported after Python clears the canonical store"
    );
    let legacy: Value = serde_json::from_slice(
        &std::fs::read(temp.path().join("space-credentials.json"))
            .unwrap_or_else(|_| b"{}".to_vec()),
    )
    .expect("retired legacy credential JSON");
    assert!(
        legacy
            .get("providers")
            .and_then(|providers| providers.get("notebooklm"))
            .is_none(),
        "the durable legacy secret shadow must be consumed"
    );
}

#[test]
fn python_interop_unbind_is_not_reversed_by_legacy_rust_group_space_state() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups
        .create("legacy NotebookLM binding", "")
        .expect("group");
    groups
        .mutate(&group.group_id, |doc| {
            doc.extra.insert(
                "group_space".into(),
                json!({
                    "bindings":{
                        "work":{
                            "remote_space_id":"nb-legacy",
                            "bound_by":"user",
                            "bound_at":"2026-08-11T00:00:00Z",
                            "status":"bound"
                        }
                    },
                    "jobs":[
                        {
                            "job_id":"spj_legacy_unique",
                            "remote_space_id":"nb-legacy",
                            "kind":"context_sync",
                            "payload":{"title":"legacy unique"},
                            "state":"pending"
                        },
                        {
                            "job_id":"spj_collision",
                            "remote_space_id":"nb-legacy",
                            "kind":"context_sync",
                            "payload":{"title":"stale collision"},
                            "state":"pending"
                        }
                    ]
                }),
            );
            Ok(())
        })
        .expect("legacy group-space fixture");
    let jobs_path = temp.path().join("state/space/jobs.json");
    std::fs::create_dir_all(jobs_path.parent().expect("jobs parent")).expect("jobs parent");
    std::fs::write(
        &jobs_path,
        serde_json::to_vec_pretty(&json!({
            "v":2,
            "jobs":{
                "spj_collision":{
                    "job_id":"spj_collision",
                    "group_id":group.group_id,
                    "provider":"notebooklm",
                    "lane":"work",
                    "remote_space_id":"nb-current",
                    "kind":"context_sync",
                    "payload":{"title":"canonical collision"},
                    "state":"pending"
                }
            }
        }))
        .expect("canonical jobs JSON"),
    )
    .expect("canonical jobs fixture");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.daemon.space.group_space_store import get_space_binding, list_space_jobs, set_space_binding_unbound

group_id = sys.argv[1]
assert get_space_binding(
    group_id,
    provider="notebooklm",
    lane="work",
)["remote_space_id"] == "nb-legacy"
jobs = {item["job_id"]: item for item in list_space_jobs(group_id=group_id, provider="notebooklm")}
assert jobs["spj_legacy_unique"]["payload"]["title"] == "legacy unique", jobs
assert jobs["spj_collision"]["payload"]["title"] == "canonical collision", jobs
binding = set_space_binding_unbound(
    group_id,
    provider="notebooklm",
    lane="work",
    by="user",
)
assert binding["remote_space_id"] == "", binding
assert get_space_binding(group_id, provider="notebooklm", lane="work")["remote_space_id"] == ""
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python unbinds NotebookLM notebook");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let status = call(
        &home,
        "group_space_status",
        json!({"group_id":group.group_id,"provider":"notebooklm"}),
    );
    assert_eq!(
        status["bindings"]["work"]["remote_space_id"], "",
        "Rust must not restore a stale legacy binding over Python's canonical unbind"
    );
    assert!(
        groups
            .load(&group.group_id)
            .expect("group after migration")
            .extra
            .get("group_space")
            .is_none(),
        "the durable legacy group-space shadow must be consumed"
    );
    let jobs = call(
        &home,
        "group_space_jobs",
        json!({"group_id":group.group_id,"provider":"notebooklm","action":"list"}),
    );
    let jobs = jobs["jobs"].as_array().expect("Rust jobs");
    assert!(jobs.iter().any(|job| {
        job["job_id"] == "spj_legacy_unique" && job["payload"]["title"] == "legacy unique"
    }));
    assert!(jobs.iter().any(|job| {
        job["job_id"] == "spj_collision" && job["payload"]["title"] == "canonical collision"
    }));
}

#[test]
fn python_interop_rust_legacy_group_space_migration_is_canonical_first_and_visible() {
    let repo = workspace_root();

    let canonical_home = tempfile::tempdir().expect("canonical-first home");
    let home = HomeLayout::from_path(canonical_home.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("canonical binding wins", "").expect("group");
    groups
        .mutate(&group.group_id, |doc| {
            doc.extra.insert(
                "group_space".into(),
                json!({
                    "bindings":{"work":{"remote_space_id":"nb-stale","status":"bound"}},
                    "jobs":[]
                }),
            );
            Ok(())
        })
        .expect("legacy binding");
    let bindings_path = canonical_home.path().join("state/space/bindings.json");
    std::fs::create_dir_all(bindings_path.parent().expect("bindings parent"))
        .expect("bindings parent");
    std::fs::write(
        &bindings_path,
        serde_json::to_vec_pretty(&json!({
            "v":2,
            "bindings":{
                (group.group_id.clone()):{
                    "notebooklm":{
                        "work":{
                            "group_id":group.group_id,
                            "provider":"notebooklm",
                            "lane":"work",
                            "remote_space_id":"",
                            "bound_by":"user",
                            "status":"unbound"
                        }
                    }
                }
            }
        }))
        .expect("canonical binding JSON"),
    )
    .expect("canonical binding fixture");
    let status = call(
        &home,
        "group_space_status",
        json!({"group_id":group.group_id,"provider":"notebooklm"}),
    );
    assert_eq!(status["bindings"]["work"]["remote_space_id"], "");
    assert!(
        groups
            .load(&group.group_id)
            .expect("migrated group")
            .extra
            .get("group_space")
            .is_none()
    );

    let import_home = tempfile::tempdir().expect("Rust import home");
    let home = HomeLayout::from_path(import_home.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("Rust legacy import", "").expect("group");
    groups
        .mutate(&group.group_id, |doc| {
            doc.extra.insert(
                "group_space".into(),
                json!({
                    "bindings":{"work":{"remote_space_id":"nb-rust-import","status":"bound"}},
                    "jobs":[{
                        "job_id":"spj_rust_import",
                        "remote_space_id":"nb-rust-import",
                        "kind":"context_sync",
                        "payload":{"title":"Rust import"},
                        "state":"pending"
                    }]
                }),
            );
            Ok(())
        })
        .expect("legacy Rust state");
    let status = call(
        &home,
        "group_space_status",
        json!({"group_id":group.group_id,"provider":"notebooklm"}),
    );
    assert_eq!(
        status["bindings"]["work"]["remote_space_id"],
        "nb-rust-import"
    );
    let jobs = call(
        &home,
        "group_space_jobs",
        json!({"group_id":group.group_id,"provider":"notebooklm","action":"list"}),
    );
    assert_eq!(jobs["jobs"][0]["job_id"], "spj_rust_import");

    let output = python(&repo, import_home.path())
        .arg(
            r#"
import sys
from cccc.daemon.space.group_space_store import get_space_binding, list_space_jobs

group_id = sys.argv[1]
assert get_space_binding(group_id, provider="notebooklm", lane="work")["remote_space_id"] == "nb-rust-import"
jobs = list_space_jobs(group_id=group_id, provider="notebooklm")
assert len(jobs) == 1 and jobs[0]["job_id"] == "spj_rust_import", jobs
assert jobs[0]["payload"]["title"] == "Rust import", jobs
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python reads Rust-migrated group-space state");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_group_delete_retires_shared_notebooklm_state() {
    let repo = workspace_root();
    for delete_engine in ["rust", "python"] {
        let temp = tempfile::tempdir().expect("temp home");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("groups")
            .create(&format!("{delete_engine} NotebookLM delete"), "")
            .expect("group");
        let output = python(&repo, temp.path())
            .arg(
                r#"
import sys
from cccc.daemon.space.group_space_store import enqueue_space_job, upsert_space_binding

group_id = sys.argv[1]
upsert_space_binding(
    group_id,
    provider="notebooklm",
    lane="work",
    remote_space_id="nb-delete",
    by="user",
)
job, deduped = enqueue_space_job(
    group_id=group_id,
    provider="notebooklm",
    lane="work",
    remote_space_id="nb-delete",
    kind="context_sync",
    payload={"title":"must be retired"},
    idempotency_key=f"delete-{group_id}",
)
assert not deduped, job
"#,
            )
            .arg(&group.group_id)
            .output()
            .expect("seed shared NotebookLM state");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        if delete_engine == "python" {
            let output = python(&repo, temp.path())
                .arg(
                    r#"
import sys
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request

response, _ = handle_request(DaemonRequest.model_validate({
    "op":"group_delete",
    "args":{"group_id":sys.argv[1],"by":"user"},
}))
assert response.ok, response
"#,
                )
                .arg(&group.group_id)
                .output()
                .expect("Python deletes NotebookLM group");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            call(
                &home,
                "group_delete",
                json!({"group_id":group.group_id,"by":"user"}),
            );
        }

        let bindings: Value = serde_json::from_slice(
            &std::fs::read(temp.path().join("state/space/bindings.json"))
                .unwrap_or_else(|_| b"{}".to_vec()),
        )
        .expect("bindings JSON");
        assert!(
            bindings
                .get("bindings")
                .and_then(|items| items.get(&group.group_id))
                .is_none(),
            "{delete_engine} deletion left a live NotebookLM binding for a deleted group"
        );
        let jobs: Value = serde_json::from_slice(
            &std::fs::read(temp.path().join("state/space/jobs.json"))
                .unwrap_or_else(|_| b"{}".to_vec()),
        )
        .expect("jobs JSON");
        assert!(
            jobs.get("jobs")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .all(|(_, job)| job["group_id"] != group.group_id),
            "{delete_engine} deletion left executable NotebookLM jobs for a deleted group"
        );
        let payload_dir = temp.path().join("state/space/job_payloads");
        assert!(
            !payload_dir.exists()
                || payload_dir
                    .read_dir()
                    .expect("payload entries")
                    .next()
                    .is_none(),
            "{delete_engine} deletion left NotebookLM job payloads for a deleted group"
        );
    }
}

#[test]
fn python_interop_share_persisted_control_plane_state() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("interop", "").expect("group");
    let group_id = group.group_id.as_str();
    let python_active_group = groups.create("python active selection", "").expect("group");
    active::set(&home, group_id).expect("Rust active selection");

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
    event.data = object(json!({"to":["peer"],"text":"interop","message_mode":"mail"}));
    ledger::append(&groups.ledger_path(group_id).expect("ledger path"), &event)
        .expect("ledger event");
    let group = groups.load(group_id).expect("group for Rust Mail read");
    let consumed = inbox::consume_unread(&home, &group, "peer", "peer", 1).expect("Rust Mail read");
    assert_eq!(consumed.messages[0].id, event.id);
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
    assert!(
        cccc_core::automation::tick_group(&home, group_id, false)
            .expect("Rust automation clock start")
            .notifications
            .is_empty()
    );
    let automation_state_path = groups
        .state_dir(group_id)
        .expect("automation state dir")
        .join("automation.json");
    let mut automation_state: Value =
        cccc_core::fs::read_json(&automation_state_path).expect("automation state");
    automation_state["rules"]["rust-rule"]["last_fired_at"] = json!("2020-01-01T00:00:00Z");
    cccc_core::fs::write_json(&automation_state_path, &automation_state)
        .expect("due automation state");
    assert_eq!(
        cccc_core::automation::tick_group(&home, group_id, false)
            .expect("Rust due automation tick")
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
                },
                "skill:test:blocked":{
                    "capability_id":"skill:test:blocked",
                    "kind":"skill",
                    "name":"blocked",
                    "description_short":"interop block"
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
            "capability_id":"skill:test:blocked",
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
from cccc.kernel.active import load_active, set_active_group_id
from cccc.kernel.group import load_group
from cccc.kernel.inbox import get_cursor
from cccc.kernel.ledger import append_event
from cccc.daemon.messaging.inbox_read_ops import handle_inbox_read
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

group_id, rust_allowlist_revision, python_active_group_id = sys.argv[1:4]
assert load_active()["active_group_id"] == group_id
set_active_group_id(python_active_group_id)
settings = load_settings()
assert settings["web_branding"]["title"] == "Rust title"
settings["web_branding"]["subtitle"] = "from-python"
save_settings(settings)

profile = get_actor_profile("shared")
assert profile["env"] == {}
assert load_actor_profile_secrets("shared") == {
    "PRIVATE_VALUE": "from-rust",
    "PUBLIC_VALUE": "from-rust",
}
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
python_mail = append_event(
    group.ledger_path,
    kind="chat.message",
    group_id=group_id,
    scope_key="",
    by="user",
    data={"to": ["peer"], "text": "python cursor", "message_mode": "mail"},
)
consumed = handle_inbox_read({
    "group_id": group_id,
    "actor_id": "peer",
    "by": "peer",
    "limit": 1,
})
assert consumed.ok, consumed.error
assert consumed.result["messages"][0]["id"] == python_mail["id"]

automation = load_automation_state(group)
assert automation["rules"]["rust-rule"]["last_fired_at"]
automation["rules"]["rust-rule"]["last_fired_at"] = "2999-01-01T00:00:00Z"
atomic_write_json(group.path / "state" / "automation.json", automation)

state_path, state = _load_state_doc()
assert state["session_enabled"][group_id]["peer"][0]["capability_id"] == "skill:test:shared"
assert state["group_blocked"][group_id]["skill:test:blocked"]["reason"] == "interop"
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
        .arg(&python_active_group.group_id)
        .output()
        .expect("run Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        active::get(&home).expect("Rust reads Python active selection"),
        Some(python_active_group.group_id.clone())
    );

    let loaded_settings = settings::load(&home).expect("Rust reads Python settings");
    assert_eq!(loaded_settings.branding["subtitle"], "from-python");
    let profiles = ProfileStore::new(home.clone()).expect("profiles");
    let profile = profiles
        .get_ref("shared", "global", "")
        .expect("get profile")
        .expect("shared profile");
    assert_eq!(profile["name"], "Python profile");
    assert_eq!(profile["env"], json!({}));
    assert_eq!(
        profiles
            .secret_values_ref("shared", "global", "")
            .expect("profile secrets")["PUBLIC_VALUE"],
        "from-python"
    );
    assert_eq!(
        profiles
            .secret_values_ref("shared", "global", "")
            .expect("profile secrets")["PYTHON_SECRET"],
        "from-python"
    );
    let python_cursor = inbox::cursor(&home, group_id, "peer")
        .expect("Rust cursor")
        .expect("Python cursor event");
    let cursor_event = ledger::read_all(&groups.ledger_path(group_id).expect("ledger path"))
        .expect("ledger")
        .into_iter()
        .find(|event| event.id == python_cursor)
        .expect("Python cursor resolves to a ledger event");
    assert_eq!(cursor_event.data["message_mode"], "mail");
    assert_eq!(cursor_event.data["text"], "python cursor");
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
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "capability_id":"skill:test:blocked"
        }),
    );
    assert_eq!(
        capabilities["enabled_capabilities"],
        json!(["skill:cccc:self-evolution", "skill:test:shared"]),
        "Rust reads both its session enable and Python's actor enable as one capability"
    );
    assert_eq!(
        capabilities["capability_usage"]["blocked"], true,
        "Rust preserves its independent group block across the Python handoff"
    );
    assert_eq!(capabilities["capability_usage"]["blocked_scope"], "group");
    assert_eq!(
        capabilities["capability_usage"]["blocked_reason"], "interop",
        "the shared block metadata survives the Python handoff"
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
fn python_interop_rust_fired_one_time_automation_is_not_replayed() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("automation handoff", "").expect("group");
    groups
        .mutate(&group.group_id, |group| {
            cccc_core::actors::add(group, cccc_contracts::Actor::new("peer"))?;
            group.automation = json!({
                "version":1,
                "rules":[{
                    "id":"once-handoff","enabled":true,"scope":"group","to":["peer"],
                    "trigger":{"kind":"at","at":"2020-01-01T00:00:00Z"},
                    "action":{"kind":"notify","message":"fire once"}
                }],
                "snippets":{},"snippet_overrides":{}
            })
            .as_object()
            .cloned()
            .expect("automation");
            Ok(())
        })
        .expect("automation rule");
    assert_eq!(
        cccc_core::automation::tick_group(&home, &group.group_id, false)
            .expect("Rust automation tick")
            .notifications
            .len(),
        1
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from datetime import datetime, timezone
from unittest.mock import patch

from cccc.daemon.automation import AutomationManager
from cccc.kernel.group import load_group

group = load_group(sys.argv[1])
assert group is not None
manager = AutomationManager()
with patch("cccc.daemon.automation.engine.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
    "cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None
):
    manager._check_rules(group, datetime.now(timezone.utc))

events = [json.loads(line) for line in group.ledger_path.read_text(encoding="utf-8").splitlines() if line.strip()]
matching = [
    event for event in events
    if event.get("kind") == "system.notify"
    and isinstance(event.get("data"), dict)
    and (
        event["data"].get("rule_id") == "once-handoff"
        or (
            isinstance(event["data"].get("context"), dict)
            and event["data"]["context"].get("rule_id") == "once-handoff"
        )
    )
]
assert len(matching) == 1, matching
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python automation handoff");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_completed_one_time_automation_is_not_replayed_by_rust() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups
        .create("python automation handoff", "")
        .expect("group");
    groups
        .mutate(&group.group_id, |group| {
            cccc_core::actors::add(group, cccc_contracts::Actor::new("peer"))?;
            Ok(())
        })
        .expect("actor");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from datetime import datetime, timezone
from unittest.mock import patch

from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.automation import AutomationManager
from cccc.daemon.server import handle_request
from cccc.kernel.group import load_group

group_id = sys.argv[1]
updated, _ = handle_request(DaemonRequest(
    op="group_automation_update",
    args={
        "group_id": group_id,
        "by": "user",
        "ruleset": {
            "rules": [{
                "id": "python-once",
                "enabled": True,
                "scope": "group",
                "to": ["peer"],
                "trigger": {"kind": "at", "at": "2020-01-01T00:00:00Z"},
                "action": {"kind": "notify", "message": "python once"},
            }],
            "snippets": {},
        },
    },
))
assert updated.ok, updated.error
group = load_group(group_id)
assert group is not None
with patch("cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None):
    AutomationManager()._check_rules(group, datetime.now(timezone.utc))

group = load_group(group_id)
assert group is not None
assert group.doc["automation"]["rules"][0]["enabled"] is False
events = [json.loads(line) for line in group.ledger_path.read_text(encoding="utf-8").splitlines() if line.strip()]
matching = [
    event for event in events
    if event.get("kind") == "system.notify"
    and isinstance(event.get("data"), dict)
    and isinstance(event["data"].get("context"), dict)
    and event["data"]["context"].get("rule_id") == "python-once"
]
assert len(matching) == 1, matching
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python automation completion");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let reloaded = groups.load(&group.group_id).expect("Rust reload");
    assert_eq!(reloaded.automation["rules"][0]["enabled"], false);
    assert!(
        cccc_core::automation::tick_group(&home, &group.group_id, false)
            .expect("Rust handoff tick")
            .notifications
            .is_empty()
    );
    let state = call(
        &home,
        "group_automation_state",
        json!({"group_id":group.group_id,"by":"user"}),
    );
    assert_eq!(state["status"]["python-once"]["completed"], true);
    assert!(
        state["status"]["python-once"]["completed_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
}

#[test]
fn python_interop_rust_fired_cron_slot_is_not_replayed() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("cron handoff", "").expect("group");
    groups
        .mutate(&group.group_id, |group| {
            cccc_core::actors::add(group, cccc_contracts::Actor::new("peer"))?;
            group.automation = json!({
                "version":1,
                "rules":[{
                    "id":"cron-handoff","enabled":true,"scope":"group","to":["peer"],
                    "trigger":{"kind":"cron","cron":"* * * * *","timezone":"UTC"},
                    "action":{"kind":"notify","message":"fire one slot"}
                }],
                "snippets":{},"snippet_overrides":{}
            })
            .as_object()
            .cloned()
            .expect("automation");
            Ok(())
        })
        .expect("automation rule");
    assert_eq!(
        cccc_core::automation::tick_group(&home, &group.group_id, false)
            .expect("Rust automation tick")
            .notifications
            .len(),
        1
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from datetime import datetime
from unittest.mock import patch

from cccc.daemon.automation import AutomationManager
from cccc.daemon.automation.engine import _load_state
from cccc.kernel.group import load_group

group = load_group(sys.argv[1])
assert group is not None
last_fired = _load_state(group)["rules"]["cron-handoff"]["last_fired_at"]
same_slot = datetime.fromisoformat(last_fired.replace("Z", "+00:00"))
manager = AutomationManager()
with patch("cccc.daemon.automation.engine.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
    "cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None
):
    manager._check_rules(group, same_slot)

events = [json.loads(line) for line in group.ledger_path.read_text(encoding="utf-8").splitlines() if line.strip()]
matching = [
    event for event in events
    if event.get("kind") == "system.notify"
    and isinstance(event.get("data"), dict)
    and (
        event["data"].get("rule_id") == "cron-handoff"
        or (
            isinstance(event["data"].get("context"), dict)
            and event["data"]["context"].get("rule_id") == "cron-handoff"
        )
    )
]
assert len(matching) == 1, matching
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python cron handoff");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_fired_cron_slot_is_not_replayed_by_rust() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("reverse cron handoff", "").expect("group");
    groups
        .mutate(&group.group_id, |group| {
            cccc_core::actors::add(group, cccc_contracts::Actor::new("peer"))?;
            group.automation = json!({
                "version":1,
                "rules":[{
                    "id":"cron-reverse","enabled":true,"scope":"group","to":["peer"],
                    "trigger":{"kind":"cron","cron":"* * * * *","timezone":"UTC"},
                    "action":{"kind":"notify","message":"fire one slot"}
                }],
                "snippets":{},"snippet_overrides":{}
            })
            .as_object()
            .cloned()
            .expect("automation");
            Ok(())
        })
        .expect("automation rule");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from datetime import datetime, timezone
from unittest.mock import patch

from cccc.daemon.automation import AutomationManager
from cccc.kernel.group import load_group

group = load_group(sys.argv[1])
assert group is not None
manager = AutomationManager()
with patch("cccc.daemon.automation.engine.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
    "cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None
):
    manager._check_rules(group, datetime.now(timezone.utc))
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python cron execution");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        cccc_core::automation::tick_group(&home, &group.group_id, false)
            .expect("Rust handoff tick")
            .notifications
            .is_empty()
    );
}

#[test]
fn python_interop_share_inbox_order_status_and_actor_generation() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"inbox interop","by":"user"}),
    );
    let group_id = created["group_id"].as_str().expect("group id");
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,"actor_id":"peer","runtime":"custom","runner":"pty",
            "command":["sh","-c","exit 0"],"by":"user"
        }),
    );
    let groups = GroupStore::new(home.clone()).expect("groups");
    let ledger_path = groups.ledger_path(group_id).expect("ledger path");
    let append = |timestamp: &str, text: &str, request_reply: bool| {
        let mut event = Event::new("chat.message", group_id);
        event.ts = timestamp.into();
        event.by = "user".into();
        event.data = object(json!({
            "to":["peer"],"text":text,
            "message_mode":if request_reply { "request_reply" } else { "mail" }
        }));
        ledger::append(&ledger_path, &event).expect("append message");
        event
    };
    let first = append("2099-01-01T00:00:00Z", "first", false);
    let second = append("2099-01-01T00:00:00Z", "same timestamp", true);
    let third = append("2000-01-01T00:00:00Z", "regressed timestamp", false);
    call(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"peer","by":"peer","limit":1}),
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
from cccc.kernel.group import load_group
from cccc.kernel.inbox import find_event, get_read_status_batch

group_id, first_id, second_id, third_id, third_ts = sys.argv[1:]

def call(op, args):
    response, _ = handle_request(DaemonRequest.model_validate({"op": op, "args": args}))
    assert response.ok, (op, response)
    return response.result

inbox = call("inbox_peek", {
    "group_id": group_id, "actor_id": "peer", "by": "peer", "limit": 10,
})
assert [event["id"] for event in inbox["messages"]] == [third_id]
group = load_group(group_id)
assert group is not None
events = [find_event(group, event_id) for event_id in (first_id, second_id, third_id)]
assert all(event is not None for event in events)
statuses = get_read_status_batch(group, events)
assert statuses[first_id]["peer"] is True
assert second_id not in statuses
assert statuses[third_id]["peer"] is False
marked = call("inbox_read", {
    "group_id": group_id, "actor_id": "peer", "by": "peer", "limit": 1,
})
assert [event["id"] for event in marked["messages"]] == [third_id]
assert marked["cursor"]["event_id"] == third_id
assert marked["cursor"]["ts"] == third_ts
"#,
        )
        .arg(group_id)
        .arg(&first.id)
        .arg(&second.id)
        .arg(&third.id)
        .arg(&third.ts)
        .output()
        .expect("Python advances Rust cursor");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer","by":"peer","limit":10}),
    );
    assert_eq!(
        inbox["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|event| event["id"].as_str().expect("event id"))
            .collect::<Vec<_>>(),
        Vec::<&str>::new()
    );
    let statuses = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[second.id]}),
    );
    assert!(
        statuses["statuses"][&second.id]
            .get("read_status")
            .is_none()
    );
    assert_eq!(
        statuses["statuses"][&second.id]["obligation_status"]["peer"]["reply_requested"],
        true
    );
    assert!(statuses["statuses"][&second.id].get("ack_status").is_none());
    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request
from cccc.kernel.group import load_group
from cccc.kernel.inbox import find_event, get_read_status_batch

group_id, third_id = sys.argv[1:]

def call(op, args):
    response, _ = handle_request(DaemonRequest.model_validate({"op": op, "args": args}))
    assert response.ok, (op, response)
    return response.result

inbox = call("inbox_peek", {
    "group_id": group_id, "actor_id": "peer", "by": "peer", "limit": 10,
})
assert inbox["messages"] == []
group = load_group(group_id)
assert group is not None
event = find_event(group, third_id)
assert event is not None
statuses = get_read_status_batch(group, [event])
assert statuses[third_id]["peer"] is True
call("actor_remove", {"group_id": group_id, "actor_id": "peer", "by": "user"})
call("actor_add", {
    "group_id": group_id, "actor_id": "peer", "runtime": "custom", "runner": "pty",
    "command": ["sh", "-c", "exit 0"], "by": "user",
})
inbox = call("inbox_peek", {
    "group_id": group_id, "actor_id": "peer", "by": "peer", "limit": 10,
})
assert inbox["messages"] == []
"#,
        )
        .arg(group_id)
        .arg(&third.id)
        .output()
        .expect("Python verifies Rust cursor and recreates actor");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer","by":"peer","limit":10}),
    );
    assert!(inbox["messages"].as_array().expect("messages").is_empty());
    let statuses = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[third.id]}),
    );
    assert!(
        statuses["statuses"][&third.id]["read_status"]
            .get("peer")
            .is_none()
    );
}

#[test]
fn python_interop_accept_each_others_group_copy_packages() {
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

#[test]
fn python_interop_share_actor_updates_secrets_and_deletion() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"actor lifecycle interop","by":"user"}),
    );
    let group_id = created["group"]["group_id"].as_str().expect("group id");
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"rust-owned",
            "title":"Rust actor",
            "runtime":"custom",
            "runner":"pty",
            "command":["sh","-c","exit 0"],
            "enabled":false,
            "env_private":{"RUST_SECRET":"rust-value"},
            "by":"user"
        }),
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request

group_id = sys.argv[1]

def call(op, args, *, ok=True):
    response, _ = handle_request(DaemonRequest.model_validate({"op": op, "args": args}))
    assert response.ok is ok, (op, response)
    return response

listed = call("actor_list", {"group_id": group_id, "by": "user"})
rust_actor = next(actor for actor in listed.result["actors"] if actor["id"] == "rust-owned")
assert rust_actor["title"] == "Rust actor"
keys = call("actor_env_private_keys", {
    "group_id": group_id, "actor_id": "rust-owned", "by": "user"
})
assert keys.result["keys"] == ["RUST_SECRET"]
call("actor_update", {
    "group_id": group_id, "actor_id": "rust-owned", "by": "user",
    "patch": {"title": "Updated by Python"},
})
call("actor_remove", {
    "group_id": group_id, "actor_id": "rust-owned", "by": "user",
})
missing = call("actor_env_private_keys", {
    "group_id": group_id, "actor_id": "rust-owned", "by": "user",
}, ok=False)
assert missing.error.code == "actor_not_found"

call("actor_add", {
    "group_id": group_id,
    "actor_id": "python-owned",
    "title": "Python actor",
    "runtime": "custom",
    "runner": "pty",
    "command": ["sh", "-c", "exit 0"],
    "enabled": False,
    "env_private": {"PYTHON_SECRET": "python-value"},
    "by": "user",
})
call("actor_update", {
    "group_id": group_id, "actor_id": "python-owned", "by": "user",
    "patch": {"title": "Updated by Python"},
})
"#,
        )
        .arg(group_id)
        .output()
        .expect("Python actor lifecycle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let listed = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    let python_actor = listed["actors"]
        .as_array()
        .expect("actors")
        .iter()
        .find(|actor| actor["id"] == "python-owned")
        .expect("Python-owned actor");
    assert_eq!(python_actor["title"], "Updated by Python");
    let keys = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"python-owned","by":"user"}),
    );
    assert_eq!(keys["keys"], json!(["PYTHON_SECRET"]));
    let removed = call(
        &home,
        "actor_remove",
        json!({"group_id":group_id,"actor_id":"python-owned","by":"user"}),
    );
    assert_eq!(removed["event"]["kind"], "actor.remove");
    let missing = raw(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"python-owned","by":"user"}),
    );
    assert_eq!(
        missing.error.expect("removed actor error").code,
        "actor_not_found"
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request

group_id = sys.argv[1]
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "actor_list", "args": {"group_id": group_id, "by": "user"},
}))
assert response.ok, response
assert response.result["actors"] == []
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "actor_env_private_keys",
    "args": {"group_id": group_id, "actor_id": "python-owned", "by": "user"},
}))
assert not response.ok, response
assert response.error.code == "actor_not_found"
"#,
        )
        .arg(group_id)
        .output()
        .expect("Python confirms Rust deletion");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_actor_secret_deletion_retires_the_legacy_rust_shadow() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"legacy actor secret retirement","by":"user"}),
    );
    let group_id = created["group"]["group_id"].as_str().expect("group id");
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "runtime":"custom",
            "runner":"pty",
            "command":["sh","-c","exit 0"],
            "enabled":false,
            "by":"user"
        }),
    );
    let legacy_path = home
        .root()
        .join("groups")
        .join(group_id)
        .join("state/actor-secrets.json");
    std::fs::create_dir_all(legacy_path.parent().expect("legacy parent")).expect("legacy parent");
    std::fs::write(
        &legacy_path,
        serde_json::to_vec_pretty(&json!({
            "actors":{"peer":{"STALE_TOKEN":"must-not-return"}}
        }))
        .expect("legacy actor secrets"),
    )
    .expect("legacy actor secret fixture");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request

group_id = sys.argv[1]
listed, _ = handle_request(DaemonRequest.model_validate({
    "op": "actor_env_private_keys",
    "args": {
        "group_id": group_id,
        "actor_id": "peer",
        "by": "user",
    },
}))
assert listed.ok, listed
assert listed.result["keys"] == ["STALE_TOKEN"], listed
response, _ = handle_request(DaemonRequest.model_validate({
    "op": "actor_env_private_update",
    "args": {
        "group_id": group_id,
        "actor_id": "peer",
        "by": "user",
        "clear": True,
    },
}))
assert response.ok, response
assert response.result["keys"] == [], response
"#,
        )
        .arg(group_id)
        .output()
        .expect("Python clears actor secrets");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let keys = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"peer","by":"user"}),
    );
    assert_eq!(keys["keys"], json!([]));
}

#[test]
fn python_interop_profile_deletion_retires_the_legacy_rust_shadow() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    std::fs::write(
        home.root().join("profiles.json"),
        serde_json::to_vec_pretty(&json!({
            "profiles":{
                "legacy":{
                    "id":"legacy",
                    "name":"Legacy Rust profile",
                    "runtime":"codex",
                    "runner":"pty",
                    "command":[],
                    "submit":"enter",
                    "env":{"STALE_TOKEN":"must-not-return"},
                    "revision":1
                }
            }
        }))
        .expect("legacy profiles"),
    )
    .expect("legacy profile fixture");
    std::fs::write(
        home.root().join("profile-secrets.json"),
        serde_json::to_vec_pretty(&json!({
            "profiles":{"legacy":{"STALE_SECRET":"must-not-return"}}
        }))
        .expect("legacy profile secrets"),
    )
    .expect("legacy profile secret fixture");

    let output = python(&repo, temp.path())
        .arg(
            r#"
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request

def call(op, args):
    response, _ = handle_request(DaemonRequest.model_validate({"op": op, "args": args}))
    assert response.ok, (op, response)
    return response

profile = call("actor_profile_get", {
    "by": "user",
    "profile_id": "legacy",
})
assert profile.result["profile"]["name"] == "Legacy Rust profile", profile
assert profile.result["profile"]["env"] == {}, profile
keys = call("actor_profile_secret_keys", {
    "by": "user",
    "profile_id": "legacy",
})
assert set(keys.result["keys"]) == {"STALE_SECRET", "STALE_TOKEN"}, keys
call("actor_profile_delete", {
    "by": "user",
    "profile_id": "legacy",
})
"#,
        )
        .output()
        .expect("Python deletes canonical profile");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let profiles = ProfileStore::new(home).expect("Rust profile store");
    assert_eq!(profiles.get("legacy").expect("Rust profile lookup"), None);
    assert_eq!(
        profiles
            .secret_values("legacy")
            .err()
            .map(|error| error.kind()),
        Some(std::io::ErrorKind::NotFound)
    );
}

#[test]
fn python_interop_im_unset_retires_legacy_rust_durable_shadow() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store.create("legacy IM unset", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "im".into(),
                json!({"platform":"telegram","bot_token_env":"CANONICAL"}),
            );
            group.extra.insert(
                "im_bridge".into(),
                json!({
                    "config":{"platform":"telegram","bot_token_env":"STALE"},
                    "enabled":true,
                    "authorized":[{"chat_id":"stale-chat","thread_id":0,"platform":"telegram"}],
                    "pending":[{"key":"stale-key","chat_id":"stale-chat","thread_id":0,"platform":"telegram","created_at":chrono::Utc::now().timestamp() as f64}],
                    "subscribers":[{"chat_id":"stale-chat","thread_id":0,"platform":"telegram","subscribed":true}],
                    "running":false
                }),
            );
            Ok(())
        })
        .expect("legacy fixture");
    let state_dir = store.state_dir(&group.group_id).expect("state dir");
    cccc_core::fs::write_json(
        &state_dir.join("im_authorized_chats.json"),
        &json!({"current-chat":{"chat_id":"current-chat","thread_id":0,"platform":"telegram"}}),
    )
    .expect("authorized fixture");
    cccc_core::fs::write_json(
        &state_dir.join("im_pending_keys.json"),
        &json!({"current-key":{"chat_id":"current-chat","thread_id":0,"platform":"telegram","created_at":chrono::Utc::now().timestamp() as f64}}),
    )
    .expect("pending fixture");
    cccc_core::fs::write_json(
        &state_dir.join("im_subscribers.json"),
        &json!({"current-chat":{"thread_id":0,"platform":"telegram","subscribed":true}}),
    )
    .expect("subscriber fixture");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import argparse
from cccc.cli.im_cmds import cmd_im_unset

assert cmd_im_unset(argparse.Namespace(group=__import__('sys').argv[1])) == 0
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python IM unset");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let state = im_state::load(&store, &group.group_id).expect("Rust IM reload");
    assert!(state.get("config").is_none(), "{state:#}");
    assert_eq!(state["authorized"], json!([]));
    assert_eq!(state["pending"], json!([]));
    assert_eq!(state["subscribers"], json!([]));
    let group = store.load(&group.group_id).expect("group after unset");
    assert!(group.extra.get("im").is_none());
    assert_eq!(group.extra["im_bridge"], json!({"running":false}));
}

#[test]
fn python_interop_serialize_im_state_with_the_shared_lock() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store.create("IM lock interop", "").expect("group");
    let mut child = python(&repo, temp.path())
        .arg(
            r#"
import sys
from pathlib import Path
from cccc.kernel.im_state import im_state_lock
from cccc.ports.im.auth import KeyManager

state_dir = Path(sys.argv[1])
with im_state_lock(state_dir):
    print("locked", flush=True)
    sys.stdin.readline()
    KeyManager(state_dir).authorize_direct("python-chat", 0, "telegram", "interop")
"#,
        )
        .arg(state_dir_string(&store, &group.group_id))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn Python IM lock holder");
    let mut child_stdin = child.stdin.take().expect("Python stdin");
    let mut child_stdout = BufReader::new(child.stdout.take().expect("Python stdout"));
    let mut ready = String::new();
    child_stdout
        .read_line(&mut ready)
        .expect("Python ready line");
    assert_eq!(ready.trim(), "locked");

    let (sent, received) = mpsc::channel();
    let rust_store = store.clone();
    let group_id = group.group_id.clone();
    let writer = thread::spawn(move || {
        sent.send(im_state::update(&rust_store, &group_id, |state| {
            state["authorized"]
                .as_array_mut()
                .expect("authorized array")
                .push(json!({
                    "chat_id":"rust-chat",
                    "thread_id":0,
                    "platform":"telegram"
                }));
            Ok(())
        }))
        .expect("send Rust result");
    });
    let early_result = received.recv_timeout(Duration::from_millis(100)).ok();
    let rust_writer_was_blocked = early_result.is_none();
    child_stdin.write_all(b"release\n").expect("release Python");
    drop(child_stdin);
    let status = child.wait().expect("wait for Python");
    let rust_result = early_result.unwrap_or_else(|| {
        received
            .recv_timeout(Duration::from_secs(3))
            .expect("Rust writer finishes after Python releases the lock")
    });
    writer.join().expect("Rust writer");

    assert!(status.success());
    assert!(
        rust_writer_was_blocked,
        "Rust IM mutation bypassed the Python-held shared lock"
    );
    rust_result.expect("Rust IM update");
    let state = im_state::load(&store, &group.group_id).expect("shared IM state");
    let chats = state["authorized"]
        .as_array()
        .expect("authorized")
        .iter()
        .filter_map(|item| item["chat_id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        chats,
        std::collections::BTreeSet::from(["python-chat", "rust-chat"])
    );
}

#[test]
fn python_interop_share_im_revoke_and_opaque_thread_state() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store.create("IM state handoff", "").expect("group");
    let state_dir = state_dir_string(&store, &group.group_id);
    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from pathlib import Path
from cccc.kernel.im_state import im_state_lock
from cccc.ports.im.auth import KeyManager
from cccc.ports.im.subscribers import SubscriberManager

state_dir = Path(sys.argv[1])
thread_id = "1710000000.100"
with im_state_lock(state_dir):
    keys = KeyManager(state_dir)
    key = keys.generate_key("C-shared", thread_id, "slack")
    keys.authorize("C-shared", thread_id, "slack", key)
    SubscriberManager(state_dir).subscribe(
        "C-shared", thread_id=thread_id, platform="slack"
    )
"#,
        )
        .arg(&state_dir)
        .output()
        .expect("Python IM seed");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let state = im_state::load(&store, &group.group_id).expect("Rust reads Python IM state");
    assert_eq!(state["authorized"][0]["thread_id"], "1710000000.100");
    assert_eq!(state["subscribers"][0]["thread_id"], "1710000000.100");
    let revoked = call(
        &home,
        "im_revoke_chat",
        json!({
            "group_id":group.group_id,
            "chat_id":"C-shared",
            "thread_id":"1710000000.100"
        }),
    );
    assert_eq!(revoked["revoked"], true);
    assert_eq!(revoked["unsubscribed"], true);

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from pathlib import Path
from cccc.ports.im.auth import KeyManager
from cccc.ports.im.subscribers import SubscriberManager

state_dir = Path(sys.argv[1])
thread_id = "1710000000.100"
assert not KeyManager(state_dir).is_authorized("C-shared", thread_id, "slack")
assert not SubscriberManager(state_dir).is_subscribed("C-shared", thread_id)
"#,
        )
        .arg(&state_dir)
        .output()
        .expect("Python verifies Rust revoke");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_interop_share_presentation_state_and_result_contracts() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("Presentation interop", "")
        .expect("group");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request

response, _ = handle_request(DaemonRequest.model_validate({
    "op": "presentation_publish",
    "args": {
        "group_id": sys.argv[1],
        "slot": "slot-2",
        "card_type": "pdf",
        "url": "https://example.test/python.pdf",
        "title": "Python deck",
        "summary": "published by Python",
        "by": "user",
    },
}))
assert response.ok, response
print(json.dumps(response.result))
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python publishes presentation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_publish: Value = serde_json::from_slice(&output.stdout).expect("Python result");
    assert_eq!(python_publish["slot_id"], "slot-2");
    assert_eq!(python_publish["replaced"], false);
    assert_eq!(python_publish["event"]["kind"], "presentation.publish");
    assert_eq!(python_publish["event_id"], python_publish["event"]["id"]);

    let rust_get = call(
        &home,
        "presentation_get",
        json!({"group_id":group.group_id}),
    );
    assert_eq!(
        rust_get["presentation"]["slots"][1]["card"]["title"],
        "Python deck"
    );
    assert_eq!(
        rust_get["presentation"]["slots"][1]["card"]["content"]["url"],
        "https://example.test/python.pdf"
    );

    let rust_publish = call(
        &home,
        "presentation_publish",
        json!({
            "group_id":group.group_id,
            "slot":"slot-2",
            "card_type":"image",
            "url":"https://example.test/rust.png",
            "title":"Rust image",
            "summary":"published by Rust",
            "by":"user"
        }),
    );
    assert_eq!(rust_publish["replaced"], true);
    assert_eq!(rust_publish["event"]["kind"], "presentation.publish");
    assert_eq!(rust_publish["event_id"], rust_publish["event"]["id"]);
    assert_eq!(
        rust_publish["event"]["data"]["summary"],
        "published by Rust"
    );

    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.server import handle_request

def call(op, args):
    response, _ = handle_request(DaemonRequest.model_validate({"op": op, "args": args}))
    assert response.ok, (op, response)
    return response.result

group_id = sys.argv[1]
snapshot = call("presentation_get", {"group_id": group_id})["presentation"]
assert snapshot["slots"][1]["card"]["title"] == "Rust image", snapshot
cleared = call("presentation_clear", {"group_id": group_id, "all": True, "by": "user"})
assert cleared["event"]["data"]["cleared_all"] is True, cleared
assert cleared["event_id"] == cleared["event"]["id"], cleared
print(json.dumps(cleared))
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("Python reads and clears Rust presentation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_clear: Value = serde_json::from_slice(&output.stdout).expect("Python clear result");
    assert_eq!(python_clear["cleared_slots"], json!(["slot-2"]));
    assert_eq!(python_clear["event"]["kind"], "presentation.clear");

    let final_get = call(
        &home,
        "presentation_get",
        json!({"group_id":group.group_id}),
    );
    assert!(
        final_get["presentation"]["slots"]
            .as_array()
            .expect("slots")
            .iter()
            .all(|slot| slot["card"].is_null())
    );
}

fn state_dir_string(store: &GroupStore, group_id: &str) -> String {
    store
        .state_dir(group_id)
        .expect("state dir")
        .to_string_lossy()
        .into_owned()
}

fn call(home: &HomeLayout, op: &str, args: Value) -> Map<String, Value> {
    let response = raw(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response.result
}

fn raw(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        },
    )
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
