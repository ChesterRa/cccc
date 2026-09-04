use std::path::Path;

pub(super) fn append_mcp_overrides(
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
    insert_before_prompt_tail(
        command,
        [
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
        ],
    );
}

pub(super) fn append_global_user_mcp_overrides(
    command: &mut Vec<String>,
    home: &Path,
    executable: &Path,
) {
    let executable_toml = toml_string(executable);
    let home = toml_string(home);
    let arguments = [
        "-c".into(),
        format!("mcp_servers.cccc.command={executable_toml}"),
        "-c".into(),
        "mcp_servers.cccc.args=[\"mcp\"]".into(),
        "-c".into(),
        format!("mcp_servers.cccc.env.CCCC_HOME={home}"),
        "-c".into(),
        "mcp_servers.cccc.env.CCCC_GROUP_ID=\"\"".into(),
        "-c".into(),
        "mcp_servers.cccc.env.CCCC_ACTOR_ID=\"user\"".into(),
        "-c".into(),
        "mcp_servers.cccc.env.CCCC_MCP_TOOL_PROFILE=\"full\"".into(),
    ];
    let index = command
        .iter()
        .position(|argument| argument == "app-server")
        .unwrap_or(command.len());
    command.splice(index..index, arguments);
}

fn insert_before_prompt_tail(
    command: &mut Vec<String>,
    arguments: impl IntoIterator<Item = String>,
) {
    let index = command
        .iter()
        .position(|argument| matches!(argument.as_str(), "app-server" | "--"))
        .unwrap_or(command.len());
    command.splice(index..index, arguments);
}

fn toml_string(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).unwrap_or_else(|_| "\"\"".into())
}
