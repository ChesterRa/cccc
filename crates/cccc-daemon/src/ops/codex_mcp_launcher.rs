use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(crate) fn configure_actor_cli(env: &mut BTreeMap<String, String>) -> Option<PathBuf> {
    let executable = resolve_cccc_executable()?;
    configure_actor_cli_path(env, &executable);
    Some(executable)
}

pub(super) fn configure_actor_cli_path(env: &mut BTreeMap<String, String>, executable: &Path) {
    prepend_executable_dir(env, executable);
    env.insert("CCCC_CLI".into(), executable.to_string_lossy().into_owned());
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
    let cwd = std::env::current_dir().ok();
    std::env::var_os("PATH").and_then(|paths| resolve_on_path(&paths, cwd.as_deref()))
}

pub(super) fn resolve_on_path(paths: &OsStr, cwd: Option<&Path>) -> Option<PathBuf> {
    std::env::split_paths(paths)
        .filter_map(|directory| {
            if directory.is_absolute() {
                Some(directory.join(executable_name()))
            } else {
                cwd.map(|cwd| cwd.join(directory).join(executable_name()))
            }
        })
        .find(|candidate| candidate.is_file())
}

pub(super) fn valid_public_launcher(path: &Path) -> bool {
    path.is_absolute() && path.is_file() && executable_stem(path) == "cccc"
}

pub(super) fn prepend_executable_dir(env: &mut BTreeMap<String, String>, executable: &Path) {
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

const fn executable_name() -> &'static str {
    if cfg!(windows) { "cccc.exe" } else { "cccc" }
}

fn executable_stem(path: &Path) -> &str {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
}
