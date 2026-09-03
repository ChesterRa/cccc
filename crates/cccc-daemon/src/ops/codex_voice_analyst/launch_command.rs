use super::required_value;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreparedLaunch {
    pub(super) app_server: Vec<String>,
    pub(super) remote_tui_prefix: Vec<String>,
    pub(super) model: String,
}

pub(super) fn prepare(
    configured: &[String],
    environment: &BTreeMap<String, String>,
) -> io::Result<PreparedLaunch> {
    let default_command = ["codex".to_owned()];
    let configured = if configured.is_empty() {
        &default_command[..]
    } else {
        configured
    };
    let executable = resolve_runtime_executable(&configured[0], environment)?;
    let (arguments, has_web_search) = codex_global_arguments(&configured[1..])?;
    let model = model_from_arguments(&arguments);
    let mut remote_tui_prefix = vec![executable.to_string_lossy().into_owned()];
    remote_tui_prefix.extend(arguments);
    if !has_web_search {
        remote_tui_prefix.extend(["-c".into(), "web_search=\"live\"".into()]);
    }
    remote_tui_prefix.extend([
        "--dangerously-bypass-approvals-and-sandbox".into(),
        "-c".into(),
        "shell_environment_policy.inherit=all".into(),
        "-c".into(),
        "approval_policy=\"never\"".into(),
        "-c".into(),
        "sandbox_mode=\"danger-full-access\"".into(),
    ]);
    let mut app_server = remote_tui_prefix.clone();
    app_server.extend([
        "app-server".into(),
        "--listen".into(),
        "ws://127.0.0.1:0".into(),
    ]);
    Ok(PreparedLaunch {
        app_server,
        remote_tui_prefix,
        model,
    })
}

fn model_from_arguments(arguments: &[String]) -> String {
    arguments
        .windows(2)
        .find(|items| matches!(items[0].as_str(), "-m" | "--model"))
        .map(|items| items[1].clone())
        .or_else(|| {
            arguments
                .iter()
                .find_map(|item| item.strip_prefix("--model=").map(str::to_owned))
        })
        .unwrap_or_default()
}

pub(super) fn resolve_runtime_executable(
    value: &str,
    environment: &BTreeMap<String, String>,
) -> io::Result<PathBuf> {
    let value = required_value(value, "runtime executable")?;
    let explicit = Path::new(value).is_absolute()
        || value == "~"
        || value.starts_with("~/")
        || value.starts_with("~\\")
        || value.contains('/')
        || value.contains('\\');
    let path = if explicit {
        cccc_core::path_input::expand_user_path(value)?
    } else {
        cccc_runtime::resolve_executable_in_path(value, environment.get("PATH").map(String::as_str))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("runtime executable is not installed or not in PATH: {value}"),
                )
            })?
    };
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("runtime executable does not exist: {}", path.display()),
        ));
    }
    Ok(path)
}

fn codex_global_arguments(arguments: &[String]) -> io::Result<(Vec<String>, bool)> {
    let mut result = Vec::new();
    let mut has_web_search = false;
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        match argument {
            "--dangerously-bypass-approvals-and-sandbox" | "--yolo" => index += 1,
            "--ask-for-approval" | "-a" | "--sandbox" | "-s" => {
                require_following_argument(arguments, index, argument)?;
                index += 2;
            }
            "--config" | "-c" => {
                let value = require_following_argument(arguments, index, argument)?;
                if !host_owned_codex_config(value) {
                    has_web_search |= config_key(value) == "web_search";
                    result.extend([argument.to_owned(), value.to_owned()]);
                }
                index += 2;
            }
            "--profile" | "-p" | "--model" | "-m" | "--enable" | "--disable"
            | "--local-provider" => {
                let value = require_following_argument(arguments, index, argument)?;
                result.extend([argument.to_owned(), value.to_owned()]);
                index += 2;
            }
            "--search" => {
                has_web_search = true;
                result.push(argument.to_owned());
                index += 1;
            }
            "--oss" | "--strict-config" | "--dangerously-bypass-hook-trust" => {
                result.push(argument.to_owned());
                index += 1;
            }
            _ if argument.starts_with("--config=") => {
                let value = argument.trim_start_matches("--config=");
                if value.is_empty() {
                    return invalid_command("--config requires key=value");
                }
                if !host_owned_codex_config(value) {
                    has_web_search |= config_key(value) == "web_search";
                    result.push(argument.to_owned());
                }
                index += 1;
            }
            _ if value_flag_with_equals(argument) => {
                result.push(argument.to_owned());
                index += 1;
            }
            _ if argument.starts_with("--ask-for-approval=")
                || argument.starts_with("--sandbox=") =>
            {
                index += 1;
            }
            _ => {
                return invalid_command(format!(
                    "Codex runtime command contains an unsupported app-server argument: {argument}"
                ));
            }
        }
    }
    Ok((result, has_web_search))
}

fn value_flag_with_equals(argument: &str) -> bool {
    [
        "--profile=",
        "--model=",
        "--enable=",
        "--disable=",
        "--local-provider=",
    ]
    .iter()
    .any(|prefix| argument.starts_with(prefix) && argument.len() > prefix.len())
}

fn require_following_argument<'a>(
    arguments: &'a [String],
    index: usize,
    flag: &str,
) -> io::Result<&'a str> {
    arguments
        .get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Codex runtime command flag {flag} requires a value"),
            )
        })
}

fn config_key(value: &str) -> &str {
    value.split_once('=').map_or(value, |(key, _)| key).trim()
}

fn host_owned_codex_config(value: &str) -> bool {
    let key = config_key(value);
    matches!(key, "approval_policy" | "sandbox_mode" | "mcp_servers")
        || key == "shell_environment_policy"
        || key.starts_with("shell_environment_policy.")
        || key == "mcp_servers.cccc"
        || key.starts_with("mcp_servers.cccc.")
}

fn invalid_command<T>(message: impl Into<String>) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}
