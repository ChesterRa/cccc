use std::io;
use std::path::Path;

pub(super) const VOICE_ANALYST_AGENT: &str = "cccc-voice-analyst";

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

pub(super) fn write_voice_analyst_agent(cwd: &Path, instructions: &str) -> io::Result<()> {
    let directory = cwd.join(".opencode/agents");
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
