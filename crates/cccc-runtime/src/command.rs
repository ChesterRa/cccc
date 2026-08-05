use cccc_contracts::ActorRuntime;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbe {
    pub name: String,
    pub display_name: String,
    pub recommended_command: String,
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
        ActorRuntime::Cline => "cline --tui --auto-approve true",
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
        "cline",
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
        "web_model",
        "custom"
    ]))
    .unwrap_or_default()
    .into_iter()
    .map(|runtime| {
        let recommended = default_command(runtime);
        let command = recommended.first().cloned().unwrap_or_default();
        let path = find_executable(&command);
        let name = runtime_name(runtime).to_owned();
        RuntimeProbe {
            display_name: display_name(runtime).to_owned(),
            recommended_command: recommended.join(" "),
            name,
            available: matches!(runtime, ActorRuntime::WebModel | ActorRuntime::Custom)
                || path.is_some(),
            command,
            path,
        }
    })
    .collect()
}

const fn runtime_name(runtime: ActorRuntime) -> &'static str {
    match runtime {
        ActorRuntime::Amp => "amp",
        ActorRuntime::Antigravity => "antigravity",
        ActorRuntime::Auggie => "auggie",
        ActorRuntime::Claude => "claude",
        ActorRuntime::Cline => "cline",
        ActorRuntime::Codex => "codex",
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

const fn display_name(runtime: ActorRuntime) -> &'static str {
    match runtime {
        ActorRuntime::Amp => "Amp",
        ActorRuntime::Antigravity => "Antigravity",
        ActorRuntime::Auggie => "Auggie",
        ActorRuntime::Claude => "Claude Code",
        ActorRuntime::Cline => "Cline CLI",
        ActorRuntime::Codex => "Codex CLI",
        ActorRuntime::Copilot => "GitHub Copilot",
        ActorRuntime::Cursor => "Cursor Agent",
        ActorRuntime::Devin => "Devin",
        ActorRuntime::Kiro => "Kiro CLI",
        ActorRuntime::Kilo => "Kilo Code",
        ActorRuntime::Droid => "Factory Droid",
        ActorRuntime::Grok => "Grok",
        ActorRuntime::Hermes => "Hermes",
        ActorRuntime::Kimi => "Kimi CLI",
        ActorRuntime::Opencode => "OpenCode",
        ActorRuntime::WebModel => "Web Model",
        ActorRuntime::Custom => "Custom",
    }
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

#[cfg(test)]
mod tests {
    use super::{default_command, detect_runtimes};
    use cccc_contracts::ActorRuntime;

    #[test]
    fn runtime_discovery_returns_frontend_contract() {
        let runtimes = detect_runtimes();
        let custom = runtimes
            .iter()
            .find(|runtime| runtime.name == "custom")
            .expect("custom runtime");
        assert_eq!(custom.display_name, "Custom");
        assert!(custom.available);
        assert!(runtimes.iter().any(|runtime| runtime.name == "codex"));
        let cline = runtimes
            .iter()
            .find(|runtime| runtime.name == "cline")
            .expect("cline runtime");
        assert_eq!(cline.display_name, "Cline CLI");
        assert_eq!(cline.recommended_command, "cline --tui --auto-approve true");
        assert_eq!(
            default_command(ActorRuntime::Cline),
            ["cline", "--tui", "--auto-approve", "true"]
        );
    }
}
