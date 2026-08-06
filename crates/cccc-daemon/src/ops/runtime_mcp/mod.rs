mod process;
mod state;

use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::dispatch::OpError;
use state::{Report, State};

const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);

fn setup_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(super) fn prepare(
    home: &HomeLayout,
    runtime: ActorRuntime,
    cwd: &Path,
    env: &mut BTreeMap<String, String>,
) -> Result<(), OpError> {
    if !cccc_core::runtime_mcp::is_auto_managed(runtime) {
        return Ok(());
    }
    // Codex receives its actor-scoped MCP entry later in the launch pipeline.
    // Do not require a discoverable public launcher for custom Codex commands.
    if runtime == ActorRuntime::Codex {
        return Ok(());
    }
    let executable = super::codex_mcp::resolve_cccc_executable().ok_or_else(|| {
        OpError::new(
            "runtime_mcp_executable_missing",
            "cannot locate the active CCCC executable for runtime MCP setup",
        )
    })?;
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );

    match runtime {
        ActorRuntime::Codex => unreachable!("Codex returns before persistent MCP setup"),
        ActorRuntime::Opencode => inject_opencode(env, &executable),
        ActorRuntime::Hermes => {
            let _guard = setup_lock().lock().map_err(|_| {
                OpError::new(
                    "runtime_mcp_lock_failed",
                    "runtime MCP setup lock is poisoned",
                )
            })?;
            super::hermes_runtime::ensure_for_actor(home, cwd, env)
        }
        _ => {
            let _guard = setup_lock().lock().map_err(|_| {
                OpError::new(
                    "runtime_mcp_lock_failed",
                    "runtime MCP setup lock is poisoned",
                )
            })?;
            ensure_persistent(runtime, cwd, env, &executable)
        }
    }
}

fn ensure_persistent(
    runtime: ActorRuntime,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    executable: &Path,
) -> Result<(), OpError> {
    let expected = cccc_core::runtime_mcp::expected_command(executable);
    let report = inspect(runtime, cwd, env, &expected)?;
    if report.state == State::Ready {
        return Ok(());
    }
    if matches!(
        runtime,
        ActorRuntime::Claude | ActorRuntime::Copilot | ActorRuntime::Kiro
    ) && report.state == State::Stale
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
        ActorRuntime::Claude => inspect_cli(
            runtime,
            &["claude", "mcp", "get", "cccc"],
            cwd,
            env,
            expected,
        ),
        ActorRuntime::Copilot => inspect_cli(
            runtime,
            &["copilot", "mcp", "get", "cccc", "--json"],
            cwd,
            env,
            expected,
        ),
        ActorRuntime::Devin => inspect_cli(
            runtime,
            &["devin", "mcp", "get", "cccc"],
            cwd,
            env,
            expected,
        ),
        ActorRuntime::Grok => inspect_cli(
            runtime,
            &["grok", "mcp", "list", "--json"],
            cwd,
            env,
            expected,
        ),
        _ => Ok(state::json_state(runtime, cwd, env, expected)),
    }
}

fn inspect_cli(
    runtime: ActorRuntime,
    command: &[&str],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    expected: &[String],
) -> Result<Report, OpError> {
    let command = command
        .iter()
        .map(|part| (*part).to_owned())
        .collect::<Vec<_>>();
    match process::run(&command, cwd, env, CHECK_TIMEOUT) {
        Ok(output) if output.code == 0 => Ok(state::command_output_state(
            runtime,
            &output.stdout,
            expected,
        )),
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

fn inject_opencode(env: &mut BTreeMap<String, String>, executable: &Path) -> Result<(), OpError> {
    let mut document = match env.get("OPENCODE_CONFIG_CONTENT") {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str::<Value>(raw)
            .map_err(|error| OpError::new("runtime_mcp_config_invalid", error.to_string()))?
            .as_object()
            .cloned()
            .ok_or_else(|| {
                OpError::new(
                    "runtime_mcp_config_invalid",
                    "OPENCODE_CONFIG_CONTENT must be a JSON object",
                )
            })?,
        _ => Map::new(),
    };
    let mcp = document
        .entry("mcp")
        .or_insert_with(|| Value::Object(Map::new()));
    if !mcp.is_object() {
        *mcp = Value::Object(Map::new());
    }
    let environment = ["CCCC_HOME", "CCCC_GROUP_ID", "CCCC_ACTOR_ID"]
        .into_iter()
        .filter_map(|key| env.get(key).map(|value| (key.to_owned(), value.clone())))
        .collect::<BTreeMap<_, _>>();
    let mcp = mcp.as_object_mut().ok_or_else(|| {
        OpError::new(
            "runtime_mcp_config_invalid",
            "OpenCode mcp config must be an object",
        )
    })?;
    mcp.insert(
        "cccc".into(),
        json!({
            "type":"local",
            "command":cccc_core::runtime_mcp::expected_command(executable),
            "enabled":true,
            "environment":environment,
        }),
    );
    env.insert(
        "OPENCODE_CONFIG_CONTENT".into(),
        Value::Object(document).to_string(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_injection_preserves_unrelated_inline_config() {
        let mut env = BTreeMap::from([
            ("CCCC_HOME".into(), "/tmp/home".into()),
            ("CCCC_GROUP_ID".into(), "g_test".into()),
            ("CCCC_ACTOR_ID".into(), "peer1".into()),
            (
                "OPENCODE_CONFIG_CONTENT".into(),
                r#"{"theme":"dark","mcp":{"other":{"type":"remote"}}}"#.into(),
            ),
        ]);
        inject_opencode(&mut env, Path::new("/opt/cccc")).expect("inject");
        let document: Value = serde_json::from_str(&env["OPENCODE_CONFIG_CONTENT"]).expect("json");
        assert_eq!(document["theme"], "dark");
        assert_eq!(document["mcp"]["other"]["type"], "remote");
        assert_eq!(
            document["mcp"]["cccc"]["command"],
            json!(["/opt/cccc", "mcp"])
        );
        assert_eq!(
            document["mcp"]["cccc"]["environment"]["CCCC_ACTOR_ID"],
            "peer1"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stale_claude_user_entry_is_replaced_and_verified_before_launch() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).expect("bin");
        let claude = bin.join("claude");
        let state = temp.path().join("claude-mcp-state");
        std::fs::write(&state, "/missing/cccc").expect("state");
        std::fs::write(
            &claude,
            r#"#!/bin/sh
state=$CCCC_TEST_MCP_STATE
case "$1 $2 $3" in
  "mcp get cccc")
    command=
    IFS= read -r command < "$state" || :
    printf 'Transport: stdio\nCommand: %s\nArgs: mcp\nScope: User config\n' "$command"
    ;;
  "mcp remove cccc")
    : > "$state"
    ;;
  "mcp add -s")
    shift 6
    printf '%s' "$1" > "$state"
    ;;
  *) exit 2 ;;
esac
"#,
        )
        .expect("script");
        let mut permissions = std::fs::metadata(&claude).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&claude, permissions).expect("permissions");
        let env = BTreeMap::from([
            ("PATH".into(), bin.to_string_lossy().into_owned()),
            (
                "CCCC_TEST_MCP_STATE".into(),
                state.to_string_lossy().into_owned(),
            ),
        ]);
        ensure_persistent(
            ActorRuntime::Claude,
            temp.path(),
            &env,
            Path::new("/opt/cccc"),
        )
        .expect("repair");
        assert_eq!(std::fs::read_to_string(state).expect("state"), "/opt/cccc");
    }
}
