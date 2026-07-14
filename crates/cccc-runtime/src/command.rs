use cccc_contracts::ActorRuntime;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbe {
    pub name: String,
    pub command: String,
    pub available: bool,
    pub path: Option<PathBuf>,
}

#[must_use]
pub fn default_command(runtime: ActorRuntime) -> Vec<String> {
    let command = match runtime {
        ActorRuntime::Amp => "amp",
        ActorRuntime::Antigravity => "agy --dangerously-skip-permissions",
        ActorRuntime::Auggie => "auggie",
        ActorRuntime::Claude => "claude --dangerously-skip-permissions",
        ActorRuntime::Codex => {
            "codex -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox --search"
        }
        ActorRuntime::Copilot => "copilot --allow-all",
        ActorRuntime::Cursor => "cursor-agent --yolo --approve-mcps",
        ActorRuntime::Devin => "devin --permission-mode dangerous",
        ActorRuntime::Kiro => "kiro-cli chat --trust-all-tools",
        ActorRuntime::Kilo => "kilo",
        ActorRuntime::Droid => "droid --auto high",
        ActorRuntime::Grok => "grok --always-approve",
        ActorRuntime::Hermes => "hermes --tui --yolo",
        ActorRuntime::Kimi => "kimi --yolo",
        ActorRuntime::Opencode => "opencode --auto",
        ActorRuntime::WebModel | ActorRuntime::Custom => "",
    };
    command.split_whitespace().map(str::to_owned).collect()
}

#[must_use]
pub fn detect_runtimes() -> Vec<RuntimeProbe> {
    serde_json::from_value::<Vec<ActorRuntime>>(serde_json::json!([
        "claude",
        "codex",
        "copilot",
        "cursor",
        "devin",
        "kiro",
        "kilo",
        "antigravity",
        "droid",
        "amp",
        "auggie",
        "grok",
        "hermes",
        "kimi",
        "opencode",
        "web_model"
    ]))
    .unwrap_or_default()
    .into_iter()
    .map(|runtime| {
        let command = default_command(runtime)
            .first()
            .cloned()
            .unwrap_or_default();
        let path = find_executable(&command);
        RuntimeProbe {
            name: serde_json::to_value(runtime)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
            available: runtime == ActorRuntime::WebModel || path.is_some(),
            command,
            path,
        }
    })
    .collect()
}

fn find_executable(command: &str) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(command);
    if candidate.components().count() > 1 && candidate.is_file() {
        return Some(candidate);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        let path = dir.join(command);
        if path.is_file() {
            return Some(path);
        }
        #[cfg(windows)]
        for extension in ["exe", "cmd", "bat"] {
            let path = dir.join(format!("{command}.{extension}"));
            if path.is_file() {
                return Some(path);
            }
        }
        None
    })
}
