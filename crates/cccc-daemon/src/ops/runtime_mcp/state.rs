use cccc_contracts::ActorRuntime;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    Missing,
    Ready,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Report {
    pub state: State,
    pub source: String,
}

impl Report {
    pub(super) fn new(state: State) -> Self {
        Self {
            state,
            source: String::new(),
        }
    }
}

pub(super) fn json_state(
    runtime: ActorRuntime,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    expected: &[String],
) -> Report {
    match runtime {
        ActorRuntime::Cline => entry_at(
            &cline_settings(env),
            &["mcpServers", "cccc"],
            expected,
            EntryShape::Cline,
        ),
        ActorRuntime::Kiro => {
            let mut local = entry_at(
                &cwd.join(".kiro/settings/mcp.json"),
                &["mcpServers", "cccc"],
                expected,
                EntryShape::Common,
            );
            if local.state != State::Missing {
                local.source = "project".into();
                return local;
            }
            entry_at(
                &kiro_home(env).join("settings/mcp.json"),
                &["mcpServers", "cccc"],
                expected,
                EntryShape::Common,
            )
        }
        ActorRuntime::Droid => first_state(
            &[
                home_dir(env).join(".factory/mcp.json"),
                home_dir(env).join(".config/droid/mcp.json"),
                home_dir(env).join(".droid/mcp.json"),
            ],
            &["mcpServers", "cccc"],
            expected,
            EntryShape::Common,
        ),
        ActorRuntime::Amp => entry_at(
            &home_dir(env).join(".config/amp/settings.json"),
            &["amp.mcpServers", "cccc"],
            expected,
            EntryShape::Common,
        ),
        ActorRuntime::Auggie => entry_at(
            &home_dir(env).join(".augment/settings.json"),
            &["mcpServers", "cccc"],
            expected,
            EntryShape::Common,
        ),
        ActorRuntime::Kimi => entry_at(
            &kimi_home(env).join("mcp.json"),
            &["mcpServers", "cccc"],
            expected,
            EntryShape::Common,
        ),
        _ => Report::new(State::Missing),
    }
}

pub(super) fn command_output_state(
    runtime: ActorRuntime,
    output: &str,
    expected: &[String],
) -> Report {
    match runtime {
        ActorRuntime::Claude => {
            let entry = parse_key_values(output);
            if entry.is_empty() {
                return Report::new(State::Missing);
            }
            let transport_ok = matches!(
                entry
                    .get("transport")
                    .map(String::as_str)
                    .unwrap_or("stdio"),
                "" | "stdio" | "local"
            );
            let command = entry.get("command").map(String::as_str).unwrap_or_default();
            let args = entry
                .get("args")
                .map(|value| {
                    value
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let source = entry
                .get("scope")
                .or_else(|| entry.get("source"))
                .cloned()
                .unwrap_or_default()
                .to_ascii_lowercase();
            Report {
                state: if transport_ok && command_matches(command, &args, expected) {
                    State::Ready
                } else {
                    State::Stale
                },
                source,
            }
        }
        ActorRuntime::Copilot => {
            let Ok(document) = serde_json::from_str::<Value>(output) else {
                return Report::new(State::Missing);
            };
            let entry = document
                .get("cccc")
                .or_else(|| document.pointer("/mcpServers/cccc"));
            let Some(entry) = entry else {
                return Report::new(State::Missing);
            };
            let source = entry
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            let tools_ok = entry.get("tools").is_none_or(|tools| match tools {
                Value::String(value) => value.trim().is_empty() || value.trim() == "*",
                Value::Array(values) => values.iter().any(|value| value.as_str() == Some("*")),
                _ => false,
            });
            let state = if common_matches(entry, expected) && tools_ok && !has_context_env(entry) {
                State::Ready
            } else {
                State::Stale
            };
            Report { state, source }
        }
        ActorRuntime::Devin => {
            if !output.to_ascii_lowercase().contains("stdio") {
                return Report::new(State::Missing);
            }
            let command = debug_string(output, "command");
            let args = debug_args(output);
            Report::new(if command_matches(&command, &args, expected) {
                State::Ready
            } else {
                State::Stale
            })
        }
        ActorRuntime::Grok => {
            let Ok(entries) = serde_json::from_str::<Vec<Value>>(output) else {
                return Report::new(State::Missing);
            };
            let mut state = State::Missing;
            for entry in entries.iter().filter(|entry| entry["name"] == "cccc") {
                let env_ok = entry
                    .get("env")
                    .and_then(Value::as_object)
                    .and_then(|env| env.get("PYTHONUNBUFFERED"))
                    .and_then(Value::as_str)
                    == Some("1");
                if common_matches(entry, expected) && env_ok {
                    return Report::new(State::Ready);
                }
                state = State::Stale;
            }
            Report::new(state)
        }
        _ => Report::new(State::Missing),
    }
}

fn parse_key_values(output: &str) -> BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| line.trim().split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect()
}

fn first_state(paths: &[PathBuf], keys: &[&str], expected: &[String], shape: EntryShape) -> Report {
    let mut report = Report::new(State::Missing);
    for path in paths {
        let current = entry_at(path, keys, expected, shape);
        if current.state == State::Ready {
            return current;
        }
        if current.state == State::Stale {
            report = current;
        }
    }
    report
}

#[derive(Clone, Copy)]
enum EntryShape {
    Common,
    Cline,
}

fn entry_at(path: &Path, keys: &[&str], expected: &[String], shape: EntryShape) -> Report {
    let Ok(document) = cccc_core::fs::read_json::<Value>(path) else {
        return Report::new(State::Missing);
    };
    let entry = keys
        .iter()
        .try_fold(&document, |value, key| value.get(*key));
    let Some(entry) = entry else {
        return Report::new(State::Missing);
    };
    let ready = match shape {
        EntryShape::Common => common_matches(entry, expected),
        EntryShape::Cline => cline_matches(entry, expected),
    };
    Report::new(if ready { State::Ready } else { State::Stale })
}

fn cline_matches(entry: &Value, expected: &[String]) -> bool {
    if disabled(entry) {
        return false;
    }
    let Some(transport) = entry.get("transport") else {
        return common_matches(entry, expected);
    };
    transport
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("stdio")
        == "stdio"
        && command_and_args_match(transport, expected)
}

fn common_matches(entry: &Value, expected: &[String]) -> bool {
    if disabled(entry) || !transport_matches(entry) {
        return false;
    }
    command_and_args_match(entry, expected)
}

fn disabled(entry: &Value) -> bool {
    entry.get("disabled").and_then(Value::as_bool) == Some(true)
        || entry.get("enabled").and_then(Value::as_bool) == Some(false)
}

fn transport_matches(entry: &Value) -> bool {
    matches!(
        entry
            .get("transport")
            .or_else(|| entry.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("stdio"),
        "" | "stdio" | "local"
    )
}

fn command_and_args_match(entry: &Value, expected: &[String]) -> bool {
    let command = entry
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = string_values(entry.get("args"));
    command_matches(command, &args, expected)
}

fn command_matches(command: &str, args: &[String], expected: &[String]) -> bool {
    expected.first().is_some_and(|expected_command| {
        normalize_path(command) == normalize_path(expected_command)
            && args == expected.get(1..).unwrap_or_default()
    })
}

fn normalize_path(value: &str) -> String {
    let value = value.trim().trim_matches(['"', '\'']);
    if cfg!(windows) {
        value.replace('/', "\\").to_ascii_lowercase()
    } else {
        value.to_owned()
    }
}

fn string_values(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(value)) => value.split_whitespace().map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

fn has_context_env(entry: &Value) -> bool {
    const KEYS: [&str; 3] = ["CCCC_HOME", "CCCC_GROUP_ID", "CCCC_ACTOR_ID"];
    let Some(env) = entry.get("env").or_else(|| entry.get("environment")) else {
        return false;
    };
    match env {
        Value::Object(values) => KEYS.iter().any(|key| values.contains_key(*key)),
        _ => KEYS.iter().any(|key| env.to_string().contains(key)),
    }
}

fn debug_string(output: &str, field: &str) -> String {
    let pattern = format!(r#"\b{}:\s*\"((?:\\.|[^\"\\])*)\""#, regex::escape(field));
    regex::Regex::new(&pattern)
        .ok()
        .and_then(|pattern| pattern.captures(output))
        .and_then(|captures| captures.get(1))
        .map(|value| decode_debug(value.as_str()))
        .unwrap_or_default()
}

fn debug_args(output: &str) -> Vec<String> {
    let Some(body) = regex::Regex::new(r"(?s)\bargs:\s*\[(.*?)\]")
        .ok()
        .and_then(|pattern| pattern.captures(output))
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str())
    else {
        return Vec::new();
    };
    regex::Regex::new(r#"\"((?:\\.|[^\"\\])*)\""#)
        .ok()
        .map(|pattern| {
            pattern
                .captures_iter(body)
                .filter_map(|captures| captures.get(1))
                .map(|value| decode_debug(value.as_str()))
                .collect()
        })
        .unwrap_or_default()
}

fn decode_debug(value: &str) -> String {
    serde_json::from_str::<String>(&format!("\"{value}\""))
        .unwrap_or_else(|_| value.replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn home_dir(env: &BTreeMap<String, String>) -> PathBuf {
    env.get("HOME")
        .or_else(|| env.get("USERPROFILE"))
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cline_settings(env: &BTreeMap<String, String>) -> PathBuf {
    if let Some(path) = configured_path(env, "CLINE_MCP_SETTINGS_PATH") {
        return path;
    }
    if let Some(path) = configured_path(env, "CLINE_DATA_DIR") {
        return path.join("settings/cline_mcp_settings.json");
    }
    if let Some(path) = configured_path(env, "CLINE_DIR") {
        return path.join("data/settings/cline_mcp_settings.json");
    }
    home_dir(env).join(".cline/data/settings/cline_mcp_settings.json")
}

fn kiro_home(env: &BTreeMap<String, String>) -> PathBuf {
    configured_path(env, "KIRO_HOME").unwrap_or_else(|| home_dir(env).join(".kiro"))
}

fn kimi_home(env: &BTreeMap<String, String>) -> PathBuf {
    configured_path(env, "KIMI_SHARE_DIR").unwrap_or_else(|| home_dir(env).join(".kimi"))
}

fn configured_path(env: &BTreeMap<String, String>, key: &str) -> Option<PathBuf> {
    match env.get(key) {
        Some(value) => (!value.trim().is_empty()).then(|| PathBuf::from(value.trim())),
        None => std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn common_entry_requires_current_command_and_stdio() {
        let expected = vec!["/opt/cccc".into(), "mcp".into()];
        assert!(common_matches(
            &json!({"command":"/opt/cccc","args":["mcp"]}),
            &expected
        ));
        assert!(!common_matches(
            &json!({"command":"/usr/local/bin/cccc","args":["mcp"]}),
            &expected
        ));
        assert!(!common_matches(
            &json!({"command":"/opt/cccc","args":["mcp"],"disabled":true}),
            &expected
        ));
    }

    #[test]
    fn copilot_rejects_actor_identity_persisted_in_global_config() {
        let report = command_output_state(
            ActorRuntime::Copilot,
            r#"{"cccc":{"command":"/opt/cccc","args":["mcp"],"source":"user","env":{"CCCC_ACTOR_ID":"peer1"}}}"#,
            &["/opt/cccc".into(), "mcp".into()],
        );
        assert_eq!(report.state, State::Stale);
        assert_eq!(report.source, "user");
    }

    #[test]
    fn cline_accepts_nested_stdio_transport() {
        let entry = json!({
            "transport":{"type":"stdio","command":"/opt/cccc","args":["mcp"]}
        });
        assert!(cline_matches(&entry, &["/opt/cccc".into(), "mcp".into()]));
    }

    #[test]
    fn json_backed_runtimes_share_python_compatible_state_detection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path();
        let expected = vec!["/opt/cccc".into(), "mcp".into()];
        let env = BTreeMap::from([
            ("HOME".into(), home.to_string_lossy().into_owned()),
            (
                "CLINE_MCP_SETTINGS_PATH".into(),
                home.join("cline.json").to_string_lossy().into_owned(),
            ),
            (
                "KIRO_HOME".into(),
                home.join("kiro").to_string_lossy().into_owned(),
            ),
            (
                "KIMI_SHARE_DIR".into(),
                home.join("kimi").to_string_lossy().into_owned(),
            ),
        ]);
        let fixtures = [
            (ActorRuntime::Cline, home.join("cline.json"), true),
            (
                ActorRuntime::Kiro,
                home.join("kiro/settings/mcp.json"),
                false,
            ),
            (ActorRuntime::Droid, home.join(".factory/mcp.json"), false),
            (
                ActorRuntime::Amp,
                home.join(".config/amp/settings.json"),
                false,
            ),
            (
                ActorRuntime::Auggie,
                home.join(".augment/settings.json"),
                false,
            ),
            (ActorRuntime::Kimi, home.join("kimi/mcp.json"), false),
        ];
        for (runtime, path, nested_cline) in fixtures {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
            let entry = if nested_cline {
                json!({"transport":{"type":"stdio","command":"/opt/cccc","args":["mcp"]}})
            } else {
                json!({"command":"/opt/cccc","args":["mcp"]})
            };
            let document = if runtime == ActorRuntime::Amp {
                json!({"amp.mcpServers":{"cccc":entry}})
            } else {
                json!({"mcpServers":{"cccc":entry}})
            };
            cccc_core::fs::write_json(&path, &document).expect("fixture");
            assert_eq!(
                json_state(runtime, home, &env, &expected).state,
                State::Ready,
                "{}",
                cccc_core::runtime_mcp::name(runtime)
            );
        }
    }

    #[test]
    fn cli_backed_runtime_outputs_match_python_parsers() {
        let expected = ["/opt/cccc".into(), "mcp".into()];
        let fixtures = [
            (
                ActorRuntime::Claude,
                "Transport: stdio\nCommand: /opt/cccc\nArgs: mcp\nScope: User config",
            ),
            (
                ActorRuntime::Copilot,
                r#"{"cccc":{"command":"/opt/cccc","args":["mcp"],"source":"user","tools":["*"]}}"#,
            ),
            (
                ActorRuntime::Devin,
                r#"McpServer { transport: stdio, command: "/opt/cccc", args: ["mcp"] }"#,
            ),
            (
                ActorRuntime::Grok,
                r#"[{"name":"cccc","command":"/opt/cccc","args":["mcp"],"enabled":true,"env":{"PYTHONUNBUFFERED":"1"}}]"#,
            ),
        ];
        for (runtime, output) in fixtures {
            assert_eq!(
                command_output_state(runtime, output, &expected).state,
                State::Ready,
                "{}",
                cccc_core::runtime_mcp::name(runtime)
            );
        }
    }
}
