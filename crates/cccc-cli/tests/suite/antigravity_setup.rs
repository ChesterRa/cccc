use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn explicit_acp_setup_login_runs_outside_the_cli_async_executor() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
    home.initialize().expect("initialize");
    let installation = home
        .root()
        .join("runtimes/antigravity-acp")
        .join(cccc_daemon::antigravity_acp_setup::VERSION)
        .join("linux-x86_64");
    std::fs::create_dir_all(&installation).expect("distribution fixture");
    let script = installation.join("agy_acp_server.par");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json, os, pathlib, sys
managed = pathlib.Path(os.environ['CCCC_HOME']) / 'state/antigravity-acp/home'
assert pathlib.Path(os.environ['HOME']) == managed
assert pathlib.Path(os.environ['GEMINI_HOME']) == managed / '.gemini'
for line in sys.stdin:
    request = json.loads(line)
    with (managed / 'login-methods.jsonl').open('a') as record:
        record.write(json.dumps(request['method']) + '\n')
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':{}}), flush=True)
"#,
    )
    .expect("mock official protocol");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).expect("executable");
    std::fs::write(installation.join("localharness_external"), b"fixture").expect("mock harness");
    cccc_core::fs::write_json(
        &installation.join("installation.json"),
        &json!({"version":cccc_daemon::antigravity_acp_setup::VERSION,
            "archive_sha256":"9fb60956af0a9d76220a4db91ca9ac88e2a2372ad68f985ab5fceace6b825b96"}),
    )
    .expect("mock installed distribution");
    let native = temp.path().join("native-home");
    std::fs::create_dir_all(&native).expect("native home");
    let native_state = native.join("unchanged-fixture");
    std::fs::write(&native_state, b"native fixture remains untouched").expect("native marker");
    let mut command = Command::new(env!("CARGO_BIN_EXE_cccc"));
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("CCCC_")) {
        command.env_remove(key);
    }
    let output = command
        .env("CCCC_HOME", home.root())
        .env("HOME", &native)
        .env("GEMINI_HOME", native.join(".gemini"))
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .args([
            "setup",
            "--runtime",
            "antigravity",
            "--runtime-mode",
            "acp",
            "--login",
        ])
        .output()
        .expect("isolated CLI");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("setup result");
    assert_eq!(result["status"], "authenticated");
    let methods = std::fs::read_to_string(
        cccc_daemon::antigravity_acp_setup::provider_home(&home).join("login-methods.jsonl"),
    )
    .expect("protocol methods");
    assert_eq!(methods, "\"initialize\"\n\"authenticate\"\n");
    assert_eq!(
        std::fs::read(&native_state).expect("preserved native state"),
        b"native fixture remains untouched"
    );
    assert!(!native.join(".gemini").exists());
}
