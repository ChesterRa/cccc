use cccc_core::HomeLayout;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[path = "codex_hook_probe.rs"]
mod hook_probe;
#[path = "codex_mcp_launcher.rs"]
mod launcher;
use launcher::configure_actor_cli_path;
pub(crate) use launcher::{configure_actor_cli, resolve_cccc_executable};
#[path = "codex_mcp_overrides.rs"]
mod overrides;
use overrides::{append_hook_overrides, append_mcp_overrides};

struct CodexLaunch<'a> {
    home: &'a HomeLayout,
    group_id: &'a str,
    actor_id: &'a str,
    cwd: &'a Path,
}

pub fn configure(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> std::io::Result<super::runtime_hook_session::HookSetup> {
    configure_with_executable(
        CodexLaunch {
            home,
            group_id,
            actor_id,
            cwd,
        },
        command,
        env,
        resolve_cccc_executable(),
        hook_probe::supports_hooks,
    )
}

fn configure_with_executable<F>(
    launch: CodexLaunch<'_>,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
    executable: Option<PathBuf>,
    hook_probe: F,
) -> std::io::Result<super::runtime_hook_session::HookSetup>
where
    F: FnOnce(&[String], &Path, &Path, &BTreeMap<String, String>) -> bool,
{
    let launch_token =
        begin_hook_launch(launch.home, "codex", launch.group_id, launch.actor_id, env)?;
    if !is_direct_codex_command(command) {
        record_launch_issue(
            launch.home,
            "codex",
            launch.group_id,
            launch.actor_id,
            &launch_token,
            "HookUnavailableCommand",
        )?;
        return Ok(setup("codex", launch_token, false));
    }
    let Some(executable) = executable else {
        record_launch_issue(
            launch.home,
            "codex",
            launch.group_id,
            launch.actor_id,
            &launch_token,
            "HookUnavailableExecutable",
        )?;
        return Ok(setup("codex", launch_token, false));
    };
    configure_actor_cli_path(env, &executable);
    append_mcp_overrides(
        command,
        launch.home.root(),
        &executable,
        launch.group_id,
        launch.actor_id,
    );
    env.insert(
        "CCCC_HOME".into(),
        launch.home.root().to_string_lossy().into_owned(),
    );
    if !hook_probe(command, &executable, launch.cwd, env) {
        record_launch_issue(
            launch.home,
            "codex",
            launch.group_id,
            launch.actor_id,
            &launch_token,
            "HookUnavailableSettings",
        )?;
        return Ok(setup("codex", launch_token, false));
    }
    append_hook_overrides(command, &executable);
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

pub(crate) fn hook_command_for(executable: &Path, action: &str) -> String {
    hook_command_for_platform(executable, action, cfg!(windows))
}

fn hook_command_for_platform(executable: &Path, action: &str, windows: bool) -> String {
    let path = executable.to_string_lossy();
    if windows {
        let needs_quotes = path
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '&' | '|' | '<' | '>' | '^' | '(' | ')'));
        if needs_quotes {
            format!("\"{path}\" hook {action}")
        } else {
            format!("{path} hook {action}")
        }
    } else {
        format!("'{}' hook {action}", path.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
#[path = "codex_mcp_tests.rs"]
mod tests;
