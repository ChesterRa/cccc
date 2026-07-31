use cccc_contracts::Event;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
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
fn rust_append_waits_for_the_python_ledger_lock() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf();
    let temp = tempfile::tempdir().expect("tempdir");
    let group = temp.path().join("g_interop");
    std::fs::create_dir_all(&group).expect("group");
    let ledger = group.join("ledger.jsonl");
    let script = r#"
import sys
from pathlib import Path
from cccc.util.file_lock import acquire_lockfile, release_lockfile

ledger = Path(sys.argv[1])
lock = ledger.parent / "state" / "ledger" / "ledger.lock"
handle = acquire_lockfile(lock, blocking=True)
print("locked", flush=True)
sys.stdin.readline()
release_lockfile(handle)
"#;
    let mut python = Command::new(python_executable(&repo))
        .arg("-c")
        .arg(script)
        .arg(&ledger)
        .env("PYTHONPATH", repo.join("src"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Python");
    let mut ready = String::new();
    BufReader::new(python.stdout.take().expect("stdout"))
        .read_line(&mut ready)
        .expect("read ready");
    assert_eq!(ready.trim(), "locked");

    let (sent, received) = mpsc::sync_channel(1);
    let append_path = ledger.clone();
    let writer = std::thread::spawn(move || {
        sent.send(cccc_core::ledger::append(
            &append_path,
            &Event::new("chat.message", "g_interop"),
        ))
        .expect("send result");
    });
    assert!(
        received.recv_timeout(Duration::from_millis(100)).is_err(),
        "Rust append bypassed the Python ledger lock"
    );

    python
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"\n")
        .expect("release Python lock");
    received
        .recv_timeout(Duration::from_secs(5))
        .expect("Rust append completed")
        .expect("append");
    writer.join().expect("writer");
    let output = python.wait_with_output().expect("wait Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
