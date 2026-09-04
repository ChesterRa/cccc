use super::launcher::{prepend_executable_dir, resolve_on_path, valid_public_launcher};
use super::overrides::{append_global_user_mcp_overrides, append_mcp_overrides};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn actor_mcp_overrides_are_scoped_and_precede_app_server() {
    let mut command = vec![
        "codex".into(),
        "-m".into(),
        "glm-test".into(),
        "app-server".into(),
        "--listen".into(),
        "ws://127.0.0.1:0".into(),
    ];
    append_mcp_overrides(
        &mut command,
        Path::new("/tmp/cccc home"),
        Path::new("/tmp/cccc bin/cccc"),
        "g_test",
        "backend",
    );

    let app_server = command
        .iter()
        .position(|item| item == "app-server")
        .expect("app-server");
    let overrides = &command[..app_server];
    assert!(overrides.contains(&"mcp_servers.cccc.command=\"/tmp/cccc bin/cccc\"".into()));
    assert!(overrides.contains(&"mcp_servers.cccc.args=[\"mcp\"]".into()));
    assert!(overrides.contains(&"mcp_servers.cccc.env.CCCC_HOME=\"/tmp/cccc home\"".into()));
    assert!(overrides.contains(&"mcp_servers.cccc.env.CCCC_GROUP_ID=\"g_test\"".into()));
    assert!(overrides.contains(&"mcp_servers.cccc.env.CCCC_ACTOR_ID=\"backend\"".into()));
    assert_eq!(
        &command[app_server..],
        ["app-server", "--listen", "ws://127.0.0.1:0"]
    );
    assert!(!command.iter().any(|item| item.starts_with("hooks.")));
}

#[test]
fn actor_mcp_overrides_stay_before_a_prompt_tail() {
    let mut command = vec![
        "codex".into(),
        "--search".into(),
        "--".into(),
        "prompt".into(),
    ];
    append_mcp_overrides(
        &mut command,
        Path::new("/tmp/home"),
        Path::new("/tmp/cccc"),
        "g_test",
        "peer",
    );

    let separator = command
        .iter()
        .position(|item| item == "--")
        .expect("separator");
    assert_eq!(&command[separator..], ["--", "prompt"]);
    assert!(
        command[..separator]
            .iter()
            .any(|item| item.starts_with("mcp_servers.cccc.command="))
    );
}

#[test]
fn global_voice_mcp_has_user_authority_without_an_implicit_group() {
    let mut command = vec![
        "codex".into(),
        "-c".into(),
        "model=\"gpt-test\"".into(),
        "app-server".into(),
    ];
    append_global_user_mcp_overrides(
        &mut command,
        Path::new("/tmp/cccc home"),
        Path::new("/tmp/cccc bin/cccc"),
    );

    let app_server = command
        .iter()
        .position(|item| item == "app-server")
        .expect("app-server");
    let overrides = &command[..app_server];
    assert!(overrides.contains(&"mcp_servers.cccc.env.CCCC_GROUP_ID=\"\"".into()));
    assert!(overrides.contains(&"mcp_servers.cccc.env.CCCC_ACTOR_ID=\"user\"".into()));
    assert!(overrides.contains(&"mcp_servers.cccc.env.CCCC_MCP_TOOL_PROFILE=\"full\"".into()));
    assert!(command[app_server + 1..].is_empty());
}

#[test]
fn public_launcher_override_requires_an_absolute_cccc_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let launcher = temp
        .path()
        .join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
    let private = temp.path().join(if cfg!(windows) {
        "cccc-rust.exe"
    } else {
        "cccc-rust"
    });
    fs::write(&launcher, b"launcher").expect("write launcher");
    fs::write(&private, b"private").expect("write private");

    assert!(valid_public_launcher(&launcher));
    assert!(!valid_public_launcher(&private));
    assert!(!valid_public_launcher(Path::new("cccc")));
}

#[test]
fn prepends_binary_directory_without_duplicate() {
    let mut env = BTreeMap::from([("PATH".into(), "/usr/bin:/tmp/bin".into())]);
    prepend_executable_dir(&mut env, Path::new("/tmp/bin/cccc"));
    let paths = std::env::split_paths(env.get("PATH").expect("path")).collect::<Vec<_>>();
    assert_eq!(
        paths.first().map(std::path::PathBuf::as_path),
        Some(Path::new("/tmp/bin"))
    );
    assert_eq!(
        paths
            .iter()
            .filter(|path| *path == Path::new("/tmp/bin"))
            .count(),
        1
    );
}

#[test]
fn resolves_relative_path_entries_against_the_daemon_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("bin");
    let executable = bin.join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
    fs::write(&executable, b"launcher").expect("write launcher");
    let paths = std::env::join_paths([Path::new("bin")]).expect("relative PATH");

    assert_eq!(
        resolve_on_path(&paths, Some(temp.path())).as_deref(),
        Some(executable.as_path())
    );
}

#[test]
fn resolves_absolute_path_entries_without_a_daemon_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let executable = temp
        .path()
        .join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
    fs::write(&executable, b"launcher").expect("write launcher");
    let paths = std::env::join_paths([temp.path()]).expect("absolute PATH");

    assert_eq!(
        resolve_on_path(&paths, None).as_deref(),
        Some(executable.as_path())
    );
}
