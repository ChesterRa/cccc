use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const LAUNCHER_PATH_ENV: &str = "CCCC_LAUNCHER_PATH";

pub(super) fn report() -> Value {
    let current = env::var_os(LAUNCHER_PATH_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::current_exe().ok());
    inspect(
        current.as_deref(),
        env::var_os("PATH").as_deref(),
        env::var_os("PATHEXT").as_deref(),
    )
}

fn inspect(current: Option<&Path>, path: Option<&OsStr>, pathext: Option<&OsStr>) -> Value {
    let current = current.map(absolute_path);
    let commands = find_commands(path, pathext);
    let resolved = commands.first();
    let path_status = match (current.as_deref(), resolved) {
        (None, _) => "unknown",
        (Some(_), None) => "missing",
        (Some(current), Some(resolved)) if same_command(current, resolved) => "ok",
        (Some(_), Some(_)) => "conflict",
    };
    let conflicting_commands = commands
        .iter()
        .filter(|command| {
            current
                .as_deref()
                .is_none_or(|current| !same_command(current, command))
        })
        .map(|path| display_path(path))
        .collect::<Vec<_>>();
    json!({
        "current_executable":current.as_deref().map(display_path),
        "resolved_command":resolved.map(|path| display_path(path)),
        "command_candidates":commands.iter().map(|path| display_path(path)).collect::<Vec<_>>(),
        "conflicting_commands":conflicting_commands,
        "path_status":path_status,
        "path_conflict":path_status == "conflict",
    })
}

fn find_commands(path: Option<&OsStr>, pathext: Option<&OsStr>) -> Vec<PathBuf> {
    let names = command_names(pathext);
    let mut commands = Vec::new();
    let mut seen = HashSet::new();
    for directory in path.into_iter().flat_map(env::split_paths) {
        for name in &names {
            let candidate = absolute_path(&directory.join(name));
            let key = literal_key(&candidate);
            if is_executable(&candidate) && seen.insert(key) {
                commands.push(candidate);
            }
        }
    }
    commands
}

fn command_names(pathext: Option<&OsStr>) -> Vec<OsString> {
    if !cfg!(windows) {
        return vec![OsString::from("cccc")];
    }
    let mut names = vec![OsString::from("cccc")];
    let extensions = pathext
        .and_then(OsStr::to_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".COM;.EXE;.BAT;.CMD");
    for extension in extensions
        .split(';')
        .filter(|value| !value.trim().is_empty())
    {
        let extension = extension.trim();
        names.push(OsString::from(format!(
            "cccc{}{}",
            if extension.starts_with('.') { "" } else { "." },
            extension
        )));
    }
    if !names
        .iter()
        .any(|name| name.to_string_lossy().eq_ignore_ascii_case("cccc.ps1"))
    {
        names.push(OsString::from("cccc.ps1"));
    }
    names
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|directory| directory.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn literal_key(path: &Path) -> String {
    let value = display_path(path);
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn same_command(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| absolute_path(left));
    let right = fs::canonicalize(right).unwrap_or_else(|_| absolute_path(right));
    literal_key(&left) == literal_key(&right)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_command(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        fs::write(path, b"command").expect("command");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("permissions");
        }
    }

    #[test]
    fn reports_an_older_path_command_ahead_of_the_current_launcher() {
        let temp = tempfile::tempdir().expect("tempdir");
        let current = temp.path().join("current/cccc");
        let older = temp.path().join("older/cccc");
        write_command(&current);
        write_command(&older);
        let search_path = env::join_paths([
            older.parent().expect("older parent"),
            current.parent().expect("current parent"),
        ])
        .expect("PATH");

        let value = inspect(Some(&current), Some(&search_path), None);

        assert_eq!(value["current_executable"], display_path(&current));
        assert_eq!(value["resolved_command"], display_path(&older));
        assert_eq!(value["path_status"], "conflict");
        assert_eq!(value["path_conflict"], true);
        assert_eq!(value["conflicting_commands"], json!([display_path(&older)]));
    }

    #[test]
    fn keeps_non_active_duplicates_visible_without_reporting_a_conflict() {
        let temp = tempfile::tempdir().expect("tempdir");
        let current = temp.path().join("current/cccc");
        let older = temp.path().join("older/cccc");
        write_command(&current);
        write_command(&older);
        let search_path = env::join_paths([
            current.parent().expect("current parent"),
            older.parent().expect("older parent"),
            current.parent().expect("current parent"),
        ])
        .expect("PATH");

        let value = inspect(Some(&current), Some(&search_path), None);

        assert_eq!(value["path_status"], "ok");
        assert_eq!(value["path_conflict"], false);
        assert_eq!(
            value["command_candidates"],
            json!([display_path(&current), display_path(&older)])
        );
        assert_eq!(value["conflicting_commands"], json!([display_path(&older)]));
    }
}
