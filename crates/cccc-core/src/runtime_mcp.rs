use cccc_contracts::ActorRuntime;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

mod json_entry;
mod kimi;
pub use kimi::ensure as ensure_kimi;
mod cursor;
pub use cursor::ensure as ensure_cursor;
mod antigravity;
pub use antigravity::ensure as ensure_antigravity;
mod grok;
pub use grok::ensure as ensure_grok;

/// The variables that tell a CCCC MCP server its home and its member's identity.
pub const CONTEXT_VARIABLES: [&str; 3] = ["CCCC_HOME", "CCCC_GROUP_ID", "CCCC_ACTOR_ID"];

/// The value Auggie's `cccc` entry gives a context variable: Auggie expands it
/// to the variable's value, or to `$NAME` when the variable is unset.
#[must_use]
pub fn auggie_context_value(name: &str) -> String {
    format!("${{{name}:-${name}}}")
}

/// The `cccc` entry CCCC registers with Auggie (1.2.1). Auggie passes an MCP
/// server only HOME, LOGNAME, PATH, SHELL, TERM and USER from its own
/// environment, so the entry maps each context variable: a member's server
/// receives that member's values, and a session outside CCCC passes `$NAME`,
/// which CCCC reads as unset. Without `useShellInterpolation: false`, Auggie
/// joins the command and its arguments into one `/bin/sh -c` line, which
/// breaks an executable path with spaces.
#[must_use]
pub fn auggie_entry(executable: &Path) -> serde_json::Value {
    let env: serde_json::Map<String, serde_json::Value> = CONTEXT_VARIABLES
        .iter()
        .map(|name| ((*name).to_owned(), auggie_context_value(name).into()))
        .collect();
    serde_json::json!({
        "type": "stdio",
        "command": executable.to_string_lossy(),
        "args": ["mcp"],
        "env": env,
        "useShellInterpolation": false
    })
}

/// The directory whose project MCP files Cursor and Auggie read: the top of the
/// enclosing Git work tree, or the working directory outside one (Cursor CLI
/// 2026.11.11, Auggie 1.2.1). The runtimes see the resolved working directory,
/// so symbolic links are followed.
#[must_use]
pub fn workspace_root(cwd: &Path) -> PathBuf {
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    cwd.ancestors()
        .find(|directory| directory.join(".git").exists())
        .map_or_else(|| cwd.clone(), Path::to_path_buf)
}

#[must_use]
pub const fn is_auto_managed(runtime: ActorRuntime) -> bool {
    matches!(
        runtime,
        ActorRuntime::Amp
            | ActorRuntime::Antigravity
            | ActorRuntime::Auggie
            | ActorRuntime::Claude
            | ActorRuntime::Cline
            | ActorRuntime::Codex
            | ActorRuntime::Copilot
            | ActorRuntime::Cursor
            | ActorRuntime::Devin
            | ActorRuntime::Kiro
            | ActorRuntime::Droid
            | ActorRuntime::Grok
            | ActorRuntime::Hermes
            | ActorRuntime::Kimi
            | ActorRuntime::Kilo
            | ActorRuntime::Opencode
            | ActorRuntime::Pi
    )
}

#[must_use]
pub const fn name(runtime: ActorRuntime) -> &'static str {
    runtime.wire_name()
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
        ActorRuntime::Antigravity => common(&["agy", "mcp", "add", "cccc", "cccc", "mcp"]),
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
        // `mcp add` cannot turn off Auggie's shell interpolation; see [`auggie_entry`].
        ActorRuntime::Auggie => vec![
            "auggie".into(),
            "mcp".into(),
            "add-json".into(),
            "cccc".into(),
            auggie_entry(executable).to_string(),
        ],
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
    fn auggie_registration_writes_the_whole_entry() {
        let command = add_command(ActorRuntime::Auggie, Path::new("/opt/cccc")).expect("command");
        assert_eq!(command.len(), 5);
        assert_eq!(command[..4], ["auggie", "mcp", "add-json", "cccc"]);
        let entry: serde_json::Value = serde_json::from_str(&command[4]).expect("entry JSON");
        assert_eq!(
            entry,
            serde_json::json!({
                "type":"stdio","command":"/opt/cccc","args":["mcp"],
                "env":{
                    "CCCC_HOME":"${CCCC_HOME:-$CCCC_HOME}",
                    "CCCC_GROUP_ID":"${CCCC_GROUP_ID:-$CCCC_GROUP_ID}",
                    "CCCC_ACTOR_ID":"${CCCC_ACTOR_ID:-$CCCC_ACTOR_ID}"
                },
                "useShellInterpolation":false
            })
        );
    }

    #[test]
    fn auto_managed_runtime_catalog_matches_supported_contract() {
        let runtimes = [
            ActorRuntime::Antigravity,
            ActorRuntime::Claude,
            ActorRuntime::Cline,
            ActorRuntime::Codex,
            ActorRuntime::Copilot,
            ActorRuntime::Cursor,
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
            ActorRuntime::Pi,
        ];
        assert!(runtimes.into_iter().all(is_auto_managed));
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
