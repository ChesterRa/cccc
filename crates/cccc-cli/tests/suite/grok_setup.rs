use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn grok_setup_reconciles_native_registration_without_changing_imported_config() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join(".grok");
    std::fs::create_dir(&provider).expect("provider home");
    let claude = root.join(".claude.json");
    let imported = r#"{"mcpServers":{"cccc":{"command":"legacy-python"}}}"#;
    std::fs::write(&claude, imported).expect("imported config");
    let grok = root.join("grok");
    std::fs::write(&grok, r#"#!/usr/bin/env python3
import json, os, pathlib, sys
path = pathlib.Path(os.environ['GROK_HOME']) / 'config.toml'
if sys.argv[1:] == ['inspect', '--json']:
    entries = [{'name': 'cccc', 'transport': 'stdio', 'target': os.environ['CCCC_CLI'],
                'source': {'type': 'configToml', 'path': str(path)}}] if path.exists() else []
    print(json.dumps({'mcpServers': entries}))
elif sys.argv[1:] == ['mcp', 'list', '--json']:
    print(json.dumps([{'name': 'cccc', 'command': os.environ['CCCC_CLI'],
                       'args': ['mcp'], 'enabled': True, 'scope': 'user'}]))
else:
    assert sys.argv[1:] == ['mcp', 'add', '--scope', 'user', 'cccc', '--', '${CCCC_CLI:-cccc}', 'mcp']
    assert not path.exists(), 'matching registration must not be rewritten'
    path.write_text("[mcp_servers.cccc]\ncommand='${CCCC_CLI:-cccc}'\nargs=['mcp']\n")
"#).expect("native CLI fixture");
    std::fs::set_permissions(&grok, std::fs::Permissions::from_mode(0o700)).expect("executable");
    let setup = || {
        Command::new(env!("CARGO_BIN_EXE_cccc"))
            .args(["setup", "--runtime", "grok", "--path"])
            .arg(root)
            .env_clear()
            .env("HOME", root)
            .env("CCCC_HOME", root.join("cccc-home"))
            .env("GROK_HOME", &provider)
            .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
            .output()
            .expect("CLI setup")
    };
    for _ in 0..2 {
        let output = setup();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: Value = serde_json::from_slice(&output.stdout).expect("JSON");
        assert_eq!(response["status"], "ready");
        assert_eq!(response["mcp"], "native_registry");
        assert_eq!(
            response["path"],
            provider.join("config.toml").to_str().expect("path")
        );
    }
    assert_eq!(
        std::fs::read_to_string(claude).expect("imported config"),
        imported
    );
    let malformed = "private-fixture-value =";
    std::fs::write(provider.join("config.toml"), malformed).expect("malformed config");
    let output = setup();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("correct the TOML"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-fixture-value"));
    assert_eq!(
        std::fs::read_to_string(provider.join("config.toml")).expect("preserved config"),
        malformed
    );
}
