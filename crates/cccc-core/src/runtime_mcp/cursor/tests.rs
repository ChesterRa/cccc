use super::*;

fn environment(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".into(), root.to_string_lossy().into_owned()),
        ("USERPROFILE".into(), root.to_string_lossy().into_owned()),
    ])
}

fn placeholders() -> Value {
    json!({
        "CCCC_HOME":"${env:CCCC_HOME}",
        "CCCC_GROUP_ID":"${env:CCCC_GROUP_ID}",
        "CCCC_ACTOR_ID":"${env:CCCC_ACTOR_ID}"
    })
}

#[test]
fn the_user_entry_maps_each_context_variable_and_keeps_other_servers() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let path = root.join(".cursor/mcp.json");
    let original = json!({
        "meta":{"keep":true},
        "mcpServers":{"other":{"command":"other-server"}}
    });
    crate::fs::write_json(&path, &original).expect("user config");
    let project = root.join("project");
    std::fs::create_dir_all(&project).expect("project");

    let executable = Path::new("/opt/cccc");
    assert_eq!(
        ensure(&project, &environment(root), executable).expect("ensure"),
        path
    );
    let updated: Value = crate::fs::read_json(&path).expect("updated config");
    assert_eq!(updated["meta"], original["meta"]);
    assert_eq!(
        updated["mcpServers"]["other"],
        original["mcpServers"]["other"]
    );
    assert_eq!(
        updated["mcpServers"]["cccc"],
        json!({"command":"/opt/cccc","args":["mcp"],"env":placeholders()})
    );
    let contents = std::fs::read(&path).expect("contents");
    ensure(&project, &environment(root), executable).expect("idempotent");
    assert_eq!(std::fs::read(&path).expect("contents"), contents);
    assert!(!project.join(".cursor").exists());
}

#[test]
fn an_entry_without_the_mapping_or_with_fixed_values_is_replaced() {
    for stale in [
        // An entry a model wrote for the earlier prompt-assisted setup.
        json!({"command":"/opt/cccc","args":["mcp"]}),
        json!({"command":"/opt/cccc","args":["mcp"],"env":{"CCCC_HOME":"/home/user/.cccc"}}),
        json!({"command":"/opt/cccc","args":["mcp"],"env":{
            "CCCC_HOME":"${env:CCCC_HOME}","CCCC_GROUP_ID":"${env:CCCC_GROUP_ID}",
            "CCCC_ACTOR_ID":"fixed-actor"}}),
        json!({"command":"/opt/cccc","args":["mcp"],"env":{
            "CCCC_HOME":"${env:CCCC_HOME}","CCCC_GROUP_ID":"${env:CCCC_GROUP_ID}",
            "CCCC_ACTOR_ID":"${env:CCCC_ACTOR_ID}","CCCC_MCP_TOOL_PROFILE":"fixed"}}),
        json!({"command":"/old/cccc","args":["mcp"],"env":placeholders()}),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let path = root.join(".cursor/mcp.json");
        crate::fs::write_json(&path, &json!({"mcpServers":{"cccc":stale}})).expect("stale");
        ensure(root, &environment(root), Path::new("/opt/cccc")).expect("replace");
        let updated: Value = crate::fs::read_json(&path).expect("updated config");
        assert_eq!(updated["mcpServers"]["cccc"]["env"], placeholders());
        assert_eq!(updated["mcpServers"]["cccc"]["command"], "/opt/cccc");
    }
}

#[test]
fn a_project_entry_is_used_only_when_it_matches_and_is_never_rewritten() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let home = root.join("home");
    let project = root.join("project");
    let project_path = project.join(".cursor/mcp.json");
    let entry = json!({"command":"/opt/cccc","args":["mcp"],"env":placeholders()});
    crate::fs::write_json(&project_path, &json!({"mcpServers":{"cccc":entry}}))
        .expect("project entry");
    assert_eq!(
        ensure(&project, &environment(&home), Path::new("/opt/cccc")).expect("project entry"),
        project_path
    );
    assert!(!home.join(".cursor/mcp.json").exists());

    crate::fs::write_json(
        &project_path,
        &json!({"mcpServers":{"cccc":{"command":"/opt/cccc","args":["mcp"]}}}),
    )
    .expect("conflicting project entry");
    let before = std::fs::read(&project_path).expect("project bytes");
    let error = ensure(&project, &environment(&home), Path::new("/opt/cccc"))
        .expect_err("the project entry wins");
    assert!(
        error
            .to_string()
            .contains("Cursor project MCP entry `cccc`"),
        "{error}"
    );
    assert_eq!(std::fs::read(&project_path).expect("project bytes"), before);
    assert!(!home.join(".cursor/mcp.json").exists());
}

#[test]
fn the_project_file_is_the_one_cursor_reads_at_the_git_work_tree_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let repo = temp.path().join("repo");
    let cwd = repo.join("packages/app");
    std::fs::create_dir_all(repo.join(".git")).expect("git directory");
    std::fs::create_dir_all(&cwd).expect("working directory");
    let stale = json!({"mcpServers":{"cccc":{"command":"/old/cccc","args":["mcp"]}}});
    // Cursor does not read a nested file below the root, so it cannot block.
    crate::fs::write_json(&repo.join("packages/.cursor/mcp.json"), &stale).expect("nested");
    ensure(&cwd, &environment(&home), Path::new("/opt/cccc")).expect("nested file ignored");
    assert!(home.join(".cursor/mcp.json").is_file());

    // The root's entry overrides the user entry in Cursor, so a stale one is
    // reported and left as it is.
    let root_file = repo.join(".cursor/mcp.json");
    crate::fs::write_json(&root_file, &stale).expect("root entry");
    let before = std::fs::read(&root_file).expect("root bytes");
    let error =
        ensure(&cwd, &environment(&home), Path::new("/opt/cccc")).expect_err("the root entry wins");
    assert!(
        error
            .to_string()
            .contains("Cursor project MCP entry `cccc`"),
        "{error}"
    );
    assert_eq!(std::fs::read(&root_file).expect("root bytes"), before);
}

#[test]
fn malformed_configuration_is_reported_and_left_unchanged() {
    for project in [false, true] {
        for source in ["{", "[]", r#"{"mcpServers":null}"#] {
            let temp = tempfile::tempdir().expect("tempdir");
            let root = temp.path();
            let home = root.join("home");
            let path = if project {
                root.join(".cursor/mcp.json")
            } else {
                home.join(".cursor/mcp.json")
            };
            std::fs::create_dir_all(path.parent().expect("config parent"))
                .expect("config directory");
            std::fs::write(&path, source).expect("malformed config fixture");
            let error = ensure(root, &environment(&home), Path::new("/opt/cccc"))
                .expect_err("invalid configuration");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(
                std::fs::read_to_string(path).expect("unchanged config"),
                source
            );
        }
    }
}
