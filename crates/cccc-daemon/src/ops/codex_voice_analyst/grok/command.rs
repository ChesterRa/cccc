use std::io;
use std::path::Path;

#[derive(Debug, Default)]
pub(super) struct ParsedArguments {
    pub(super) agent_arguments: Vec<String>,
    pub(super) tui_arguments: Vec<String>,
    pub(super) rules: Vec<String>,
}

pub(super) fn parse_arguments(arguments: &[String]) -> io::Result<ParsedArguments> {
    let mut parsed = ParsedArguments::default();
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        match argument {
            "--always-approve" | "--yolo" | "--dangerously-skip-permissions" => {
                index += 1;
            }
            "--model" | "-m" | "--reasoning-effort" => {
                let value = following(arguments, index, argument)?;
                parsed
                    .agent_arguments
                    .extend([argument.into(), value.into()]);
                parsed.tui_arguments.extend([argument.into(), value.into()]);
                index += 2;
            }
            "--agent-profile"
            | "--plugin-dir"
            | "--cli-chat-proxy-base-url"
            | "--xai-api-base-url"
            | "--grok-ws-origin"
            | "--grok-ws-url" => {
                let value = following(arguments, index, argument)?;
                parsed
                    .agent_arguments
                    .extend([argument.into(), value.into()]);
                index += 2;
            }
            "--rules" | "--append-system-prompt" => {
                parsed
                    .rules
                    .push(following(arguments, index, argument)?.into());
                index += 2;
            }
            _ if matches_prefixed_value(argument, &["--model=", "--reasoning-effort="]) => {
                parsed.agent_arguments.push(argument.into());
                parsed.tui_arguments.push(argument.into());
                index += 1;
            }
            _ if matches_prefixed_value(
                argument,
                &[
                    "--agent-profile=",
                    "--plugin-dir=",
                    "--cli-chat-proxy-base-url=",
                    "--xai-api-base-url=",
                    "--grok-ws-origin=",
                    "--grok-ws-url=",
                ],
            ) =>
            {
                parsed.agent_arguments.push(argument.into());
                index += 1;
            }
            _ if matches_prefixed_value(argument, &["--rules=", "--append-system-prompt="]) => {
                parsed.rules.push(
                    argument
                        .split_once('=')
                        .map_or("", |(_, value)| value)
                        .into(),
                );
                index += 1;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "Grok runtime command contains an unsupported managed-session argument: {argument}. Configure model, reasoning effort, agent profile, plugin directories, provider URLs, rules, or environment only; CCCC owns agent/leader/session/permission flags."
                    ),
                ));
            }
        }
    }
    Ok(parsed)
}

fn following<'a>(arguments: &'a [String], index: usize, flag: &str) -> io::Result<&'a str> {
    arguments
        .get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Grok runtime command flag {flag} requires a value"),
            )
        })
}

fn matches_prefixed_value(value: &str, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| value.starts_with(prefix) && value.len() > prefix.len())
}

#[cfg(unix)]
pub(super) fn set_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub(super) fn set_private_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_parser_preserves_provider_choices_and_removes_host_policy() {
        let parsed = parse_arguments(&[
            "--model".into(),
            "grok-4.6".into(),
            "--reasoning-effort=high".into(),
            "--agent-profile".into(),
            "/tmp/profile.json".into(),
            "--rules".into(),
            "Be concise".into(),
            "--always-approve".into(),
        ])
        .expect("managed arguments");
        assert!(parsed.agent_arguments.contains(&"grok-4.6".into()));
        assert!(
            parsed
                .tui_arguments
                .contains(&"--reasoning-effort=high".into())
        );
        assert!(!parsed.agent_arguments.contains(&"--always-approve".into()));
        assert_eq!(parsed.rules, ["Be concise"]);
    }

    #[test]
    fn command_parser_rejects_session_and_subcommand_ownership() {
        for arguments in [
            vec!["--resume".into(), "old".into()],
            vec!["agent".into(), "stdio".into()],
            vec!["fix this".into()],
        ] {
            assert!(parse_arguments(&arguments).is_err());
        }
    }
}
