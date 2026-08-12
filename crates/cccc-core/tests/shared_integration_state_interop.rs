use cccc_core::{GroupStore, HomeLayout, assistant_state, group_bridge_legacy, integration_state};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn python_executable(repo: &Path) -> PathBuf {
    std::env::var_os("CCCC_TEST_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(windows) {
                repo.join(".venv/Scripts/python.exe")
            } else {
                repo.join(".venv/bin/python")
            }
        })
}

fn python(repo: &Path, home: &Path, script: &str, group_id: &str) {
    let output = Command::new(python_executable(repo))
        .arg("-c")
        .arg(script)
        .arg(home)
        .arg(group_id)
        .env("CCCC_HOME", home)
        .env("PYTHONPATH", repo.join("src"))
        .output()
        .expect("run Python interop step");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn python_and_rust_serialize_group_bridge_state_with_the_shared_lock() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let script = r#"
import sys
from pathlib import Path
import yaml
from cccc.util.file_lock import acquire_lockfile, release_lockfile
from cccc.util.fs import atomic_write_text

home = Path(sys.argv[1])
lock = acquire_lockfile(home / "group_bridge_state.lock", blocking=True)
try:
    print("locked", flush=True)
    sys.stdin.readline()
    atomic_write_text(
        home / "group_bridge_pairing.yaml",
        yaml.safe_dump({
            "invites": {},
            "requests": {},
            "trusts": {
                "trust_python_revoked": {
                    "trust_id": "trust_python_revoked",
                    "group_id": "g_local",
                    "remote_group_id": "g_remote",
                    "remote_peer_id": "peer_remote",
                    "status": "revoked",
                }
            },
            "outbounds": {},
        }, sort_keys=True),
    )
finally:
    release_lockfile(lock)
"#;
    let mut child = Command::new(python_executable(&repo))
        .arg("-c")
        .arg(script)
        .arg(temp.path())
        .env("CCCC_HOME", temp.path())
        .env("PYTHONPATH", repo.join("src"))
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
    let rust_home = home.clone();
    let writer = thread::spawn(move || {
        sent.send(group_bridge_legacy::update(&rust_home, |state| {
            state.insert(
                "deliveries".into(),
                json!([{
                    "registration_id":"registration_rust",
                    "idempotency_key":"delivery_rust",
                    "status":"queued"
                }]),
            );
            Ok(())
        }))
        .expect("send Rust result");
    });
    assert!(
        received.recv_timeout(Duration::from_millis(100)).is_err(),
        "Rust mutation bypassed the Python-held Group Bridge state lock"
    );

    child_stdin.write_all(b"release\n").expect("release Python");
    drop(child_stdin);
    let status = child.wait().expect("wait for Python");
    received
        .recv_timeout(Duration::from_secs(3))
        .expect("Rust mutation after Python releases the lock")
        .expect("Rust state update");
    writer.join().expect("Rust writer");

    assert!(status.success());
    let bridge = group_bridge_legacy::load(&home).expect("shared bridge state");
    assert_eq!(bridge["trusts"][0]["status"], "revoked");
    assert_eq!(bridge["deliveries"][0]["status"], "queued");
}

#[test]
fn group_bridge_and_voice_workflow_share_one_cross_engine_authority() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups
        .create("shared integration state", "")
        .expect("group");

    integration_state::global_update(&home, "group_bridge", |state| {
        *state = json!({
            "requests":[{
                "request_id":"request-shared","registration_id":"registration-shared",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer-remote","status":"approved"
            }],
            "trusts":[{
                "trust_id":"trust-shared","request_id":"request-shared",
                "registration_id":"registration-shared","group_id":group.group_id,
                "remote_group_id":"g_remote","remote_peer_id":"peer-remote",
                "status":"active"
            }],
            "registrations":[{
                "registration_id":"registration-shared","group_id":group.group_id,
                "remote_group_id":"g_remote","remote_peer_id":"peer-remote",
                "credential":"remote-send-secret","status":"active"
            }]
        });
        Ok(())
    })
    .expect("legacy Rust bridge state");
    assert_eq!(
        group_bridge_legacy::load(&home).expect("migrate bridge")["trusts"][0]["status"],
        "active"
    );
    group_bridge_legacy::update(&home, |state| {
        state.insert(
            "deliveries".into(),
            json!([{
                "registration_id":"registration-shared",
                "idempotency_key":"delivery-once",
                "status":"queued"
            }]),
        );
        Ok(())
    })
    .expect("Rust receipt");

    python(
        &repo,
        temp.path(),
        r#"
import sys
from cccc.daemon.assistants.assistant_ops import _load_runtime_state, _save_runtime_state
from cccc.kernel.group import load_group
from cccc.kernel.group_bridge.pairing import revoke_trust
from cccc.kernel.group_bridge.receipts import get_receipt, update_receipt

group_id = sys.argv[2]
group = load_group(group_id)
assert group is not None
receipt = get_receipt("registration-shared", "delivery-once")
assert receipt and receipt["status"] == "queued"
assert update_receipt("registration-shared", "delivery-once", status="sent")["status"] == "sent"
revoked = revoke_trust("trust-shared", revoked_by="python-test")
assert revoked["status"] == "revoked"

state = _load_runtime_state(group)
state["assistants"]["voice_secretary"] = {
    "lifecycle": "working",
    "health": {"status": "draft_ready", "source": "python", "pid": 111},
}
state["voice_sessions"]["session-python"] = {
    "session_id": "session-python", "capture_mode": "document"
}
state["voice_prompt_drafts"]["draft-python"] = {
    "request_id": "draft-python", "status": "pending", "updated_at": "2026-08-10T01:00:00Z"
}
state["voice_prompt_requests"]["draft-python"] = {"request_id": "draft-python"}
state["voice_ask_requests"]["ask-python"] = {
    "request_id": "ask-python", "status": "pending"
}
_save_runtime_state(group, state)
"#,
        &group.group_id,
    );

    // Simulate a stale pre-migration Rust store reappearing after Python has
    // revoked the route. Canonical terminal state must still win.
    integration_state::global_update(&home, "group_bridge", |state| {
        *state = json!({
            "trusts":[{
                "trust_id":"trust-shared","request_id":"request-shared",
                "registration_id":"registration-shared","group_id":group.group_id,
                "remote_group_id":"g_remote","remote_peer_id":"peer-remote",
                "credential":"stale-secret","status":"active"
            }],
            "registrations":[{
                "registration_id":"registration-alias","group_id":group.group_id,
                "remote_group_id":"g_remote","remote_peer_id":"peer-remote",
                "credential":"stale-secret","status":"active"
            }]
        });
        Ok(())
    })
    .expect("stale legacy bridge state");

    let bridge = group_bridge_legacy::load(&home).expect("Rust canonical bridge reload");
    assert_eq!(bridge["trusts"][0]["status"], "revoked");
    assert!(
        bridge["registrations"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert_eq!(bridge["deliveries"][0]["status"], "sent");
    assert!(
        integration_state::global_get(&home, "group_bridge")
            .expect("retired legacy store")
            .is_null()
    );

    let voice = assistant_state::load(&home, &group.group_id).expect("Rust voice load");
    assert_eq!(voice["assistant"]["lifecycle"], "working");
    assert_eq!(voice["assistant"]["health"]["source"], "python");
    assert!(voice["assistant"]["health"].get("pid").is_none());
    assert_eq!(voice["prompt_draft"]["request_id"], "draft-python");
    assert_eq!(voice["sessions"][0]["session_id"], "session-python");
    assert_eq!(voice["ask_requests"][0]["request_id"], "ask-python");

    assistant_state::update(&home, &group.group_id, |state| {
        state["assistant"]["lifecycle"] = json!("waiting");
        state["assistant"]["health"] = json!({"status":"rust-ready","pid":222});
        state["sessions"]
            .as_array_mut()
            .expect("sessions")
            .push(json!({"session_id":"session-rust","capture_mode":"document"}));
        state["voice_prompt_drafts"]["draft-rust"] = json!({
            "request_id":"draft-rust","status":"pending","updated_at":"2026-08-10T02:00:00Z"
        });
        state["ask_requests"]
            .as_array_mut()
            .expect("asks")
            .push(json!({"request_id":"ask-rust","status":"pending"}));
        state.insert("native_extension".into(), json!({"revision":9}));
        Ok(())
    })
    .expect("Rust voice update");

    python(
        &repo,
        temp.path(),
        r#"
import sys
from cccc.daemon.assistants.assistant_ops import _load_runtime_state, _save_runtime_state
from cccc.kernel.group import load_group
from cccc.kernel.group_bridge.credentials import lookup_pairing_remote_send_credential
from cccc.kernel.group_bridge.pairing import list_trusts
from cccc.kernel.group_bridge.receipts import get_receipt
from cccc.kernel.group_bridge.registration import get_registration

group_id = sys.argv[2]
assert list_trusts(group_id=group_id)[0]["status"] == "revoked"
assert get_registration("registration-shared") is None
assert get_registration("registration-alias") is None
assert lookup_pairing_remote_send_credential("remote-send-secret") is None
assert lookup_pairing_remote_send_credential("stale-secret") is None
assert get_receipt("registration-shared", "delivery-once")["status"] == "sent"

group = load_group(group_id)
state = _load_runtime_state(group)
assert state["assistants"]["voice_secretary"]["lifecycle"] == "waiting"
assert state["assistants"]["voice_secretary"]["health"] == {"status": "rust-ready"}
assert "session-python" in state["voice_sessions"]
assert "session-rust" in state["voice_sessions"]
assert "draft-rust" in state["voice_prompt_drafts"]
assert "ask-rust" in state["voice_ask_requests"]
assert state["rust_state"]["native_extension"]["revision"] == 9
state["assistants"]["voice_secretary"]["lifecycle"] = "idle"
_save_runtime_state(group, state)
"#,
        &group.group_id,
    );

    let voice = assistant_state::load(&home, &group.group_id).expect("final Rust voice load");
    assert_eq!(voice["assistant"]["lifecycle"], "idle");
    assert_eq!(voice["native_extension"]["revision"], 9);
}
