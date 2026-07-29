use cccc_core::fs::with_exclusive_lock;
use cccc_core::{HomeLayout, codex_hook_state, runtime_activity};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::Duration;

const GROUP_ID: &str = "g_interop";
const ACTOR_ID: &str = "peer";

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

fn python_command(repo: &Path, home: &Path, script: &str) -> Command {
    let mut command = Command::new(python_executable(repo));
    command
        .arg("-c")
        .arg(script)
        .arg(home)
        .env("PYTHONPATH", repo.join("src"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn checked(output: Output) -> String {
    assert!(
        output.status.success(),
        "python interop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("python output must be utf-8")
        .trim()
        .to_owned()
}

fn actor_digest() -> String {
    let mut digest = Sha256::new();
    digest.update(GROUP_ID.as_bytes());
    digest.update([0]);
    digest.update(ACTOR_ID.as_bytes());
    format!("{:x}", digest.finalize())
}

#[test]
fn python_and_rust_processes_share_paths_files_and_locks() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");

    codex_hook_state::begin_launch(
        &home,
        "codex",
        GROUP_ID,
        ACTOR_ID,
        "token-rust",
        "HookPending",
    )
    .expect("rust launch");
    let python_read_and_record = r#"
import json
import sys
from pathlib import Path
from cccc.kernel.runtime_hooks.activity import read_events
from cccc.kernel.runtime_hooks.store import read_state, record_hook_event
home = Path(sys.argv[1])
state = read_state(home, "codex", "g_interop", "peer")
record_hook_event(
    home, "codex", "g_interop", "peer", "token-rust",
    {"hook_event_name": "SessionStart", "session_id": "session-python"},
)
print(json.dumps({"token": state.launch_token, "events": len(read_events(home, "g_interop"))}))
"#;
    let python_result: Value = serde_json::from_str(&checked(
        python_command(&repo, temp.path(), python_read_and_record)
            .output()
            .expect("run python"),
    ))
    .expect("python result");
    assert_eq!(python_result["token"], "token-rust");
    assert_eq!(python_result["events"], 1);
    assert_eq!(
        codex_hook_state::read(&home, GROUP_ID, ACTOR_ID)
            .expect("rust reads python state")
            .session_id,
        "session-python"
    );
    assert_eq!(
        runtime_activity::read_events(&home, GROUP_ID)
            .expect("rust reads python activity")
            .len(),
        1
    );

    codex_hook_state::begin_launch(
        &home,
        "codex",
        GROUP_ID,
        ACTOR_ID,
        "token-rust-2",
        "HookPending",
    )
    .expect("second rust launch");
    let payload = json!({
        "hook_event_name": "SessionStart",
        "session_id": "session-rust",
    });
    codex_hook_state::record_runtime_with_observer(
        &home,
        "codex",
        GROUP_ID,
        ACTOR_ID,
        "token-rust-2",
        &payload,
        |state, authorized| {
            if authorized {
                runtime_activity::record_hook_event(
                    &home,
                    "codex",
                    "token-rust-2",
                    &payload,
                    state,
                )?;
            }
            Ok(())
        },
    )
    .expect("rust records state and activity");
    let python_reads_rust = r#"
import json
import sys
from pathlib import Path
from cccc.kernel.runtime_hooks.activity import read_events
from cccc.kernel.runtime_hooks.store import begin_launch, read_state
home = Path(sys.argv[1])
state = read_state(home, "codex", "g_interop", "peer")
events = read_events(home, "g_interop")
print(json.dumps({"token": state.launch_token, "session": state.session_id, "events": len(events)}))
"#;
    let python_result: Value = serde_json::from_str(&checked(
        python_command(&repo, temp.path(), python_reads_rust)
            .output()
            .expect("run python"),
    ))
    .expect("python result");
    assert_eq!(python_result["token"], "token-rust-2");
    assert_eq!(python_result["session"], "session-rust");
    assert_eq!(python_result["events"], 2);

    let lock_path = temp
        .path()
        .join("daemon/codex-hook-state")
        .join(format!("{}.lock", actor_digest()));
    let lock_script = r#"
import sys
from pathlib import Path
from cccc.kernel.runtime_hooks.store import begin_launch
begin_launch(
    Path(sys.argv[1]), "codex", "g_interop", "peer",
    "token-python-after-lock", "HookPending",
)
"#;
    let child: Child = with_exclusive_lock(&lock_path, || {
        let mut child = python_command(&repo, temp.path(), lock_script).spawn()?;
        thread::sleep(Duration::from_millis(250));
        assert!(
            child.try_wait()?.is_none(),
            "python write must block on the Rust-held lock"
        );
        Ok::<Child, io::Error>(child)
    })
    .expect("hold Rust lock");
    let output = child.wait_with_output().expect("wait for python");
    checked(output);
    assert_eq!(
        codex_hook_state::read(&home, GROUP_ID, ACTOR_ID)
            .expect("state after lock handoff")
            .launch_token,
        "token-python-after-lock"
    );
}
