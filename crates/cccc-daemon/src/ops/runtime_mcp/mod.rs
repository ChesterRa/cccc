mod process;
mod state;

use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
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
    // Managed sessions receive their actor-scoped MCP entry later in the launch
    // pipeline. Do not mutate a provider-global MCP registry for these runtimes.
    if matches!(
        runtime,
        ActorRuntime::Claude
            | ActorRuntime::Codex
            | ActorRuntime::Grok
            | ActorRuntime::Opencode
            | ActorRuntime::Kilo
    ) {
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
        ActorRuntime::Claude
        | ActorRuntime::Codex
        | ActorRuntime::Grok
        | ActorRuntime::Kilo
        | ActorRuntime::Opencode => {
            unreachable!("managed runtime returned early")
        }
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
    if runtime == ActorRuntime::Kimi {
        return cccc_core::runtime_mcp::ensure_kimi(cwd, env, executable)
            .map(|_| ())
            .map_err(|error| OpError::new("runtime_mcp_setup_failed", error.to_string()));
    }
    let expected = cccc_core::runtime_mcp::expected_command(executable);
    let report = inspect(runtime, cwd, env, &expected)?;
    if report.state == State::Ready {
        return Ok(());
    }
    if matches!(runtime, ActorRuntime::Copilot | ActorRuntime::Kiro)
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
        ActorRuntime::Copilot => inspect_cli(
            runtime,
            &["copilot", "mcp", "get", "cccc", "--json"],
            cwd,
            env,
            expected,
        ),
        ActorRuntime::Devin => inspect_devin(cwd, env, expected),
        _ => Ok(state::json_state(runtime, cwd, env, expected)),
    }
}

fn inspect_devin(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    expected: &[String],
) -> Result<Report, OpError> {
    let inspected = inspect_cli(
        ActorRuntime::Devin,
        &["devin", "mcp", "get", "cccc"],
        cwd,
        env,
        expected,
    )?;
    if inspected.state == State::Ready {
        return Ok(inspected);
    }
    let listed = inspect_cli(
        ActorRuntime::Devin,
        &["devin", "mcp", "list"],
        cwd,
        env,
        expected,
    )?;
    if listed.state == State::Ready {
        Ok(listed)
    } else {
        Ok(inspected)
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

#[cfg(test)]
mod tests {
    use super::*;

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
