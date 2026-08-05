use anyhow::{Context, Result, bail};
use cccc_core::HomeLayout;
use serde_json::json;
use std::path::Path;
use std::process::Command;

use crate::args::SetupArgs;

const SUPPORTED: &[&str] = &[
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
    "custom",
];

pub fn run(home: &HomeLayout, args: SetupArgs) -> Result<()> {
    let executable = std::env::current_exe()?;
    let runtime = args.runtime.as_deref().map(str::trim).unwrap_or("");
    let config = json!({
        "mcpServers":{"cccc":{"command":executable,"args":["mcp"],"env":{"CCCC_HOME":home.root()}}}
    });
    if runtime.is_empty() || matches!(runtime, "custom" | "hermes") {
        println!("{}", serde_json::to_string_pretty(&config)?);
        return Ok(());
    }
    if !SUPPORTED.contains(&runtime) {
        bail!(
            "unsupported runtime {runtime}; supported: {}",
            SUPPORTED.join(", ")
        );
    }
    if matches!(runtime, "cursor" | "kilo" | "antigravity") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "runtime":runtime,
                "mode":"prompt_assisted",
                "project_path":absolute(&args.path)?,
                "config":config,
                "instruction":"Add or replace the stdio MCP server named cccc with this configuration, then verify it is enabled."
            }))?
        );
        return Ok(());
    }
    if runtime == "opencode" {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "runtime":runtime,"mode":"runtime_env","status":"managed_by_cccc_actor","config":config
            }))?
        );
        return Ok(());
    }
    let command = add_command(runtime, &executable)?;
    let cwd = absolute(&args.path)?;
    let mut output = run_command(&command, &cwd, home)?;
    if !output.status.success()
        && already_exists(&output)
        && let Some(remove) = remove_command(runtime)
    {
        let removed = run_command(&remove, &cwd, home)?;
        if !removed.status.success() {
            bail!(
                "failed to replace existing CCCC MCP entry: {}",
                failure_detail(&removed)
            );
        }
        output = run_command(&command, &cwd, home)?;
    }
    if !output.status.success() {
        bail!(
            "{} failed ({}): {}",
            display(&command),
            output.status,
            failure_detail(&output)
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "runtime":runtime,"mode":"auto","status":"added","command":command,"config":config
        }))?
    );
    Ok(())
}

fn run_command(command: &[String], cwd: &Path, home: &HomeLayout) -> Result<std::process::Output> {
    Command::new(&command[0])
        .args(&command[1..])
        .current_dir(cwd)
        .env("CCCC_HOME", home.root())
        .output()
        .with_context(|| format!("{} CLI not found", command[0]))
}

fn already_exists(output: &std::process::Output) -> bool {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();
    ["already exists", "already added", "duplicate"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn failure_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        stderr
    }
}

fn remove_command(runtime: &str) -> Option<Vec<String>> {
    let parts: &[&str] = match runtime {
        "claude" => &["claude", "mcp", "remove", "cccc", "-s", "user"],
        "codex" => &["codex", "mcp", "remove", "cccc"],
        "copilot" => &["copilot", "mcp", "remove", "cccc"],
        "devin" => &["devin", "mcp", "remove", "-s", "user", "cccc"],
        "droid" => &["droid", "mcp", "remove", "cccc"],
        "amp" => &["amp", "mcp", "remove", "cccc"],
        "auggie" => &["auggie", "mcp", "remove", "cccc"],
        "grok" => &["grok", "mcp", "remove", "cccc"],
        "kimi" => &["kimi", "mcp", "remove", "cccc"],
        _ => return None,
    };
    Some(parts.iter().map(|part| (*part).to_owned()).collect())
}

fn add_command(runtime: &str, executable: &Path) -> Result<Vec<String>> {
    let cccc = executable.to_string_lossy().into_owned();
    let command = match runtime {
        "claude" => vec![
            "claude", "mcp", "add", "-s", "user", "cccc", "--", &cccc, "mcp",
        ],
        "codex" => vec!["codex", "mcp", "add", "cccc", "--", &cccc, "mcp"],
        "copilot" => vec!["copilot", "mcp", "add", "cccc", "--", &cccc, "mcp"],
        "devin" => vec![
            "devin", "mcp", "add", "-s", "user", "cccc", "--", &cccc, "mcp",
        ],
        "droid" => vec![
            "droid", "mcp", "add", "--type", "stdio", "cccc", &cccc, "mcp",
        ],
        "amp" => vec!["amp", "mcp", "add", "cccc", &cccc, "mcp"],
        "auggie" => vec!["auggie", "mcp", "add", "cccc", "--", &cccc, "mcp"],
        "kimi" => vec![
            "kimi",
            "mcp",
            "add",
            "--transport",
            "stdio",
            "cccc",
            "--",
            &cccc,
            "mcp",
        ],
        "kiro" => {
            return Ok(vec![
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
            ]);
        }
        "grok" => {
            return Ok(vec![
                "grok".into(),
                "mcp".into(),
                "add".into(),
                "cccc".into(),
                "--command".into(),
                cccc,
                "--args".into(),
                "mcp".into(),
            ]);
        }
        _ => {
            return Err(anyhow::anyhow!(
                "runtime {runtime} requires manual MCP setup"
            ));
        }
    };
    Ok(command.into_iter().map(str::to_owned).collect())
}

fn absolute(path: &str) -> Result<std::path::PathBuf> {
    let path = std::path::PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn display(command: &[String]) -> String {
    command.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_codex_command_with_compiled_binary() {
        assert_eq!(
            add_command("codex", Path::new("/opt/cccc")).expect("command"),
            ["codex", "mcp", "add", "cccc", "--", "/opt/cccc", "mcp"]
        );
    }
}
