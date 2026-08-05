use cccc_core::HomeLayout;
use cccc_core::runtime_hook_identity::{
    RuntimeHookLaunchIdentity, read as read_identity, write as write_identity,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn python_and_rust_share_launch_identity_format() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let identity =
        RuntimeHookLaunchIdentity::new("g_interop", "peer", "codex", "rust-token", true, 42);
    write_identity(&home, &identity).expect("Rust writes identity");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import json
import sys
from pathlib import Path
from cccc.daemon.runtime_hooks.launch import _write_launch_identity
from cccc.kernel.runtime_hooks.projection import read_launch_identity
home = Path(sys.argv[1])
current = read_launch_identity(home, "g_interop", "peer")
print(json.dumps(current))
_write_launch_identity(
    home, "g_interop", "peer", "claude", "python-token",
    hook_enabled=True, pid=84,
)
"#,
        )
        .arg(temp.path())
        .output()
        .expect("run Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_read: Value = serde_json::from_slice(&output.stdout).expect("Python JSON");
    assert_eq!(python_read["launch_token"], "rust-token");
    assert_eq!(python_read["pid"], 42);

    let python_identity =
        read_identity(&home, "g_interop", "peer").expect("Rust reads Python identity");
    assert_eq!(python_identity.runtime, "claude");
    assert_eq!(python_identity.launch_token, "python-token");
    assert_eq!(python_identity.pid, 84);
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
