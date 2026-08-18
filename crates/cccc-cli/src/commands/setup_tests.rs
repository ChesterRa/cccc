use super::*;

#[test]
fn public_launcher_override_must_be_an_existing_absolute_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let current = temp.path().join("cccc-rust");
    let launcher = temp.path().join("cccc");
    std::fs::write(&launcher, b"launcher").expect("write launcher");

    assert_eq!(
        select_public_executable(current.clone(), Some(launcher.clone())),
        launcher
    );
    assert_eq!(
        select_public_executable(current.clone(), Some(PathBuf::from("cccc"))),
        current
    );
    let other = temp.path().join("other");
    std::fs::write(&other, b"other").expect("write other");
    assert_eq!(
        select_public_executable(current.clone(), Some(other)),
        current
    );
}

#[test]
fn builds_codex_command_with_compiled_binary() {
    assert_eq!(
        add_command("codex", Path::new("/opt/cccc")).expect("command"),
        ["codex", "mcp", "add", "cccc", "--", "/opt/cccc", "mcp"]
    );
}

#[test]
fn manual_runtime_has_explicit_batch_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let args = SetupArgs {
        runtime: None,
        path: ".".into(),
    };
    let value = setup_one(
        &home,
        &args,
        "custom",
        Path::new("/opt/cccc"),
        &json!({"mcpServers":{}}),
    )
    .expect("manual setup");
    assert_eq!(value["status"], "requires_action");
    assert_eq!(value["mode"], "manual");
}

#[test]
fn builds_noninteractive_cline_command_with_compiled_binary() {
    assert_eq!(
        add_command("cline", Path::new("/opt/cccc")).expect("command"),
        [
            "cline",
            "mcp",
            "add",
            "cccc",
            "--yes",
            "--",
            "/opt/cccc",
            "mcp"
        ]
    );
}
