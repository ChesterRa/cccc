use super::*;

fn environment(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("KIMI_CODE_HOME".into(), String::new()),
        ("HOME".into(), root.to_string_lossy().into_owned()),
        ("USERPROFILE".into(), root.to_string_lossy().into_owned()),
        (
            "CCCC_HOME".into(),
            root.join("cccc").to_string_lossy().into_owned(),
        ),
    ])
}

#[test]
fn kimi_code_uses_its_explicit_home_and_preserves_other_servers() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let mut env = environment(root);
    env.insert("KIMI_CODE_HOME".into(), "private-kimi".into());
    env.insert(
        "KIMI_SHARE_DIR".into(),
        root.join("unused-python-kimi").display().to_string(),
    );
    let path = root.join("private-kimi/mcp.json");
    let original = json!({"meta":{"keep":true},"mcpServers":{
        "other":{"command":"other","args":["--flag"],"env":{"TOKEN":"private-other-token"}},
        "cccc":{"command":"stale","args":[],"env":{"CCCC_ACTOR_ID":"old-actor"}}
    }});
    crate::fs::write_json(&path, &original).expect("seed MCP config");
    let executable = root.join("bin with spaces/cccc");
    assert_eq!(ensure(root, &env, &executable).expect("ensure"), path);
    let updated: Value = crate::fs::read_json(&path).expect("updated config");
    assert_eq!(updated["meta"], original["meta"]);
    assert_eq!(
        updated["mcpServers"]["other"],
        original["mcpServers"]["other"]
    );
    assert_eq!(
        updated["mcpServers"]["cccc"],
        json!({
            "command":executable,"args":["mcp"],"env":{"CCCC_HOME":env["CCCC_HOME"]}
        })
    );
    let contents = std::fs::read(&path).expect("contents");
    ensure(root, &env, &executable).expect("repeat setup");
    assert_eq!(std::fs::read(&path).expect("contents"), contents);
    assert!(!root.join("unused-python-kimi").exists());
    assert!(!root.join(".kimi-code").exists());
}

#[test]
fn kimi_code_does_not_choose_a_config_based_on_existing_legacy_directories() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    std::fs::create_dir(root.join(".kimi")).expect("legacy directory");
    let env = environment(root);
    assert_eq!(
        ensure(root, &env, Path::new("/opt/cccc")).expect("ensure"),
        root.join(".kimi-code/mcp.json")
    );
    assert!(!root.join(".kimi/mcp.json").exists());
}

#[test]
fn kimi_code_checks_the_effective_project_entry_without_overwriting_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let mut env = environment(root);
    env.insert(
        "KIMI_CODE_HOME".into(),
        root.join("user").display().to_string(),
    );
    let path = root.join(".kimi-code/mcp.json");
    let executable = root.join("cccc");
    let entry = json!({"command":executable,"args":["mcp"]});
    crate::fs::write_json(&path, &json!({"mcpServers":{"cccc":entry}})).expect("project entry");
    assert_eq!(
        ensure(root, &env, &executable).expect("valid project entry"),
        path
    );
    for field in [
        json!({"command":"stale"}),
        json!({"enabled":false}),
        json!({"env":{"CCCC_HOME":"another-home"}}),
        json!({"env":{"CCCC_ACTOR_ID":"another-actor"}}),
        json!({"cwd":"another-project"}),
    ] {
        let mut conflict = entry.clone();
        conflict
            .as_object_mut()
            .expect("MCP entry object")
            .extend(field.as_object().expect("conflicting fields").clone());
        crate::fs::write_json(&path, &json!({"mcpServers":{"cccc":conflict}})).expect("conflict");
        let before = std::fs::read(&path).expect("project bytes");
        let error = ensure(root, &env, &executable).expect_err("project must win");
        assert!(
            error
                .to_string()
                .contains("overrides the user configuration"),
            "{error}"
        );
        assert_eq!(
            std::fs::read(&path).expect("unchanged project bytes"),
            before
        );
    }
}

#[test]
fn kimi_code_setup_at_the_user_home_can_repair_its_own_mcp_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let env = environment(root);
    let path = root.join(".kimi-code/mcp.json");
    crate::fs::write_json(&path, &json!({"mcpServers":{"cccc":{"command":"stale"}}}))
        .expect("stale user entry");
    assert_eq!(
        ensure(root, &env, Path::new("/opt/cccc")).expect("repair user-owned config"),
        path
    );
}

#[test]
fn kimi_code_setup_recognizes_its_user_file_through_a_parent_path_alias() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    std::fs::create_dir(root.join("config")).expect("alias parent");
    let mut env = environment(root);
    env.insert(
        "KIMI_CODE_HOME".into(),
        root.join("config/../.kimi-code").display().to_string(),
    );
    let path = root.join(".kimi-code/mcp.json");
    crate::fs::write_json(&path, &json!({"mcpServers":{"cccc":{"command":"stale"}}}))
        .expect("stale user entry");
    ensure(root, &env, Path::new("/opt/cccc")).expect("repair the same user-owned file");
    let updated: Value = crate::fs::read_json(&path).expect("updated user config");
    assert_eq!(updated["mcpServers"]["cccc"]["command"], "/opt/cccc");
}

#[cfg(unix)]
#[test]
fn kimi_code_setup_preserves_a_user_symlink_to_the_same_project_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let mut env = environment(root);
    env.insert(
        "KIMI_CODE_HOME".into(),
        root.join("user").display().to_string(),
    );
    let project = root.join(".kimi-code/mcp.json");
    let path = root.join("user/mcp.json");
    crate::fs::write_json(
        &project,
        &json!({"mcpServers":{"cccc":{"command":"stale"}}}),
    )
    .expect("stale user entry");
    std::fs::create_dir(root.join("user")).expect("user config directory");
    std::os::unix::fs::symlink(&project, &path).expect("user config alias");
    assert_eq!(
        ensure(root, &env, Path::new("/opt/cccc")).expect("repair through user alias"),
        path
    );
    assert!(
        std::fs::symlink_metadata(&path)
            .expect("user link")
            .is_symlink()
    );
    let updated: Value = crate::fs::read_json(&project).expect("updated target");
    assert_eq!(updated["mcpServers"]["cccc"]["command"], "/opt/cccc");
}

#[test]
fn kimi_code_project_override_wins_over_a_valid_user_entry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let mut env = environment(root);
    env.insert(
        "KIMI_CODE_HOME".into(),
        root.join("user").display().to_string(),
    );
    let executable = root.join("cccc");
    let user_path = ensure(root, &env, &executable).expect("user setup");
    let user_before = std::fs::read(&user_path).expect("user bytes");
    let project_path = root.join(".kimi-code/mcp.json");
    crate::fs::write_json(
        &project_path,
        &json!({"mcpServers":{"cccc":{"command":"stale","args":["mcp"]}}}),
    )
    .expect("project override");
    assert!(ensure(root, &env, &executable).is_err());
    assert_eq!(
        std::fs::read(user_path).expect("unchanged user bytes"),
        user_before
    );
}

#[test]
fn kimi_code_preserves_malformed_configuration_instead_of_claiming_success() {
    for project in [false, true] {
        for source in ["{", "[]", r#"{"mcpServers":null}"#] {
            let temp = tempfile::tempdir().expect("tempdir");
            let root = temp.path();
            let mut env = environment(root);
            env.insert(
                "KIMI_CODE_HOME".into(),
                root.join("user").display().to_string(),
            );
            let path = if project {
                root.join(".kimi-code/mcp.json")
            } else {
                root.join("user/mcp.json")
            };
            std::fs::create_dir_all(path.parent().expect("config parent"))
                .expect("config directory");
            std::fs::write(&path, source).expect("malformed config fixture");
            let error =
                ensure(root, &env, Path::new("/opt/cccc")).expect_err("invalid configuration");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(
                std::fs::read_to_string(path).expect("unchanged config"),
                source
            );
        }
    }
}

#[test]
fn kimi_code_has_no_legacy_mcp_cli_command() {
    assert!(
        crate::runtime_mcp::add_command(cccc_contracts::ActorRuntime::Kimi, Path::new("/opt/cccc"))
            .is_none()
    );
    assert!(crate::runtime_mcp::remove_command(cccc_contracts::ActorRuntime::Kimi).is_none());
}
