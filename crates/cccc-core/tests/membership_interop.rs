use cccc_core::{HomeLayout, membership};
use serde_json::Value;
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

fn python(repo: &Path, home: &Path, script: &str) -> Value {
    let output = Command::new(python_executable(repo))
        .arg("-c")
        .arg(script)
        .arg(home)
        .env("CCCC_HOME", home)
        .env("PYTHONPATH", repo.join("src"))
        .output()
        .expect("run Python membership step");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("Python JSON")
}

#[test]
fn python_interop_preserves_the_complete_membership_shape() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    python(
        &repo,
        temp.path(),
        r#"
import json
from cccc.kernel.membership import save_membership
state = save_membership({
    "logged_in": True,
    "account_origin": "https://issuer.example.test",
    "device_id": "device-python",
    "device_token": "device-secret",
    "hostname": "https://device.example.test",
    "tunnel_token": "tunnel-secret",
    "pending_login": {"device_code": "pending-secret", "interval": 120},
})
print(json.dumps(state))
"#,
    );
    let loaded = membership::load(&home).expect("Rust load");
    assert_eq!(loaded.device_token.as_deref(), Some("device-secret"));
    assert_eq!(
        loaded.account_origin.as_deref(),
        Some("https://issuer.example.test")
    );
    assert_eq!(loaded.tunnel_token.as_deref(), Some("tunnel-secret"));
    membership::update(&home, |state| {
        state.last_error = Some("updated-by-rust".into());
        Ok(())
    })
    .expect("Rust update");
    let loaded = python(
        &repo,
        temp.path(),
        r#"
import json
from cccc.kernel.membership import load_membership
print(json.dumps(load_membership()))
"#,
    );
    assert_eq!(loaded["device_token"], "device-secret");
    assert_eq!(loaded["account_origin"], "https://issuer.example.test");
    assert_eq!(loaded["tunnel_token"], "tunnel-secret");
    assert_eq!(loaded["pending_login"]["device_code"], "pending-secret");
    assert_eq!(loaded["last_error"], "updated-by-rust");
}

#[test]
fn python_interop_serializes_membership_updates_with_the_shared_lock() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let script = r#"
import json
import sys
from pathlib import Path
from cccc.kernel.membership import membership_lock_path, membership_path
from cccc.util.file_lock import acquire_lockfile, release_lockfile
from cccc.util.fs import atomic_write_json

home = Path(sys.argv[1])
lock = acquire_lockfile(membership_lock_path(home), blocking=True)
try:
    print("locked", flush=True)
    sys.stdin.readline()
    atomic_write_json(membership_path(home), {
        "logged_in": True,
        "device_id": "device-python",
        "device_token": "device-secret",
        "hostname": None,
        "tunnel_token": None,
        "disabled": False,
        "last_error": None,
        "pending_login": None,
    })
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
    child_stdout.read_line(&mut ready).expect("ready");
    assert_eq!(ready.trim(), "locked");

    let (sent, received) = mpsc::channel();
    let rust_home = home.clone();
    let writer = thread::spawn(move || {
        sent.send(membership::update(&rust_home, |state| {
            state.hostname = Some("https://rust.example.test".into());
            Ok(())
        }))
        .expect("send result");
    });
    assert!(
        received.recv_timeout(Duration::from_millis(100)).is_err(),
        "Rust mutation bypassed the Python-held membership lock"
    );
    child_stdin.write_all(b"release\n").expect("release");
    drop(child_stdin);
    assert!(child.wait().expect("wait").success());
    received
        .recv_timeout(Duration::from_secs(3))
        .expect("Rust result")
        .expect("Rust update");
    writer.join().expect("writer");
    let state = membership::load(&home).expect("final state");
    assert_eq!(state.device_token.as_deref(), Some("device-secret"));
    assert_eq!(state.hostname.as_deref(), Some("https://rust.example.test"));
}
