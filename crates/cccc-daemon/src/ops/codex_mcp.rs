use cccc_core::HomeLayout;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn configure(home: &HomeLayout, command: &mut Vec<String>, env: &mut BTreeMap<String, String>) {
    let Some(executable) = resolve_cccc_executable() else {
        return;
    };
    append_overrides(command, home.root(), &executable);
    prepend_executable_dir(env, &executable);
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
}

fn append_overrides(command: &mut Vec<String>, home: &Path, executable: &Path) {
    let executable = toml_string(executable);
    let home = toml_string(home);
    command.extend([
        "-c".into(),
        format!("mcp_servers.cccc.command={executable}"),
        "-c".into(),
        "mcp_servers.cccc.args=[\"mcp\"]".into(),
        "-c".into(),
        format!("mcp_servers.cccc.env.CCCC_HOME={home}"),
    ]);
}

fn resolve_cccc_executable() -> Option<PathBuf> {
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
    use super::{append_overrides, prepend_executable_dir};
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn appends_absolute_mcp_overrides() {
        let mut command = vec!["codex".into(), "--search".into()];
        append_overrides(
            &mut command,
            Path::new("/tmp/cccc home"),
            Path::new("/tmp/cccc bin/cccc"),
        );
        assert!(command.contains(&"mcp_servers.cccc.command=\"/tmp/cccc bin/cccc\"".into()));
        assert!(command.contains(&"mcp_servers.cccc.args=[\"mcp\"]".into()));
        assert!(command.contains(&"mcp_servers.cccc.env.CCCC_HOME=\"/tmp/cccc home\"".into()));
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
}
