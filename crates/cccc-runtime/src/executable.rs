use std::path::{Path, PathBuf};

pub fn resolve_executable_in_path(command: &str, path_override: Option<&str>) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(command);
    if candidate.components().count() > 1 && is_executable_file(&candidate) {
        return Some(candidate);
    }
    let path_value = path_override
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path_value).find_map(|dir| {
        let path = dir.join(command);
        if is_executable_file(&path) {
            return Some(path);
        }
        #[cfg(windows)]
        for extension in ["exe", "cmd", "bat"] {
            let path = dir.join(format!("{command}.{extension}"));
            if is_executable_file(&path) {
                return Some(path);
            }
        }
        None
    })
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{value}").to_ascii_uppercase());
        let pathext = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
            .split(';')
            .map(|value| value.to_ascii_uppercase())
            .collect::<std::collections::HashSet<_>>();
        extension.is_some_and(|value| pathext.contains(&value))
    }
}
