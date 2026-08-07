use cccc_core::HomeLayout;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const HOOK_TIMEOUT_SECONDS: u64 = 3;
// Keep this list aligned with Codex's documented hook contract. Failed tool
// commands are reported through PostToolUse; Codex does not currently expose
// separate PostToolUseFailure or StopFailure hook events.
const HOOK_EVENTS: [(&str, &str); 9] = [
    ("SessionStart", "session_start"),
    ("UserPromptSubmit", "user_prompt_submit"),
    ("PreToolUse", "pre_tool_use"),
    ("PermissionRequest", "permission_request"),
    ("PostToolUse", "post_tool_use"),
    ("SubagentStart", "subagent_start"),
    ("SubagentStop", "subagent_stop"),
    ("Stop", "stop"),
    ("SessionEnd", "session_end"),
];

pub fn configure(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> std::io::Result<super::runtime_hook_session::HookSetup> {
    configure_with_executable(
        home,
        group_id,
        actor_id,
        command,
        env,
        resolve_cccc_executable(),
    )
}

fn configure_with_executable(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
    executable: Option<PathBuf>,
) -> std::io::Result<super::runtime_hook_session::HookSetup> {
    let launch_token = begin_hook_launch(home, "codex", group_id, actor_id, env)?;
    if !is_direct_codex_command(command) {
        record_launch_issue(
            home,
            "codex",
            group_id,
            actor_id,
            &launch_token,
            "HookUnavailableCommand",
        )?;
        return Ok(setup("codex", launch_token, false));
    }
    let Some(executable) = executable else {
        record_launch_issue(
            home,
            "codex",
            group_id,
            actor_id,
            &launch_token,
            "HookUnavailableExecutable",
        )?;
        return Ok(setup("codex", launch_token, false));
    };
    configure_actor_cli_path(env, &executable);
    append_overrides(command, home.root(), &executable, group_id, actor_id);
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    Ok(setup("codex", launch_token, true))
}

fn setup(
    runtime: &str,
    launch_token: String,
    hook_enabled: bool,
) -> super::runtime_hook_session::HookSetup {
    super::runtime_hook_session::HookSetup {
        runtime: runtime.into(),
        launch_token,
        hook_enabled,
    }
}

fn is_direct_codex_command(command: &[String]) -> bool {
    const SUBCOMMANDS: [&str; 11] = [
        "app-server",
        "completion",
        "debug",
        "exec",
        "login",
        "logout",
        "mcp",
        "proto",
        "sandbox",
        "server",
        "status",
    ];
    let Some(program) = command.first() else {
        return false;
    };
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(name.as_str(), "codex" | "codex.exe") {
        return false;
    }
    command
        .iter()
        .skip(1)
        .map(|value| value.trim())
        .take_while(|value| *value != "--")
        .find(|value| !value.is_empty() && !value.starts_with('-'))
        .is_none_or(|value| !SUBCOMMANDS.contains(&value))
}

pub(crate) fn begin_hook_launch(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
    env: &mut BTreeMap<String, String>,
) -> std::io::Result<String> {
    let launch_token = uuid::Uuid::new_v4().simple().to_string();
    cccc_core::codex_hook_state::begin_launch(
        home,
        runtime,
        group_id,
        actor_id,
        &launch_token,
        "HookPending",
    )?;
    env.insert("CCCC_HOOK_LAUNCH_TOKEN".into(), launch_token.clone());
    Ok(launch_token)
}

pub(crate) fn record_launch_issue(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
    launch_token: &str,
    event: &str,
) -> std::io::Result<()> {
    cccc_core::codex_hook_state::begin_launch(
        home,
        runtime,
        group_id,
        actor_id,
        launch_token,
        event,
    )
    .map(|_| ())
}

pub(crate) fn configure_actor_cli(env: &mut BTreeMap<String, String>) -> Option<PathBuf> {
    let executable = resolve_cccc_executable()?;
    configure_actor_cli_path(env, &executable);
    Some(executable)
}

pub(crate) fn configure_mcp_only(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) {
    let Some(executable) = configure_actor_cli(env) else {
        return;
    };
    append_mcp_overrides(command, home.root(), &executable, group_id, actor_id);
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
}

fn configure_actor_cli_path(env: &mut BTreeMap<String, String>, executable: &Path) {
    prepend_executable_dir(env, executable);
    env.insert("CCCC_CLI".into(), executable.to_string_lossy().into_owned());
}

fn append_overrides(
    command: &mut Vec<String>,
    home: &Path,
    executable: &Path,
    group_id: &str,
    actor_id: &str,
) {
    append_mcp_overrides(command, home, executable, group_id, actor_id);
    append_hook_overrides(command, executable);
}

fn append_mcp_overrides(
    command: &mut Vec<String>,
    home: &Path,
    executable: &Path,
    group_id: &str,
    actor_id: &str,
) {
    let executable_toml = toml_string(executable);
    let home = toml_string(home);
    let group_id = serde_json::to_string(group_id).unwrap_or_else(|_| "\"\"".into());
    let actor_id = serde_json::to_string(actor_id).unwrap_or_else(|_| "\"\"".into());
    command.extend([
        "-c".into(),
        format!("mcp_servers.cccc.command={executable_toml}"),
        "-c".into(),
        "mcp_servers.cccc.args=[\"mcp\"]".into(),
        "-c".into(),
        format!("mcp_servers.cccc.env.CCCC_HOME={home}"),
        "-c".into(),
        format!("mcp_servers.cccc.env.CCCC_GROUP_ID={group_id}"),
        "-c".into(),
        format!("mcp_servers.cccc.env.CCCC_ACTOR_ID={actor_id}"),
    ]);
}

fn append_hook_overrides(command: &mut Vec<String>, executable: &Path) {
    let hook_command = hook_command(executable);
    let hook_command_toml = serde_json::to_string(&hook_command).unwrap_or_else(|_| "\"\"".into());
    for (event_name, _) in HOOK_EVENTS {
        command.extend([
            "-c".into(),
            format!(
                "hooks.{event_name}=[{{hooks=[{{type=\"command\",command={hook_command_toml},timeout={HOOK_TIMEOUT_SECONDS}}}]}}]"
            ),
        ]);
    }
    let state = HOOK_EVENTS
        .iter()
        .map(|(_, event_key)| {
            let key = format!("/<session-flags>/config.toml:{event_key}:0:0");
            let key = serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".into());
            let hash = hook_hash(event_key, &hook_command);
            format!("{key}={{trusted_hash=\"{hash}\"}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    command.extend(["-c".into(), format!("hooks.state={{{state}}}")]);
}

fn hook_command(executable: &Path) -> String {
    hook_command_for(executable, "codex-state")
}

pub(crate) fn hook_command_for(executable: &Path, action: &str) -> String {
    let path = executable.to_string_lossy();
    if cfg!(windows) {
        format!("\"{path}\" hook {action}")
    } else {
        format!("'{}' hook {action}", path.replace('\'', "'\"'\"'"))
    }
}

fn hook_hash(event_key: &str, command: &str) -> String {
    let mut identity = json!({
        "event_name": event_key,
        "hooks": [{
            "async": false,
            "command": command,
            "timeout": HOOK_TIMEOUT_SECONDS,
            "type": "command"
        }]
    });
    canonicalize(&mut identity);
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(&identity).unwrap_or_default());
    let hex = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

fn canonicalize(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for value in object.values_mut() {
                canonicalize(value);
            }
            let mut sorted = std::mem::take(object).into_iter().collect::<Vec<_>>();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            object.extend(sorted);
        }
        Value::Array(items) => items.iter_mut().for_each(canonicalize),
        _ => {}
    }
}

pub(crate) fn resolve_cccc_executable() -> Option<PathBuf> {
    if let Some(launcher) = std::env::var_os("CCCC_LAUNCHER_PATH")
        .map(PathBuf::from)
        .filter(|path| valid_public_launcher(path))
    {
        return Some(launcher);
    }
    let current = std::env::current_exe().ok()?;
    if executable_stem(&current) == "cccc" {
        return Some(current);
    }
    let sibling = current.with_file_name(executable_name());
    if sibling.is_file() {
        return Some(sibling);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(executable_name()))
            .find(|candidate| candidate.is_file())
    })
}

fn valid_public_launcher(path: &Path) -> bool {
    path.is_absolute() && path.is_file() && executable_stem(path) == "cccc"
}

fn prepend_executable_dir(env: &mut BTreeMap<String, String>, executable: &Path) {
    let Some(directory) = executable.parent() else {
        return;
    };
    let inherited = env
        .get("PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"));
    let mut paths = inherited
        .as_deref()
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|path| path != directory)
        .collect::<Vec<_>>();
    paths.insert(0, directory.to_path_buf());
    if let Ok(value) = std::env::join_paths(paths) {
        env.insert("PATH".into(), value.to_string_lossy().into_owned());
    }
}

fn toml_string(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).unwrap_or_else(|_| "\"\"".into())
}

const fn executable_name() -> &'static str {
    if cfg!(windows) { "cccc.exe" } else { "cccc" }
}

fn executable_stem(path: &Path) -> &str {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::{
        append_overrides, begin_hook_launch, configure_with_executable, hook_hash,
        is_direct_codex_command, prepend_executable_dir, record_launch_issue,
        valid_public_launcher,
    };
    use cccc_core::HomeLayout;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    #[test]
    fn appends_absolute_mcp_overrides() {
        let mut command = vec!["codex".into(), "--search".into()];
        append_overrides(
            &mut command,
            Path::new("/tmp/cccc home"),
            Path::new("/tmp/cccc bin/cccc"),
            "g_test",
            "backend",
        );
        assert!(command.contains(&"mcp_servers.cccc.command=\"/tmp/cccc bin/cccc\"".into()));
        assert!(command.contains(&"mcp_servers.cccc.args=[\"mcp\"]".into()));
        assert!(command.contains(&"mcp_servers.cccc.env.CCCC_HOME=\"/tmp/cccc home\"".into()));
        assert!(command.contains(&"mcp_servers.cccc.env.CCCC_GROUP_ID=\"g_test\"".into()));
        assert!(command.contains(&"mcp_servers.cccc.env.CCCC_ACTOR_ID=\"backend\"".into()));
        assert!(
            command
                .iter()
                .any(|item| item.starts_with("hooks.UserPromptSubmit="))
        );
        assert!(
            command
                .iter()
                .any(|item| item.starts_with("hooks.PermissionRequest="))
        );
        assert!(
            command
                .iter()
                .any(|item| item.starts_with("hooks.PostToolUse="))
        );
        assert!(command.iter().any(|item| item.starts_with("hooks.Stop=")));
        assert!(
            !command
                .iter()
                .any(|item| item.starts_with("hooks.PostToolUseFailure="))
        );
        assert!(
            !command
                .iter()
                .any(|item| item.starts_with("hooks.StopFailure="))
        );
        assert!(command.iter().any(|item| item.starts_with("hooks.state=")));
        assert!(!command.contains(&"--dangerously-bypass-hook-trust".into()));
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
    fn only_interactive_codex_commands_are_hook_eligible() {
        assert!(is_direct_codex_command(&[
            "codex".into(),
            "--search".into()
        ]));
        assert!(is_direct_codex_command(&[
            "codex".into(),
            "resume".into(),
            "session".into(),
        ]));
        assert!(!is_direct_codex_command(&[
            "codex".into(),
            "app-server".into(),
        ]));
        assert!(!is_direct_codex_command(&[
            "wrapper".into(),
            "codex".into()
        ]));
    }

    #[test]
    fn hook_hash_matches_codex_normalized_identity() {
        assert_eq!(
            hook_hash("user_prompt_submit", "/usr/bin/true"),
            "sha256:6990bafd84f554a7905347cfff30dc8ac278a24b17f343073271fc9737efd49f"
        );
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
    fn each_hook_launch_rotates_the_environment_fence_atomically() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut env = BTreeMap::new();
        let first =
            begin_hook_launch(&home, "codex", "g_test", "peer1", &mut env).expect("first launch");
        let first_state =
            cccc_core::codex_hook_state::read(&home, "g_test", "peer1").expect("first state");
        assert_eq!(env["CCCC_HOOK_LAUNCH_TOKEN"], first);
        assert_eq!(first_state.launch_token, first);
        assert!(first_state.awaiting_session_start);

        let second =
            begin_hook_launch(&home, "codex", "g_test", "peer1", &mut env).expect("second launch");
        let second_state =
            cccc_core::codex_hook_state::read(&home, "g_test", "peer1").expect("second state");
        assert_ne!(first, second);
        assert_eq!(env["CCCC_HOOK_LAUNCH_TOKEN"], second);
        assert_eq!(second_state.launch_token, second);

        let mut claude_env = BTreeMap::new();
        let claude = begin_hook_launch(&home, "claude", "g_test", "claude-peer", &mut claude_env)
            .expect("claude launch");
        let claude_state =
            cccc_core::codex_hook_state::read_runtime(&home, "claude", "g_test", "claude-peer")
                .expect("claude state");
        assert_eq!(claude_env["CCCC_HOOK_LAUNCH_TOKEN"], claude);
        assert_eq!(claude_state.launch_token, claude);
        assert_eq!(claude_state.observation, "pty_fail_closed");
    }

    #[test]
    fn missing_codex_executable_records_a_specific_setup_issue() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut command = vec!["codex".into()];
        let mut env = BTreeMap::new();
        let setup =
            configure_with_executable(&home, "g_test", "peer1", &mut command, &mut env, None)
                .expect("fail-closed setup");

        assert_eq!(command, ["codex"]);
        assert!(!setup.hook_enabled);
        assert!(!env["CCCC_HOOK_LAUNCH_TOKEN"].is_empty());
        let state =
            cccc_core::codex_hook_state::read(&home, "g_test", "peer1").expect("hook issue");
        assert_eq!(state.event, "HookUnavailableExecutable");
        assert!(state.awaiting_session_start);
    }

    #[test]
    fn setup_issue_write_failures_propagate_for_both_runtimes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        for runtime in ["codex", "claude"] {
            let actor_id = format!("{runtime}-peer");
            let mut env = BTreeMap::new();
            let token = begin_hook_launch(&home, runtime, "g_test", &actor_id, &mut env)
                .expect("initial pending");
            let state_dir = home.daemon_dir().join(format!("{runtime}-hook-state"));
            fs::remove_dir_all(&state_dir).expect("remove state directory");
            fs::write(&state_dir, "blocks directory recreation").expect("blocking file");

            assert!(
                record_launch_issue(
                    &home,
                    runtime,
                    "g_test",
                    &actor_id,
                    &token,
                    "HookUnavailableExecutable"
                )
                .is_err(),
                "{runtime} setup issue write must fail"
            );
        }
    }
}
