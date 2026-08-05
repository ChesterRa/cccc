use cccc_core::HomeLayout;
use cccc_core::group_bridge_identity::{GroupBridgeIdentity, authenticated_session_peer_id};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

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
fn python_and_rust_share_identity_and_signed_session_hello() {
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
