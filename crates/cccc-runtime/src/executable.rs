mod windows;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn resolve_executable_in_path(command: &str, path_override: Option<&str>) -> Option<PathBuf> {
    resolve_executable(
        command,
        path_override,
        cfg!(windows),
        std::env::var("PATHEXT").ok().as_deref(),
    )
}

/// Resolve the actor executable before handing it to `portable-pty`.
/// Windows batch shims must be run through `cmd.exe`; `CreateProcessW` cannot
/// execute them directly.
#[must_use]
pub fn prepare_pty_command(command: &[String], env: &BTreeMap<String, String>) -> Vec<String> {
    prepare_pty_command_for(command, env, cfg!(windows))
}

fn prepare_pty_command_for(
    command: &[String],
    env: &BTreeMap<String, String>,
    windows: bool,
) -> Vec<String> {
    let resolved_command = resolve_command_executable_for(command, env, windows);
    let Some(program) = resolved_command.first() else {
        return Vec::new();
    };
    windows::wrap_resolved_command(
        &resolved_command,
        Path::new(program),
        windows,
        environment_value(env, "COMSPEC", windows),
    )
}

#[must_use]
pub fn resolve_command_executable(
    command: &[String],
    env: &BTreeMap<String, String>,
) -> Vec<String> {
    resolve_command_executable_for(command, env, cfg!(windows))
}

fn resolve_command_executable_for(
    command: &[String],
    env: &BTreeMap<String, String>,
    windows: bool,
) -> Vec<String> {
    let Some(program) = command.first() else {
        return Vec::new();
    };
    let mut resolved = command.to_vec();
    let path = environment_value(env, "PATH", windows);
    let pathext = environment_value(env, "PATHEXT", windows)
        .map(str::to_owned)
        .or_else(|| std::env::var("PATHEXT").ok());
    if let Some(path) = resolve_executable(program, path, windows, pathext.as_deref()) {
        resolved[0] = path.to_string_lossy().into_owned();
    }
    resolved
}

fn environment_value<'a>(
    env: &'a BTreeMap<String, String>,
    key: &str,
    case_insensitive: bool,
) -> Option<&'a str> {
    if case_insensitive {
        // CommandBuilder applies the sorted map in iteration order, so the
        // last case-insensitive duplicate is the effective Windows value.
        env.iter()
            .rev()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    } else {
        env.get(key).map(String::as_str)
    }
}

fn resolve_executable(
    command: &str,
    path_override: Option<&str>,
    windows: bool,
    pathext: Option<&str>,
) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(command);
    if candidate.components().count() > 1 {
        return windows::executable_candidates(&candidate, windows, pathext)
            .into_iter()
            .find(|path| is_executable_file_for(path, windows, pathext));
    }
    let path_value = path_override
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path_value).find_map(|directory| {
        windows::executable_candidates(&directory.join(command), windows, pathext)
            .into_iter()
            .find(|path| is_executable_file_for(path, windows, pathext))
    })
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
    is_executable_file_for(
        path,
        cfg!(windows),
        std::env::var("PATHEXT").ok().as_deref(),
    )
}

fn is_executable_file_for(path: &Path, windows: bool, pathext: Option<&str>) -> bool {
    if !path.is_file() {
        return false;
    }
    if windows {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{value}").to_ascii_uppercase());
        return extension.is_some_and(|value| windows::is_supported_extension(&value, pathext));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    true
}

#[cfg(test)]
mod tests;
