use std::collections::BTreeMap;
use std::io;
use std::path::Path;

pub(super) const VOICE_ANALYST_AGENT: &str = "cccc-voice-analyst";

pub(super) fn launch_prefix(
    executable: &Path,
    name: &str,
    environment: &BTreeMap<String, String>,
) -> io::Result<Vec<String>> {
    let filename = executable
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if filename.eq_ignore_ascii_case(name) || filename.eq_ignore_ascii_case(&format!("{name}.exe"))
    {
        return Ok(vec![executable.to_string_lossy().into_owned()]);
    }
    if name == "kilo" && filename.eq_ignore_ascii_case("kilo.cmd") {
        let bin = executable.parent().expect("resolved executable has parent");
        // npm global: <prefix>/kilo.cmd -> node_modules/@kilocode/cli/bin/kilo
        // npm local: node_modules/.bin/kilo.cmd -> ../@kilocode/cli/bin/kilo
        let modules = if bin.file_name().is_some_and(|name| name == ".bin") {
            bin.parent().unwrap_or(bin).to_owned()
        } else {
            bin.join("node_modules")
        };
        let script = modules.join("@kilocode/cli/bin/kilo");
        if !script.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "Kilo npm entrypoint is missing: {}. Reinstall @kilocode/cli or configure a direct kilo.exe path.",
                    script.display()
                ),
            ));
        }
        // Match npm's node selection, but bypass cmd.exe. The official JS
        // launcher still owns CPU/binary selection, KILO_BIN_PATH, resources,
        // and signal forwarding. Node and its children stay in CCCC's Job.
        let local_node = bin.join("node.exe");
        let node = if local_node.is_file() {
            local_node.to_string_lossy().into_owned()
        } else {
            let resolved =
                cccc_runtime::resolve_command_executable(&["node.exe".into()], environment);
            let node = &resolved[0];
            if !Path::new(node).is_absolute() || !Path::new(node).is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Kilo npm launch requires node.exe alongside its shim or in PATH",
                ));
            }
            node.clone()
        };
        return Ok(vec![node, script.to_string_lossy().into_owned()]);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "{name} managed sessions require its direct executable{}; custom wrappers and renamed binaries are not supported",
            if name == "kilo" {
                " or official npm kilo.cmd entrypoint"
            } else {
                ""
            }
        ),
    ))
}

#[derive(Debug, Default)]
pub(super) struct ParsedArguments {
    pub(super) acp_arguments: Vec<String>,
    pub(super) tui_arguments: Vec<String>,
    pub(super) model: Option<String>,
    pub(super) agent: Option<String>,
}

pub(super) fn parse_arguments(arguments: &[String]) -> io::Result<ParsedArguments> {
    let mut parsed = ParsedArguments::default();
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        match argument {
            "--auto" => index += 1,
            "--pure" | "--print-logs" => {
                parsed.acp_arguments.push(argument.into());
                parsed.tui_arguments.push(argument.into());
                index += 1;
            }
            "--log-level" => {
                let value = following(arguments, index, argument)?;
                parsed.acp_arguments.extend([argument.into(), value.into()]);
                parsed.tui_arguments.extend([argument.into(), value.into()]);
                index += 2;
            }
            "--model" | "-m" => {
                parsed.model = Some(following(arguments, index, argument)?.into());
                index += 2;
            }
            "--agent" => {
                parsed.agent = Some(following(arguments, index, argument)?.into());
                index += 2;
            }
            _ if prefixed(argument, "--log-level=") => {
                parsed.acp_arguments.push(argument.into());
                parsed.tui_arguments.push(argument.into());
                index += 1;
            }
            _ if prefixed(argument, "--model=") => {
                parsed.model = argument.split_once('=').map(|(_, value)| value.into());
                index += 1;
            }
            _ if prefixed(argument, "--agent=") => {
                parsed.agent = argument.split_once('=').map(|(_, value)| value.into());
                index += 1;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "OpenCode runtime command contains an unsupported managed-session argument: {argument}. Configure model, agent, pure mode, logging, or environment only; CCCC owns ACP, server, session, TUI attach, and permission flags."
                    ),
                ));
            }
        }
    }
    Ok(parsed)
}

pub(super) fn write_voice_analyst_agent(
    cwd: &Path,
    instructions: &str,
    runtime: cccc_contracts::ActorRuntime,
) -> io::Result<()> {
    let directory = cwd.join(if runtime == cccc_contracts::ActorRuntime::Kilo {
        ".kilo/agents"
    } else {
        ".opencode/agents"
    });
    std::fs::create_dir_all(&directory)?;
    let content =
        format!("---\ndescription: CCCC Voice Analyst\nmode: primary\n---\n{instructions}\n");
    let path = directory.join(format!("{VOICE_ANALYST_AGENT}.md"));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
        std::fs::write(path, content)?;
    }
    Ok(())
}

fn following<'a>(arguments: &'a [String], index: usize, flag: &str) -> io::Result<&'a str> {
    arguments
        .get(index + 1)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("OpenCode runtime command flag {flag} requires a value"),
            )
        })
}

fn prefixed(value: &str, prefix: &str) -> bool {
    value.starts_with(prefix) && value.len() > prefix.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_preserves_configuration_and_removes_host_policy() {
        let parsed = parse_arguments(&[
            "--model".into(),
            "openai/gpt-5".into(),
            "--agent=build".into(),
            "--pure".into(),
            "--auto".into(),
        ])
        .expect("managed arguments");
        assert_eq!(parsed.model.as_deref(), Some("openai/gpt-5"));
        assert_eq!(parsed.agent.as_deref(), Some("build"));
        assert_eq!(parsed.acp_arguments, ["--pure"]);
        assert!(!parsed.tui_arguments.contains(&"--auto".into()));
    }

    #[test]
    fn parser_rejects_session_and_subcommand_ownership() {
        for arguments in [
            vec!["--session".into(), "old".into()],
            vec!["attach".into(), "http://localhost".into()],
            vec!["run".into(), "fix this".into()],
        ] {
            assert!(parse_arguments(&arguments).is_err());
        }
    }
}
