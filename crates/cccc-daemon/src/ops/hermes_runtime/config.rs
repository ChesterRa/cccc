use cccc_core::fs::{read_yaml, write_yaml};
use serde_json::{Value, json};
use serde_yaml::{Mapping, Value as YamlValue};
use std::io;
use std::path::{Path, PathBuf};

use super::{PLACEHOLDERS, SERVER};

pub(super) fn inspect_mcp(config: &Mapping, expected: &[String]) -> Value {
    let Some(entry) = config
        .get(YamlValue::String("mcp_servers".into()))
        .and_then(YamlValue::as_mapping)
        .and_then(|servers| servers.get(YamlValue::String(SERVER.into())))
        .and_then(YamlValue::as_mapping)
    else {
        return json!({"status":"missing","configured":false,"server_name":SERVER,"expected_command":expected});
    };
    let command = yaml_string(entry, "command");
    let args = entry
        .get(YamlValue::String("args".into()))
        .and_then(YamlValue::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(YamlValue::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let env = entry
        .get(YamlValue::String("env".into()))
        .and_then(YamlValue::as_mapping);
    let env_ok = env.is_some_and(|env| {
        PLACEHOLDERS
            .iter()
            .all(|(key, value)| yaml_string(env, key) == *value)
    });
    let command_ok = expected
        .first()
        .is_some_and(|value| Path::new(value) == Path::new(&command));
    let args_ok = args == expected.get(1..).unwrap_or_default();
    json!({"status":if command_ok&&args_ok&&env_ok{"ready"}else{"stale"},"configured":true,"server_name":SERVER,"command":command,"args":args,"expected_command":expected,"command_matches":command_ok,"args_match":args_ok,"env_placeholders_match":env_ok})
}

pub(super) fn normalize_placeholders(path: &Path) -> io::Result<()> {
    let mut config = load_config(path);
    let servers = config
        .entry(YamlValue::String("mcp_servers".into()))
        .or_insert_with(|| YamlValue::Mapping(Mapping::new()))
        .as_mapping_mut()
        .ok_or_else(|| io::Error::other("invalid Hermes mcp_servers"))?;
    let entry = servers
        .get_mut(YamlValue::String(SERVER.into()))
        .and_then(YamlValue::as_mapping_mut)
        .ok_or_else(|| io::Error::other("Hermes did not persist CCCC MCP config"))?;
    let env = entry
        .entry(YamlValue::String("env".into()))
        .or_insert_with(|| YamlValue::Mapping(Mapping::new()))
        .as_mapping_mut()
        .ok_or_else(|| io::Error::other("invalid Hermes MCP env"))?;
    for (key, value) in PLACEHOLDERS {
        env.insert(
            YamlValue::String(key.into()),
            YamlValue::String(value.into()),
        );
    }
    write_yaml(path, &config)
}

pub(super) fn load_config(path: &Path) -> Mapping {
    read_yaml::<Mapping>(path).unwrap_or_default()
}

fn yaml_string(map: &Mapping, key: &str) -> String {
    map.get(YamlValue::String(key.into()))
        .and_then(YamlValue::as_str)
        .unwrap_or("")
        .into()
}

pub(super) fn hermes_home() -> PathBuf {
    std::env::var_os("HERMES_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".hermes")))
        .unwrap_or_else(|| PathBuf::from(".hermes"))
}

pub(super) fn cccc_command() -> Vec<String> {
    let executable =
        crate::ops::codex_mcp::resolve_cccc_executable().unwrap_or_else(|| PathBuf::from("cccc"));
    vec![executable.to_string_lossy().into_owned(), "mcp".into()]
}

pub(super) fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH");
    cccc_core::runtime_mcp::find_program(name, path.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_actor_placeholders_in_hermes_mcp_config() {
        let config: Mapping = serde_yaml::from_str(
            r#"
mcp_servers:
  cccc:
    command: /tmp/cccc
    args: [mcp]
    env:
      CCCC_HOME: ${CCCC_HOME}
      CCCC_GROUP_ID: ${CCCC_GROUP_ID}
      CCCC_ACTOR_ID: ${CCCC_ACTOR_ID}
"#,
        )
        .expect("yaml");
        let state = inspect_mcp(&config, &["/tmp/cccc".into(), "mcp".into()]);
        assert_eq!(state["status"], "ready");
        assert_eq!(state["env_placeholders_match"], true);
    }
}
