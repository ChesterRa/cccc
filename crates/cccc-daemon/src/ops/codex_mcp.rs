use cccc_core::HomeLayout;
use std::collections::BTreeMap;

#[path = "codex_mcp_launcher.rs"]
mod launcher;
pub(crate) use launcher::{configure_actor_cli, resolve_cccc_executable};
#[path = "codex_mcp_overrides.rs"]
mod overrides;
use overrides::{append_global_user_mcp_overrides, append_mcp_overrides};

pub(crate) fn configure_mcp_only(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> bool {
    let Some(executable) = configure_actor_cli(env) else {
        return false;
    };
    append_mcp_overrides(command, home.root(), &executable, group_id, actor_id);
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    true
}

pub(crate) fn configure_global_user_mcp(
    home: &HomeLayout,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> bool {
    let Some(executable) = configure_actor_cli(env) else {
        return false;
    };
    append_global_user_mcp_overrides(command, home.root(), &executable);
    if let Some(origin) = env.get(cccc_core::voice_notifications::ORIGIN_ENV) {
        let index = command
            .iter()
            .position(|arg| arg == "app-server")
            .unwrap_or(command.len());
        command.splice(
            index..index,
            [
                "-c".into(),
                format!(
                    "mcp_servers.cccc.env.CCCC_VOICE_ORIGIN={}",
                    serde_json::to_string(origin).expect("origin string")
                ),
            ],
        );
    }
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    env.insert("CCCC_GROUP_ID".into(), String::new());
    env.insert("CCCC_ACTOR_ID".into(), "user".into());
    env.insert("CCCC_MCP_TOOL_PROFILE".into(), "full".into());
    true
}

#[cfg(test)]
#[path = "codex_mcp_tests.rs"]
mod tests;
