use cccc_core::HomeLayout;
use cccc_core::group_bridge_identity::{GroupBridgeIdentity, authenticated_session_peer_id};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

#[test]
fn python_interop_share_identity_and_signed_session_hello() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf();
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let rust_identity = GroupBridgeIdentity::load_or_create(&home).expect("Rust identity");
    let script = r#"
import json
import sys
from pathlib import Path
from cccc.daemon.group_bridge.identity import get_group_bridge_identity
from cccc.daemon.group_bridge.ws_auth import sign_session_hello
home = Path(sys.argv[1])
identity = get_group_bridge_identity(home=home)
hello = sign_session_hello({
    "target_group_id": "g_remote",
    "src_group_id": "g_local",
}, home=home)
print(json.dumps({"peer_id": identity.peer_id, "hello": hello}))
"#;
    let output = Command::new(python_executable(&repo))
        .arg("-c")
        .arg(script)
        .arg(temp.path())
        .env("PYTHONPATH", repo.join("src"))
        .output()
        .expect("run Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("Python JSON");
    assert_eq!(result["peer_id"], rust_identity.peer_id);
    assert_eq!(
        authenticated_session_peer_id(&result["hello"]).as_deref(),
        Some(rust_identity.peer_id.as_str())
    );
}

#[test]
fn python_interop_serialize_identity_initialization_with_the_shared_lock() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf();
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let script = r#"
import base64
import sys
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
from cccc.daemon.group_bridge.identity import (
    _new_private_key_b64,
    _peer_id_for_public_key,
    _save_yaml,
)
from cccc.util.file_lock import acquire_lockfile, release_lockfile

home = Path(sys.argv[1])
lock = acquire_lockfile(home / "group_bridge_identity_key.lock", blocking=True)
try:
    print("locked", flush=True)
    sys.stdin.readline()
    private_b64 = _new_private_key_b64()
    private_raw = base64.b64decode(private_b64.encode("ascii"))
    key = Ed25519PrivateKey.from_private_bytes(private_raw)
    public_raw = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    public_b64 = base64.b64encode(public_raw).decode("ascii")
    peer_id = _peer_id_for_public_key(public_raw)
    _save_yaml(home / "group_bridge_identity_key.yaml", {
        "private_key": private_b64,
        "public_key": public_b64,
        "peer_id": peer_id,
    })
    print(peer_id, flush=True)
finally:
    release_lockfile(lock)
"#;
    let mut child = Command::new(python_executable(&repo))
        .arg("-c")
        .arg(script)
        .arg(temp.path())
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
    let initializer = thread::spawn(move || {
        sent.send(GroupBridgeIdentity::load_or_create(&rust_home))
            .expect("send Rust result");
    });
    assert!(
        received.recv_timeout(Duration::from_millis(100)).is_err(),
        "Rust initialization bypassed the Python-held identity lock"
    );

    child_stdin.write_all(b"release\n").expect("release Python");
    drop(child_stdin);
    let mut python_peer_id = String::new();
    child_stdout
        .read_line(&mut python_peer_id)
        .expect("Python peer id");
    let status = child.wait().expect("wait for Python");
    let rust_identity = received
        .recv_timeout(Duration::from_secs(3))
        .expect("Rust initialization after Python releases the lock")
        .expect("Rust identity");
    initializer.join().expect("Rust initializer");

    assert!(status.success());
    assert_eq!(rust_identity.peer_id, python_peer_id.trim());
}
