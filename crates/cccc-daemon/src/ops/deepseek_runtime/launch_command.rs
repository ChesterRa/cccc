use cccc_contracts::{Actor, DEEPSEEK_MAX_OUTPUT_TOKENS};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

pub(super) struct Launch {
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub mcp_servers: Vec<Value>,
}

pub(super) fn prepare(
    actor: &Actor,
    env: &mut BTreeMap<String, String>,
    state: &Path,
    workspace: &Path,
    cccc_executable: Option<&Path>,
) -> io::Result<Launch> {
    if !cccc_runtime::deepseek_managed_command(&actor.command) {
        let sessions = state.join("sessions");
        std::fs::create_dir_all(&sessions)?;
        env.insert(
            "CCCC_DEEPSEEK_SESSION_ROOT".into(),
            sessions.to_string_lossy().into_owned(),
        );
        return Ok(Launch {
            command: actor.command.clone(),
            cwd: workspace.to_owned(),
            mcp_servers: vec![],
        });
    }

    let executable = cccc_runtime::deepseek_executable(env)
        .ok_or_else(|| io::Error::other("managed DeepSeek executable not found: dsh"))?;
    let cccc = cccc_executable
        .and_then(Path::to_str)
        .ok_or_else(|| io::Error::other("CCCC executable not available for DeepSeek MCP"))?;
    let context = ["CCCC_HOME", "CCCC_GROUP_ID", "CCCC_ACTOR_ID"]
        .map(|key| json!({"name":key, "value":env[key]}));
    let mcp = json!({"name":"cccc", "command": cccc, "args":["mcp"], "env":context});

    // The new harness has a different persistence format. Each member starts the CLI
    // from its own state directory; its ACP session and MCP use the actual workspace.
    let member_home = state.join("harness");
    env.insert(
        "CCCC_DEEPSEEK_SESSION_ROOT".into(),
        member_home.join("sessions").to_string_lossy().into_owned(),
    );
    env.insert(
        "DSH_HOME".into(),
        member_home.to_string_lossy().into_owned(),
    );
    env.insert(
        "DSH_AGENTS_HOME".into(),
        member_home.join("agents").to_string_lossy().into_owned(),
    );
    env.insert(
        "CCCC_DEEPSEEK_WORKSPACE".into(),
        workspace.to_string_lossy().into_owned(),
    );
    let route = if official_endpoint(env.get("DEEPSEEK_BASE_URL").map(String::as_str)) {
        // The official Messages adapter adds /v1/messages to the Anthropic API root.
        env.insert(
            "DEEPSEEK_BASE_URL".into(),
            "https://api.deepseek.com/anthropic".into(),
        );
        include_str!("../../../resources/deepseek-messages.yaml")
    } else {
        // Existing custom endpoints use streaming Chat Completions.
        include_str!("../../../resources/deepseek-chat.yaml")
    };
    let patch = member_home.join("cccc-acp.yaml");
    let common = include_str!("../../../resources/deepseek-acp.yaml");
    let config = format!(
        "{common}{}",
        route.replace(
            "__MAX_OUTPUT_TOKENS__",
            &DEEPSEEK_MAX_OUTPUT_TOKENS.to_string()
        )
    );
    cccc_core::fs::atomic_write(&patch, config.as_bytes())?;
    Ok(Launch {
        command: vec![
            executable.to_string_lossy().into_owned(),
            "--profile".into(),
            "acp".into(),
            "--patch".into(),
            patch.to_string_lossy().into_owned(),
        ],
        cwd: member_home,
        mcp_servers: vec![mcp],
    })
}

fn official_endpoint(base: Option<&str>) -> bool {
    let Some(base) = base else {
        return true;
    };
    url::Url::parse(base).is_ok_and(|url| {
        url.origin().ascii_serialization() == "https://api.deepseek.com"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && matches!(
                url.path().trim_end_matches('/'),
                "" | "/v1" | "/anthropic" | "/anthropic/v1"
            )
    })
}
