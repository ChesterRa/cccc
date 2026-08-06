use cccc_contracts::ActorRuntime;
use std::path::Path;

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
pub fn add_command(runtime: ActorRuntime, executable: &Path) -> Option<Vec<String>> {
    let cccc = executable.to_string_lossy().into_owned();
    let common = |parts: &[&str]| parts.iter().map(|part| (*part).to_owned()).collect();
    Some(match runtime {
        ActorRuntime::Claude => common(&[
            "claude", "mcp", "add", "-s", "user", "cccc", "--", &cccc, "mcp",
        ]),
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
        ActorRuntime::Grok => vec![
            "grok".into(),
            "mcp".into(),
            "add".into(),
            "cccc".into(),
            "--command".into(),
            cccc,
            "--args".into(),
            "mcp".into(),
            "--env".into(),
            "PYTHONUNBUFFERED=1".into(),
        ],
        ActorRuntime::Kimi => common(&[
            "kimi",
            "mcp",
            "add",
            "--transport",
            "stdio",
            "cccc",
            "--",
            &cccc,
            "mcp",
        ]),
        _ => return None,
    })
}

#[must_use]
pub fn remove_command(runtime: ActorRuntime) -> Option<Vec<String>> {
    let parts: &[&str] = match runtime {
        ActorRuntime::Claude => &["claude", "mcp", "remove", "cccc", "-s", "user"],
        ActorRuntime::Codex => &["codex", "mcp", "remove", "cccc"],
        ActorRuntime::Copilot => &["copilot", "mcp", "remove", "cccc"],
        ActorRuntime::Devin => &["devin", "mcp", "remove", "-s", "user", "cccc"],
        ActorRuntime::Kiro => &[
            "kiro-cli", "mcp", "remove", "--name", "cccc", "--scope", "global",
        ],
        ActorRuntime::Droid => &["droid", "mcp", "remove", "cccc"],
        ActorRuntime::Amp => &["amp", "mcp", "remove", "cccc"],
        ActorRuntime::Auggie => &["auggie", "mcp", "remove", "cccc"],
        ActorRuntime::Grok => &["grok", "mcp", "remove", "cccc"],
        ActorRuntime::Kimi => &["kimi", "mcp", "remove", "cccc"],
        _ => return None,
    };
    Some(parts.iter().map(|part| (*part).to_owned()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_managed_runtime_catalog_matches_python_contract() {
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
            ActorRuntime::Opencode,
        ];
        assert!(runtimes.into_iter().all(is_auto_managed));
        assert!(!is_auto_managed(ActorRuntime::Cursor));
        assert!(!is_auto_managed(ActorRuntime::Custom));
    }

    #[test]
    fn grok_setup_keeps_python_compatibility_environment() {
        let command = add_command(ActorRuntime::Grok, Path::new("/opt/cccc")).expect("command");
        assert!(
            command
                .windows(2)
                .any(|parts| parts == ["--env", "PYTHONUNBUFFERED=1"])
        );
    }
}
