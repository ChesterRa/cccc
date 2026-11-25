mod process;
mod state;

use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::dispatch::OpError;
use state::{Report, State};

const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
const ANTIGRAVITY_SESSION_SETTINGS: &str = r#"{"showFeedbackSurvey":false}"#;

fn setup_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(super) fn prepare(
    home: &HomeLayout,
    runtime: ActorRuntime,
    launch_command: &[String],
    cwd: &Path,
    env: &mut BTreeMap<String, String>,
) -> Result<(), OpError> {
    if !cccc_core::runtime_mcp::is_auto_managed(runtime) {
        return Ok(());
    }
    // These adapters own MCP setup later in the launch pipeline. Grok shares
    // its native registry with the TUI; the others inject a session entry.
    if matches!(
        runtime,
        ActorRuntime::Claude
            | ActorRuntime::Codex
            | ActorRuntime::Copilot
            | ActorRuntime::Grok
            | ActorRuntime::Opencode
            | ActorRuntime::Kilo
            | ActorRuntime::Pi
            | ActorRuntime::Hermes
    ) {
        return Ok(());
    }
    let executable = resolve_cccc_executable()?;
    prepare_with_executable(home, runtime, launch_command, cwd, env, &executable)
}

fn resolve_cccc_executable() -> Result<PathBuf, OpError> {
    super::codex_mcp::resolve_cccc_executable().ok_or_else(|| {
        OpError::new(
            "runtime_mcp_executable_missing",
            "cannot locate the active CCCC executable for runtime MCP setup",
        )
    })
}

fn prepare_with_executable(
    home: &HomeLayout,
    runtime: ActorRuntime,
    launch_command: &[String],
    cwd: &Path,
    env: &mut BTreeMap<String, String>,
    executable: &Path,
) -> Result<(), OpError> {
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );

    if runtime == ActorRuntime::Antigravity {
        return ensure_antigravity(launch_command, cwd, env, executable);
    }

    match runtime {
        ActorRuntime::Claude
        | ActorRuntime::Codex
        | ActorRuntime::Copilot
        | ActorRuntime::Grok
        | ActorRuntime::Kilo
        | ActorRuntime::Pi
        | ActorRuntime::Opencode
        | ActorRuntime::Hermes => {
            unreachable!("runtime with separate launch integration returned early")
        }
        _ => {
            let _guard = setup_lock().lock().map_err(|_| {
                OpError::new(
                    "runtime_mcp_lock_failed",
                    "runtime MCP setup lock is poisoned",
                )
            })?;
            ensure_persistent(runtime, cwd, env, executable)
        }
    }
}

fn ensure_antigravity(
    launch_command: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    executable: &Path,
) -> Result<(), OpError> {
    let runtime = ActorRuntime::Antigravity;
    let mut command = cccc_core::runtime_mcp::add_command(runtime, executable)
        .expect("Antigravity native setup command");
    if let Some(program) = launch_command.first() {
        command[0].clone_from(program);
    }
    cccc_core::runtime_mcp::ensure_antigravity(cwd, env, || {
        run_checked(
            runtime,
            "add CCCC MCP entry",
            &command,
            cwd,
            env,
            SETUP_TIMEOUT,
        )
        .map_err(|error| io::Error::other(error.message))
    })
    .map(|_| ())
    .map_err(|error| OpError::new("runtime_mcp_setup_failed", error.to_string()))
}

fn is_direct_cli_for(program: &str, expected: &str, windows: bool) -> bool {
    let basename = if windows {
        program.rsplit(['/', '\\']).next().unwrap_or(program)
    } else {
        program.rsplit('/').next().unwrap_or(program)
    };
    if !windows {
        return basename == expected;
    }
    basename.eq_ignore_ascii_case(expected)
        || basename.rsplit_once('.').is_some_and(|(stem, extension)| {
            stem.eq_ignore_ascii_case(expected)
                && ["exe", "cmd", "bat"]
                    .iter()
                    .any(|shim| extension.eq_ignore_ascii_case(shim))
        })
}

fn options_end(command: &[String]) -> usize {
    command
        .iter()
        .skip(1)
        .position(|arg| arg == "--")
        .map_or(command.len(), |offset| offset + 1)
}

/// These options belong to native PTY launches. Managed adapters configure
/// their own invocation after parsing the saved command.
pub(super) fn inject_pty_arguments(
    runtime: ActorRuntime,
    command: &mut Vec<String>,
    env: &BTreeMap<String, String>,
) -> Result<(), OpError> {
    match runtime {
        ActorRuntime::Antigravity => inject_antigravity_settings(command),
        ActorRuntime::Copilot
            if command
                .first()
                .is_some_and(|program| is_direct_cli_for(program, "copilot", cfg!(windows))) =>
        {
            inject_copilot_mcp(command, env, &resolve_cccc_executable()?);
        }
        _ => {}
    }
    Ok(())
}

fn inject_copilot_mcp(
    command: &mut Vec<String>,
    env: &BTreeMap<String, String>,
    executable: &Path,
) {
    let context = cccc_core::runtime_mcp::CONTEXT_VARIABLES
        .into_iter()
        .filter_map(|key| {
            env.get(key)
                .map(|value| (key.to_owned(), Value::String(value.clone())))
        })
        .collect();
    let end = options_end(command);
    command.splice(end..end, copilot_mcp_args(executable, context));
}

/// Both Copilot launch surfaces use the CLI's process-local stdio server
/// format, with a caller-selected set of CCCC context variables.
pub(super) fn copilot_mcp_args(executable: &Path, env: Map<String, Value>) -> [String; 2] {
    let config = json!({"mcpServers":{"cccc":{
        "type":"local", "command":executable.to_string_lossy(),
        "args":["mcp"], "env":env, "tools":["*"]
    }}});
    ["--additional-mcp-config".into(), config.to_string()]
}

/// Pass session-only feedback-survey suppression on native `agy` PTY launches
/// without mutating `~/.gemini/antigravity-cli/settings.json`. AGY treats
/// repeated `--settings` flags as last-wins, so commands that already pass
/// `--settings` / `--settings=` or invoke a non-`agy` wrapper stay unchanged.
fn inject_antigravity_settings(command: &mut Vec<String>) {
    let Some(program) = command.first() else {
        return;
    };
    if !is_direct_cli_for(program, "agy", cfg!(windows)) {
        return;
    }
    let flag_end = options_end(command);
    let has_settings = command[1..flag_end]
        .iter()
        .any(|arg| arg == "--settings" || arg.starts_with("--settings="));
    if has_settings {
        return;
    }
    command.splice(
        flag_end..flag_end,
        [
            "--settings".to_owned(),
            ANTIGRAVITY_SESSION_SETTINGS.to_owned(),
        ],
    );
}

fn ensure_persistent(
    runtime: ActorRuntime,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    executable: &Path,
) -> Result<(), OpError> {
    if runtime == ActorRuntime::Kimi {
        return cccc_core::runtime_mcp::ensure_kimi(cwd, env, executable)
            .map(|_| ())
            .map_err(|error| OpError::new("runtime_mcp_setup_failed", error.to_string()));
    }
    if runtime == ActorRuntime::Cursor {
        return cccc_core::runtime_mcp::ensure_cursor(cwd, env, executable)
            .map(|_| ())
            .map_err(|error| OpError::new("runtime_mcp_setup_failed", error.to_string()));
    }
    let expected = cccc_core::runtime_mcp::expected_command(executable);
    let report = inspect(runtime, cwd, env, &expected)?;
    if report.state == State::Ready {
        return Ok(());
    }
    if matches!(runtime, ActorRuntime::Kiro | ActorRuntime::Auggie)
        && report.state == State::Stale
        && !report.source.is_empty()
        && !report.source.contains("user")
    {
        return Err(OpError::new(
            "runtime_mcp_scope_conflict",
            format!(
                "{} MCP server `cccc` is stale in {} scope; remove that entry before starting the actor",
                cccc_core::runtime_mcp::name(runtime),
                report.source
            ),
        ));
    }
    if report.state == State::Stale
        && let Some(command) = cccc_core::runtime_mcp::remove_command(runtime)
    {
        run_checked(
            runtime,
            "remove stale CCCC MCP entry",
            &command,
            cwd,
            env,
            SETUP_TIMEOUT,
        )?;
    }
    let command = cccc_core::runtime_mcp::add_command(runtime, executable).ok_or_else(|| {
        OpError::new(
            "runtime_mcp_setup_unsupported",
            format!(
                "{} does not expose an automatic MCP setup command",
                cccc_core::runtime_mcp::name(runtime)
            ),
        )
    })?;
    run_checked(
        runtime,
        "add CCCC MCP entry",
        &command,
        cwd,
        env,
        SETUP_TIMEOUT,
    )?;
    let verified = inspect(runtime, cwd, env, &expected)?;
    if verified.state != State::Ready {
        return Err(OpError::new(
            "runtime_mcp_verification_failed",
            format!(
                "{} MCP setup completed, but its CCCC entry did not match {} mcp",
                cccc_core::runtime_mcp::name(runtime),
                executable.display()
            ),
        ));
    }
    Ok(())
}

fn inspect(
    runtime: ActorRuntime,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    expected: &[String],
) -> Result<Report, OpError> {
    match runtime {
        ActorRuntime::Devin => inspect_devin(cwd, env, expected),
        _ => Ok(state::json_state(runtime, cwd, env, expected)),
    }
}

fn inspect_devin(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    expected: &[String],
) -> Result<Report, OpError> {
    let inspected = inspect_devin_cli(&["devin", "mcp", "get", "cccc"], cwd, env, expected)?;
    if inspected.state == State::Ready {
        return Ok(inspected);
    }
    let listed = inspect_devin_cli(&["devin", "mcp", "list"], cwd, env, expected)?;
    if listed.state == State::Ready {
        Ok(listed)
    } else {
        Ok(inspected)
    }
}

fn inspect_devin_cli(
    command: &[&str],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    expected: &[String],
) -> Result<Report, OpError> {
    let command = command
        .iter()
        .map(|part| (*part).to_owned())
        .collect::<Vec<_>>();
    let runtime = ActorRuntime::Devin;
    match process::run(&command, cwd, env, CHECK_TIMEOUT) {
        Ok(output) if output.code == 0 => Ok(state::devin_cli_state(&output.stdout, expected)),
        Ok(_) => Ok(Report::new(State::Missing)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(OpError::new(
            "runtime_mcp_cli_missing",
            format!(
                "{} CLI was not found while checking CCCC MCP setup",
                cccc_core::runtime_mcp::name(runtime)
            ),
        )),
        Err(error) => Err(OpError::new(
            "runtime_mcp_check_failed",
            format!(
                "{} MCP setup check failed: {error}",
                cccc_core::runtime_mcp::name(runtime)
            ),
        )),
    }
}

fn run_checked(
    runtime: ActorRuntime,
    step: &str,
    command: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<(), OpError> {
    let output = process::run(command, cwd, env, timeout).map_err(|error| {
        OpError::new(
            "runtime_mcp_setup_failed",
            format!(
                "failed to {step} for {}: {error}",
                cccc_core::runtime_mcp::name(runtime)
            ),
        )
    })?;
    if output.code == 0 {
        return Ok(());
    }
    let detail = if output.stderr.trim().is_empty() {
        output.stdout.trim()
    } else {
        output.stderr.trim()
    };
    Err(OpError::new(
        "runtime_mcp_setup_failed",
        format!(
            "failed to {step} for {} (exit {}): {}",
            cccc_core::runtime_mcp::name(runtime),
            output.code,
            if detail.is_empty() {
                "no output"
            } else {
                detail
            }
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn antigravity_uses_selected_cli_before_launch_and_rechecks_its_config() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let provider_home = temp.path().join("provider");
        let script = temp.path().join("selected-agy");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import sys,os,json,pathlib
assert sys.argv[1:]==['mcp','add','cccc','cccc','mcp']
assert os.environ['CCCC_GROUP_ID']=='group-fixture'
assert os.environ['CCCC_ACTOR_ID']=='actor-fixture'
root=pathlib.Path(os.environ['HOME']);path=root/'.gemini/config/mcp_config.json'
path.parent.mkdir(parents=True,exist_ok=True)
path.write_text(json.dumps({'mcpServers':{'cccc':{'command':'cccc','args':['mcp']}}}))
(root/'selected-invoked').touch()
"#,
        )
        .expect("CLI fixture");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("executable");
        let default_cli = temp.path().join("agy");
        std::fs::write(&default_cli, b"#!/bin/sh\nexit 99\n").expect("unused default CLI");
        std::fs::set_permissions(&default_cli, std::fs::Permissions::from_mode(0o700))
            .expect("default executable");
        let search_path = std::env::join_paths(std::iter::once(temp.path().to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").expect("test PATH")),
        ))
        .expect("fixture PATH");
        let env = BTreeMap::from([
            ("HOME".into(), provider_home.display().to_string()),
            ("USERPROFILE".into(), provider_home.display().to_string()),
            ("CCCC_GROUP_ID".into(), "group-fixture".into()),
            ("CCCC_ACTOR_ID".into(), "actor-fixture".into()),
            ("PATH".into(), search_path.to_string_lossy().into_owned()),
        ]);
        let command = [
            script.display().to_string(),
            "--dangerously-skip-permissions".into(),
        ];
        ensure_antigravity(&command, &cwd, &env, Path::new("/fixture/cccc")).expect("native setup");
        assert!(provider_home.join("selected-invoked").is_file());
        assert!(
            !provider_home
                .join(".gemini/antigravity-cli/settings.json")
                .exists(),
            "Antigravity setup must not create user settings.json"
        );
        std::fs::remove_file(&script).expect("remove fixture CLI");
        ensure_antigravity(&command, &cwd, &env, Path::new("/fixture/cccc"))
            .expect("ready entry needs no subprocess");
    }

    #[test]
    fn antigravity_injects_session_settings_for_default_persisted_and_double_dash_commands() {
        let mut default_cmd = cccc_runtime::default_command(ActorRuntime::Antigravity);
        inject_antigravity_settings(&mut default_cmd);
        assert_eq!(
            default_cmd,
            [
                "agy",
                "--dangerously-skip-permissions",
                "--settings",
                r#"{"showFeedbackSurvey":false}"#
            ]
        );

        let mut persisted = vec![
            "/home/user/.local/bin/agy".to_owned(),
            "--model".to_owned(),
            "gemini-4".to_owned(),
        ];
        inject_antigravity_settings(&mut persisted);
        assert_eq!(
            persisted,
            [
                "/home/user/.local/bin/agy",
                "--model",
                "gemini-4",
                "--settings",
                r#"{"showFeedbackSurvey":false}"#
            ]
        );

        let mut bare = vec!["agy".to_owned()];
        inject_antigravity_settings(&mut bare);
        assert_eq!(
            bare,
            ["agy", "--settings", r#"{"showFeedbackSurvey":false}"#]
        );

        let mut with_separator = vec![
            "agy".to_owned(),
            "--dangerously-skip-permissions".to_owned(),
            "--".to_owned(),
            "--settings".to_owned(),
        ];
        inject_antigravity_settings(&mut with_separator);
        assert_eq!(
            with_separator,
            [
                "agy",
                "--dangerously-skip-permissions",
                "--settings",
                r#"{"showFeedbackSurvey":false}"#,
                "--",
                "--settings"
            ]
        );
    }

    #[test]
    fn antigravity_skips_session_settings_when_user_already_passes_settings_flag() {
        for initial in [
            vec![
                "agy".to_owned(),
                "--dangerously-skip-permissions".to_owned(),
                "--settings".to_owned(),
                r#"{"showTips":false}"#.to_owned(),
            ],
            vec![
                "agy".to_owned(),
                r#"--settings={"showTips":false}"#.to_owned(),
                "--dangerously-skip-permissions".to_owned(),
            ],
            vec![
                "/opt/bin/agy".to_owned(),
                "--settings=/tmp/custom.json".to_owned(),
            ],
        ] {
            let mut command = initial.clone();
            inject_antigravity_settings(&mut command);
            assert_eq!(command, initial);
        }
    }

    #[test]
    fn antigravity_skips_session_settings_for_non_agy_executables_and_wrappers() {
        for initial in [
            vec![
                "python3".to_owned(),
                "-u".to_owned(),
                "-c".to_owned(),
                "import time; time.sleep(30)".to_owned(),
            ],
            vec!["/tmp/fixture/input.py".to_owned(), "/tmp/work".to_owned()],
            vec!["/usr/local/bin/agy-wrapper".to_owned()],
            vec!["/tmp/selected-agy".to_owned()],
            vec!["agy.sh".to_owned()],
            Vec::new(),
        ] {
            let mut command = initial.clone();
            inject_antigravity_settings(&mut command);
            assert_eq!(command, initial);
        }

        // Verify platform basename matching directly for Unix and Windows.
        assert!(is_direct_cli_for("agy", "agy", false));
        assert!(is_direct_cli_for("/usr/local/bin/agy", "agy", false));
        assert!(!is_direct_cli_for("agy.exe", "agy", false));
        assert!(!is_direct_cli_for("agy.sh", "agy", false));
        assert!(!is_direct_cli_for("selected-agy", "agy", false));

        assert!(is_direct_cli_for("agy", "agy", true));
        assert!(is_direct_cli_for(
            r"C:\Users\dev\.local\bin\AGY.EXE",
            "agy",
            true
        ));
        assert!(is_direct_cli_for(r"C:\bin\agy.cmd", "agy", true));
        assert!(is_direct_cli_for("agy.bat", "agy", true));
        assert!(!is_direct_cli_for(r"C:\bin\agy.ps1", "agy", true));
        assert!(!is_direct_cli_for(r"C:\bin\agy-wrapper.exe", "agy", true));
    }

    #[test]
    fn antigravity_prepares_mcp_then_adds_settings_only_to_the_pty_command() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let provider_home = temp.path().join("provider");
        let mcp_path = provider_home.join(".gemini/config/mcp_config.json");
        std::fs::create_dir_all(mcp_path.parent().expect("mcp dir")).expect("mcp dir");
        std::fs::write(
            &mcp_path,
            r#"{"mcpServers":{"cccc":{"command":"cccc","args":["mcp"],"disabled":false}}}"#,
        )
        .expect("ready mcp config");
        let mut env = BTreeMap::from([
            ("HOME".into(), provider_home.display().to_string()),
            ("USERPROFILE".into(), provider_home.display().to_string()),
        ]);

        let mut command = vec![
            "agy".to_owned(),
            "--dangerously-skip-permissions".to_owned(),
        ];
        prepare_with_executable(
            &home,
            ActorRuntime::Antigravity,
            &command,
            &cwd,
            &mut env,
            Path::new("/fixture/cccc"),
        )
        .expect("prepare antigravity");
        assert_eq!(command, ["agy", "--dangerously-skip-permissions"]);
        inject_pty_arguments(ActorRuntime::Antigravity, &mut command, &env)
            .expect("configure antigravity PTY");
        assert_eq!(
            command,
            [
                "agy",
                "--dangerously-skip-permissions",
                "--settings",
                r#"{"showFeedbackSurvey":false}"#
            ]
        );
        assert!(
            !provider_home
                .join(".gemini/antigravity-cli/settings.json")
                .exists(),
            "prepare must not create ~/.gemini/antigravity-cli/settings.json"
        );

        let mut with_user_settings = vec![
            "agy".to_owned(),
            "--settings".to_owned(),
            r#"{"showTips":false}"#.to_owned(),
        ];
        prepare_with_executable(
            &home,
            ActorRuntime::Antigravity,
            &with_user_settings,
            &cwd,
            &mut env,
            Path::new("/fixture/cccc"),
        )
        .expect("prepare antigravity with user --settings");
        inject_pty_arguments(ActorRuntime::Antigravity, &mut with_user_settings, &env)
            .expect("configure antigravity PTY with user --settings");
        assert_eq!(
            with_user_settings,
            ["agy", "--settings", r#"{"showTips":false}"#]
        );
    }

    #[test]
    fn copilot_pty_injects_only_its_launch_identity_and_leaves_user_config_untouched() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
        let settings = temp.path().join(".copilot/mcp-config.json");
        std::fs::create_dir_all(settings.parent().expect("settings dir")).expect("settings dir");
        let user_config = br#"{"mcpServers":{"other":{"type":"local","command":"other"}}}"#;
        std::fs::write(&settings, user_config).expect("user config");
        let mut env = BTreeMap::from([
            ("HOME".into(), temp.path().display().to_string()),
            ("USERPROFILE".into(), temp.path().display().to_string()),
            ("PATH".into(), temp.path().display().to_string()),
            ("CCCC_HOME".into(), home.root().display().to_string()),
            ("CCCC_GROUP_ID".into(), "g_fixture".into()),
            ("CCCC_ACTOR_ID".into(), "peer".into()),
            (
                "UNRELATED_FIXTURE_ENV".into(),
                "not a launch identity".into(),
            ),
        ]);
        let user_flag = r#"{"mcpServers":{"another":{"command":"another"}}}"#;
        let saved = [
            "copilot",
            "--allow-all",
            "--additional-mcp-config",
            user_flag,
            "--",
            "prompt text",
        ];
        let mut command = saved.map(str::to_owned).to_vec();
        prepare(
            &home,
            ActorRuntime::Copilot,
            &command,
            temp.path(),
            &mut env,
        )
        .expect("Copilot has no persistent setup step");
        assert_eq!(command, saved);
        inject_copilot_mcp(&mut command, &env, Path::new("/a path/cccc"));
        assert_eq!(
            command[..5],
            [
                "copilot",
                "--allow-all",
                "--additional-mcp-config",
                user_flag,
                "--additional-mcp-config",
            ]
        );
        assert_eq!(command[6..], ["--", "prompt text"]);
        let config: Value = serde_json::from_str(&command[5]).expect("MCP config");
        assert_eq!(
            config,
            json!({"mcpServers":{"cccc":{
                "type":"local", "command":"/a path/cccc", "args":["mcp"],
                "env":{"CCCC_HOME":home.root(),"CCCC_GROUP_ID":"g_fixture","CCCC_ACTOR_ID":"peer"},
                "tools":["*"]
            }}})
        );
        assert_eq!(std::fs::read(settings).expect("user config"), user_config);
    }

    #[test]
    fn copilot_pty_does_not_pass_native_options_to_wrappers() {
        let saved = ["script-wrapper", "copilot", "--allow-all"];
        let mut command = saved.map(str::to_owned).to_vec();
        inject_pty_arguments(ActorRuntime::Copilot, &mut command, &BTreeMap::new())
            .expect("wrapper needs no native options");
        assert_eq!(command, saved);
        assert!(is_direct_cli_for(r"C:\bin\COPILOT.CMD", "copilot", true));
    }

    #[test]
    fn kimi_actor_setup_uses_the_code_config_without_invoking_a_cli() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("project");
        let config = temp.path().join("kimi-code");
        let home = temp.path().join("cccc-home");
        let env = BTreeMap::from([
            ("KIMI_CODE_HOME".into(), config.display().to_string()),
            ("CCCC_HOME".into(), home.display().to_string()),
            ("PATH".into(), String::new()),
        ]);
        ensure_persistent(ActorRuntime::Kimi, &cwd, &env, Path::new("/opt/cccc"))
            .expect("setup without kimi mcp command");
        let config: serde_json::Value =
            cccc_core::fs::read_json(&config.join("mcp.json")).expect("config");
        assert_eq!(config["mcpServers"]["cccc"]["command"], "/opt/cccc");
        assert_eq!(
            config["mcpServers"]["cccc"]["env"]["CCCC_HOME"],
            home.display().to_string()
        );
    }

    #[cfg(unix)]
    #[test]
    fn auggie_actor_setup_replaces_an_old_user_entry_and_refuses_a_stale_project_entry() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let user_home = temp.path().join("user");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        // Auggie 1.2.1 behaviour for the two commands CCCC runs: `add-json`
        // refuses an existing name without --replace, `remove` deletes it.
        std::fs::write(
            bin.join("auggie"),
            r#"#!/usr/bin/env python3
import json, os, pathlib, sys
home = pathlib.Path(os.environ["HOME"])
with open(home / "calls.log", "a") as log:
    log.write(json.dumps(sys.argv[1:]) + "\n")
path = home / ".augment/settings.json"
document = json.loads(path.read_text()) if path.exists() else {}
servers = document.setdefault("mcpServers", {})
if sys.argv[1:3] == ["mcp", "remove"]:
    servers.pop(sys.argv[3], None)
elif sys.argv[1:3] == ["mcp", "add-json"]:
    if sys.argv[3] in servers and "-r" not in sys.argv:
        sys.exit("Error: Server '%s' already exists; use --replace to overwrite" % sys.argv[3])
    servers[sys.argv[3]] = json.loads(sys.argv[4])
else:
    sys.exit("unexpected command")
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(document))
"#,
        )
        .expect("CLI fixture");
        std::fs::set_permissions(bin.join("auggie"), std::fs::Permissions::from_mode(0o700))
            .expect("executable");
        let search_path = std::env::join_paths(std::iter::once(bin.clone()).chain(
            std::env::split_paths(&std::env::var_os("PATH").expect("test PATH")),
        ))
        .expect("fixture PATH");
        let env = BTreeMap::from([
            ("HOME".into(), user_home.display().to_string()),
            ("PATH".into(), search_path.to_string_lossy().into_owned()),
        ]);
        let settings = user_home.join(".augment/settings.json");
        std::fs::create_dir_all(settings.parent().expect("parent")).expect("directory");
        let old = serde_json::json!({"type":"stdio","command":"/opt/cccc","args":["mcp"]});
        let other = serde_json::json!({"type":"stdio","command":"/opt/other"});
        cccc_core::fs::write_json(
            &settings,
            &serde_json::json!({"mcpServers":{"cccc":old,"other":other}}),
        )
        .expect("old entry");

        ensure_persistent(ActorRuntime::Auggie, &cwd, &env, Path::new("/opt/cccc"))
            .expect("old entry replaced");
        let document: serde_json::Value = cccc_core::fs::read_json(&settings).expect("settings");
        assert_eq!(
            document["mcpServers"]["cccc"],
            cccc_core::runtime_mcp::auggie_entry(Path::new("/opt/cccc"))
        );
        assert_eq!(document["mcpServers"]["other"], other);
        let calls = std::fs::read_to_string(user_home.join("calls.log")).expect("calls");
        assert_eq!(calls.lines().count(), 2, "{calls}");
        assert!(calls.starts_with(r#"["mcp", "remove", "cccc"]"#), "{calls}");

        ensure_persistent(ActorRuntime::Auggie, &cwd, &env, Path::new("/opt/cccc"))
            .expect("ready entry needs no command");
        let project = cwd.join(".augment/settings.json");
        std::fs::create_dir_all(project.parent().expect("parent")).expect("directory");
        let project_document = serde_json::json!({"mcpServers":{"cccc":old}});
        cccc_core::fs::write_json(&project, &project_document).expect("project entry");
        let error = ensure_persistent(ActorRuntime::Auggie, &cwd, &env, Path::new("/opt/cccc"))
            .expect_err("a stale project entry would replace the user entry");
        assert_eq!(error.code, "runtime_mcp_scope_conflict");
        let project_after: serde_json::Value =
            cccc_core::fs::read_json(&project).expect("project file");
        assert_eq!(project_after, project_document);
        let calls = std::fs::read_to_string(user_home.join("calls.log")).expect("calls");
        assert_eq!(calls.lines().count(), 2, "{calls}");
    }

    #[test]
    fn cursor_actor_setup_writes_the_user_entry_before_launch_without_a_cli() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("project");
        let user_home = temp.path().join("user");
        let env = BTreeMap::from([
            ("HOME".into(), user_home.display().to_string()),
            ("USERPROFILE".into(), user_home.display().to_string()),
            (
                "CCCC_HOME".into(),
                temp.path().join("cccc-home").display().to_string(),
            ),
            ("PATH".into(), String::new()),
        ]);
        ensure_persistent(ActorRuntime::Cursor, &cwd, &env, Path::new("/opt/cccc"))
            .expect("setup without a cursor-agent command");
        let config: serde_json::Value =
            cccc_core::fs::read_json(&user_home.join(".cursor/mcp.json")).expect("config");
        assert_eq!(
            config["mcpServers"]["cccc"],
            serde_json::json!({
                "command":"/opt/cccc","args":["mcp"],
                "env":{
                    "CCCC_HOME":"${env:CCCC_HOME}",
                    "CCCC_GROUP_ID":"${env:CCCC_GROUP_ID}",
                    "CCCC_ACTOR_ID":"${env:CCCC_ACTOR_ID}"
                }
            })
        );
    }

    #[test]
    fn kimi_actor_setup_inherits_the_daemon_environment() {
        const CANARY: &str = "CCCC_KIMI_SETUP_CANARY";
        if let Some(root) = std::env::var_os(CANARY) {
            let root = std::path::PathBuf::from(root);
            // No HOME/KIMI_CODE_HOME override on the Actor itself.
            let overrides =
                BTreeMap::from([("CCCC_HOME".into(), root.join("cccc").display().to_string())]);
            ensure_persistent(
                ActorRuntime::Kimi,
                &root.join("project"),
                &overrides,
                Path::new("/opt/cccc"),
            )
            .expect("inherited Kimi Code home");
            assert!(root.join("kimi-code/mcp.json").is_file());
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "ops::runtime_mcp::tests::kimi_actor_setup_inherits_the_daemon_environment",
                "--nocapture",
            ])
            .env(CANARY, temp.path())
            .env("KIMI_CODE_HOME", temp.path().join("kimi-code"))
            .output()
            .expect("isolated test process");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
