use cccc_core::HomeLayout;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const HOOK_TIMEOUT_SECONDS: u64 = 3;
const HOOK_EVENTS: [(&str, &str); 9] = [
    ("SessionStart", "session_start"),
    ("UserPromptSubmit", "user_prompt_submit"),
    ("PreToolUse", "pre_tool_use"),
    ("PermissionRequest", "permission_request"),
    ("PostToolUse", "post_tool_use"),
    ("SubagentStart", "subagent_start"),
    ("SubagentStop", "subagent_stop"),
    ("Stop", "stop"),
    ("SessionEnd", "session_end"),
];

pub fn configure(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) {
    cccc_core::codex_hook_state::remove(home, group_id, actor_id);
    let Some(executable) = configure_actor_cli(env) else {
        return;
    };
    append_overrides(command, home.root(), &executable, group_id, actor_id);
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
}

pub(crate) fn configure_actor_cli(env: &mut BTreeMap<String, String>) -> Option<PathBuf> {
    let executable = resolve_cccc_executable()?;
    prepend_executable_dir(env, &executable);
    env.insert("CCCC_CLI".into(), executable.to_string_lossy().into_owned());
    Some(executable)
}

fn append_overrides(
    command: &mut Vec<String>,
    home: &Path,
    executable: &Path,
    group_id: &str,
    actor_id: &str,
) {
    let executable_toml = toml_string(executable);
    let home = toml_string(home);
    let group_id = serde_json::to_string(group_id).unwrap_or_else(|_| "\"\"".into());
    let actor_id = serde_json::to_string(actor_id).unwrap_or_else(|_| "\"\"".into());
    command.extend([
        "-c".into(),
        format!("mcp_servers.cccc.command={executable_toml}"),
        "-c".into(),
        "mcp_servers.cccc.args=[\"mcp\"]".into(),
        "-c".into(),
        format!("mcp_servers.cccc.env.CCCC_HOME={home}"),
        "-c".into(),
        format!("mcp_servers.cccc.env.CCCC_GROUP_ID={group_id}"),
        "-c".into(),
        format!("mcp_servers.cccc.env.CCCC_ACTOR_ID={actor_id}"),
    ]);
    append_hook_overrides(command, executable);
}

fn append_hook_overrides(command: &mut Vec<String>, executable: &Path) {
    let hook_command = hook_command(executable);
    let hook_command_toml = serde_json::to_string(&hook_command).unwrap_or_else(|_| "\"\"".into());
    for (event_name, _) in HOOK_EVENTS {
        command.extend([
            "-c".into(),
            format!(
                "hooks.{event_name}=[{{hooks=[{{type=\"command\",command={hook_command_toml},timeout={HOOK_TIMEOUT_SECONDS}}}]}}]"
            ),
        ]);
    }
    let state = HOOK_EVENTS
        .iter()
        .map(|(_, event_key)| {
            let key = format!("/<session-flags>/config.toml:{event_key}:0:0");
            let key = serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".into());
            let hash = hook_hash(event_key, &hook_command);
            format!("{key}={{trusted_hash=\"{hash}\"}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    command.extend(["-c".into(), format!("hooks.state={{{state}}}")]);
}

fn hook_command(executable: &Path) -> String {
    let path = executable.to_string_lossy();
    if cfg!(windows) {
        format!("\"{path}\" hook codex-state")
    } else {
        format!("'{}' hook codex-state", path.replace('\'', "'\"'\"'"))
    }
}

fn hook_hash(event_key: &str, command: &str) -> String {
    let mut identity = json!({
        "event_name": event_key,
        "hooks": [{
            "async": false,
            "command": command,
            "timeout": HOOK_TIMEOUT_SECONDS,
            "type": "command"
        }]
    });
    canonicalize(&mut identity);
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(&identity).unwrap_or_default());
    let hex = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

fn canonicalize(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for value in object.values_mut() {
                canonicalize(value);
            }
            let mut sorted = std::mem::take(object).into_iter().collect::<Vec<_>>();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            object.extend(sorted);
        }
        Value::Array(items) => items.iter_mut().for_each(canonicalize),
        _ => {}
    }
}

pub(crate) fn resolve_cccc_executable() -> Option<PathBuf> {
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
    use super::{append_overrides, hook_hash, prepend_executable_dir};
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn appends_absolute_mcp_overrides() {
        let mut command = vec!["codex".into(), "--search".into()];
        append_overrides(
            &mut command,
            Path::new("/tmp/cccc home"),
            Path::new("/tmp/cccc bin/cccc"),
            "g_test",
            "backend",
        );
        assert!(command.contains(&"mcp_servers.cccc.command=\"/tmp/cccc bin/cccc\"".into()));
        assert!(command.contains(&"mcp_servers.cccc.args=[\"mcp\"]".into()));
        assert!(command.contains(&"mcp_servers.cccc.env.CCCC_HOME=\"/tmp/cccc home\"".into()));
        assert!(command.contains(&"mcp_servers.cccc.env.CCCC_GROUP_ID=\"g_test\"".into()));
        assert!(command.contains(&"mcp_servers.cccc.env.CCCC_ACTOR_ID=\"backend\"".into()));
        assert!(
            command
                .iter()
                .any(|item| item.starts_with("hooks.UserPromptSubmit="))
        );
        assert!(
            command
                .iter()
                .any(|item| item.starts_with("hooks.PermissionRequest="))
        );
        assert!(command.iter().any(|item| item.starts_with("hooks.state=")));
        assert!(!command.contains(&"--dangerously-bypass-hook-trust".into()));
    }

    #[test]
    fn hook_hash_matches_codex_normalized_identity() {
        assert_eq!(
            hook_hash("user_prompt_submit", "/usr/bin/true"),
            "sha256:6990bafd84f554a7905347cfff30dc8ac278a24b17f343073271fc9737efd49f"
        );
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
