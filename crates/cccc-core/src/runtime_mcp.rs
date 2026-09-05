use cccc_contracts::ActorRuntime;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

mod kimi;
pub use kimi::ensure as ensure_kimi;

#[must_use]
pub const fn is_auto_managed(runtime: ActorRuntime) -> bool {
    matches!(
        runtime,
        ActorRuntime::Amp
            | ActorRuntime::Auggie
            | ActorRuntime::Claude
            | ActorRuntime::Cline
            | ActorRuntime::Codex
            | ActorRuntime::Copilot
            | ActorRuntime::Devin
            | ActorRuntime::Kiro
            | ActorRuntime::Droid
            | ActorRuntime::Grok
            | ActorRuntime::Hermes
            | ActorRuntime::Kimi
            | ActorRuntime::Kilo
            | ActorRuntime::Opencode
    )
}

#[must_use]
pub const fn name(runtime: ActorRuntime) -> &'static str {
    match runtime {
        ActorRuntime::Amp => "amp",
        ActorRuntime::Antigravity => "antigravity",
        ActorRuntime::Auggie => "auggie",
        ActorRuntime::Claude => "claude",
        ActorRuntime::Cline => "cline",
        ActorRuntime::Codex => "codex",
        ActorRuntime::Deepseek => "deepseek",
        ActorRuntime::Copilot => "copilot",
        ActorRuntime::Cursor => "cursor",
        ActorRuntime::Devin => "devin",
        ActorRuntime::Kiro => "kiro",
        ActorRuntime::Kilo => "kilo",
        ActorRuntime::Droid => "droid",
        ActorRuntime::Grok => "grok",
        ActorRuntime::Hermes => "hermes",
        ActorRuntime::Kimi => "kimi",
        ActorRuntime::Opencode => "opencode",
        ActorRuntime::WebModel => "web_model",
        ActorRuntime::Custom => "custom",
    }
}

#[must_use]
pub fn from_name(value: &str) -> Option<ActorRuntime> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
}

#[must_use]
pub fn expected_command(executable: &Path) -> Vec<String> {
    vec![executable.to_string_lossy().into_owned(), "mcp".into()]
}

#[must_use]
pub fn find_program(program: &str, search_path: Option<&OsStr>) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    find_program_in(program, search_path, &cwd)
}

#[must_use]
pub fn find_program_in(program: &str, search_path: Option<&OsStr>, cwd: &Path) -> Option<PathBuf> {
    let cwd = if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(cwd)
    };
    let requested = Path::new(program);
    if requested.is_absolute() {
        return executable_candidate(requested);
    }
    if requested.components().count() > 1 {
        return executable_candidate(&cwd.join(requested));
    }
    search_path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .find_map(|directory| {
            let directory = if directory.is_absolute() {
                directory
            } else {
                cwd.join(directory)
            };
            executable_candidate(&directory.join(requested))
        })
}

#[must_use]
pub fn resolve_program(program: &str, search_path: Option<&OsStr>) -> PathBuf {
    find_program(program, search_path).unwrap_or_else(|| PathBuf::from(program))
}

#[must_use]
pub fn resolve_program_in(program: &str, search_path: Option<&OsStr>, cwd: &Path) -> PathBuf {
    find_program_in(program, search_path, cwd).unwrap_or_else(|| PathBuf::from(program))
}

fn executable_candidate(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    if path.extension().is_none() {
        for extension in ["com", "exe", "bat", "cmd"] {
            let candidate = path.with_extension(extension);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if !path.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if path.metadata().ok()?.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(path.to_path_buf())
}

#[must_use]
pub fn add_command(runtime: ActorRuntime, executable: &Path) -> Option<Vec<String>> {
    let cccc = executable.to_string_lossy().into_owned();
    let common = |parts: &[&str]| parts.iter().map(|part| (*part).to_owned()).collect();
    Some(match runtime {
        ActorRuntime::Cline => {
            common(&["cline", "mcp", "add", "cccc", "--yes", "--", &cccc, "mcp"])
        }
        ActorRuntime::Codex => common(&["codex", "mcp", "add", "cccc", "--", &cccc, "mcp"]),
        ActorRuntime::Copilot => common(&["copilot", "mcp", "add", "cccc", "--", &cccc, "mcp"]),
        ActorRuntime::Devin => common(&[
            "devin", "mcp", "add", "-s", "user", "cccc", "--", &cccc, "mcp",
        ]),
        ActorRuntime::Kiro => vec![
            "kiro-cli".into(),
            "mcp".into(),
            "add".into(),
            "--name".into(),
            "cccc".into(),
            "--scope".into(),
            "global".into(),
            "--command".into(),
            cccc,
            "--args=mcp".into(),
            "--force".into(),
        ],
        ActorRuntime::Droid => common(&[
            "droid", "mcp", "add", "--type", "stdio", "cccc", &cccc, "mcp",
        ]),
        ActorRuntime::Amp => common(&["amp", "mcp", "add", "cccc", &cccc, "mcp"]),
        ActorRuntime::Auggie => common(&["auggie", "mcp", "add", "cccc", "--", &cccc, "mcp"]),
        _ => return None,
    })
}

#[must_use]
pub fn remove_command(runtime: ActorRuntime) -> Option<Vec<String>> {
    let parts: &[&str] = match runtime {
        ActorRuntime::Codex => &["codex", "mcp", "remove", "cccc"],
        ActorRuntime::Copilot => &["copilot", "mcp", "remove", "cccc"],
        ActorRuntime::Devin => &["devin", "mcp", "remove", "-s", "user", "cccc"],
        ActorRuntime::Kiro => &[
            "kiro-cli", "mcp", "remove", "--name", "cccc", "--scope", "global",
        ],
        ActorRuntime::Droid => &["droid", "mcp", "remove", "cccc"],
        ActorRuntime::Amp => &["amp", "mcp", "remove", "cccc"],
        ActorRuntime::Auggie => &["auggie", "mcp", "remove", "cccc"],
        _ => return None,
    };
    Some(parts.iter().map(|part| (*part).to_owned()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_managed_runtime_catalog_matches_supported_contract() {
        let runtimes = [
            ActorRuntime::Claude,
            ActorRuntime::Cline,
            ActorRuntime::Codex,
            ActorRuntime::Copilot,
            ActorRuntime::Devin,
            ActorRuntime::Kiro,
            ActorRuntime::Droid,
            ActorRuntime::Amp,
            ActorRuntime::Auggie,
            ActorRuntime::Grok,
            ActorRuntime::Hermes,
            ActorRuntime::Kimi,
            ActorRuntime::Kilo,
            ActorRuntime::Opencode,
        ];
        assert!(runtimes.into_iter().all(is_auto_managed));
        assert!(!is_auto_managed(ActorRuntime::Cursor));
        assert!(!is_auto_managed(ActorRuntime::Custom));
    }

    #[test]
    fn resolves_a_runtime_command_from_an_explicit_search_path() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let filename = if cfg!(windows) {
            "runtime.cmd"
        } else {
            "runtime"
        };
        let executable = temp.path().join(filename);
        std::fs::write(&executable, b"runtime").expect("fixture");
        #[cfg(unix)]
        {
            let mut permissions = executable.metadata().expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&executable, permissions).expect("permissions");
        }
        let search_path = std::env::join_paths([temp.path()]).expect("search path");

        assert_eq!(
            find_program("runtime", Some(&search_path)),
            Some(executable)
        );
    }

    #[test]
    fn resolves_a_relative_search_path_from_the_child_working_directory() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let child_cwd = temp.path().join("actor");
        let relative_bin = child_cwd.join("bin");
        std::fs::create_dir_all(&relative_bin).expect("relative bin");
        let filename = if cfg!(windows) {
            "runtime.cmd"
        } else {
            "runtime"
        };
        let executable = relative_bin.join(filename);
        std::fs::write(&executable, b"runtime").expect("fixture");
        #[cfg(unix)]
        {
            let mut permissions = executable.metadata().expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&executable, permissions).expect("permissions");
        }
        let search_path = std::env::join_paths([Path::new("bin")]).expect("search path");

        assert_eq!(
            find_program_in("runtime", Some(&search_path), &child_cwd),
            Some(executable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn skips_a_non_executable_file_earlier_on_the_search_path() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let blocked_dir = temp.path().join("blocked");
        let executable_dir = temp.path().join("executable");
        std::fs::create_dir_all(&blocked_dir).expect("blocked directory");
        std::fs::create_dir_all(&executable_dir).expect("executable directory");
        std::fs::write(blocked_dir.join("runtime"), b"not executable").expect("blocked file");
        let executable = executable_dir.join("runtime");
        std::fs::write(&executable, b"#!/bin/sh\n").expect("executable file");
        let mut permissions = executable.metadata().expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("permissions");
        let search_path = std::env::join_paths([blocked_dir, executable_dir]).expect("search path");

        assert_eq!(
            find_program("runtime", Some(&search_path)),
            Some(executable)
        );
    }
}
