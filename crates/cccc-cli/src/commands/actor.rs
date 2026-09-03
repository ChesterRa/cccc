use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::path::Path;

use crate::args::{ActorAction, ActorArgs};
use crate::commands::common::{call, env, group, print};

pub async fn run(client: &DaemonClient, home: &HomeLayout, args: ActorArgs) -> Result<()> {
    let response = match args.action {
        ActorAction::List { group_id } => {
            call(
                client,
                "actor_list",
                json!({"group_id":group(home,group_id)?,"by":"user"}),
            )
            .await?
        }
        ActorAction::Add {
            actor_id,
            title,
            runtime,
            runner,
            command,
            env: raw_env,
            scope,
            submit,
            group_id,
            by,
        } => {
            let command = parse_command(&command)?;
            let scope = canonical_scope_key(&scope)?;
            call(
                client,
                "actor_add",
                json!({
                    "group_id":group(home,group_id)?,"actor_id":actor_id,"title":title,
                    "runtime":runtime,"runner":runner,"command":command,"env":env(raw_env)?,
                    "default_scope_key":scope,"submit":submit,"by":by
                }),
            )
            .await?
        }
        ActorAction::Remove(target) => lifecycle(client, home, "actor_remove", target).await?,
        ActorAction::Start(target) => lifecycle(client, home, "actor_start", target).await?,
        ActorAction::Stop(target) => lifecycle(client, home, "actor_stop", target).await?,
        ActorAction::Restart(target) => lifecycle(client, home, "actor_restart", target).await?,
        ActorAction::Update {
            actor_id,
            group_id,
            title,
            runtime,
            runner,
            scope,
            command,
            env: raw_env,
            submit,
            enabled,
            runtime_state_source,
            by,
        } => {
            let mut patch = Map::new();
            optional(&mut patch, "title", title);
            optional(&mut patch, "runtime", runtime);
            optional(&mut patch, "runner", runner);
            if let Some(scope) = scope {
                patch.insert(
                    "default_scope_key".into(),
                    Value::String(canonical_scope_key(&scope)?),
                );
            }
            optional(&mut patch, "submit", submit);
            optional(&mut patch, "runtime_state_source", runtime_state_source);
            if let Some(enabled) = enabled {
                patch.insert("enabled".into(), Value::Bool(enabled));
            }
            if let Some(command) = command {
                patch.insert("command".into(), json!(parse_command(&command)?));
            }
            if !raw_env.is_empty() {
                patch.insert("env".into(), Value::Object(env(raw_env)?));
            }
            call(
                client,
                "actor_update",
                json!({"group_id":group(home,group_id)?,"actor_id":actor_id,"patch":patch,"by":by}),
            )
            .await?
        }
        ActorAction::Secrets {
            actor_id,
            group_id,
            set,
            unset,
            clear,
            keys,
            restart,
            by,
        } => {
            let group_id = group(home, group_id)?;
            if keys || (set.is_empty() && unset.is_empty() && !clear) {
                call(
                    client,
                    "actor_env_private_keys",
                    json!({"group_id":group_id,"actor_id":actor_id,"by":by}),
                )
                .await?
            } else {
                let updated = call(client, "actor_env_private_update", json!({"group_id":group_id,"actor_id":actor_id,"set":env(set)?,"unset":unset,"clear":clear,"by":&by})).await?;
                if restart && updated.ok {
                    call(
                        client,
                        "actor_restart",
                        json!({"group_id":group_id,"actor_id":actor_id,"by":by}),
                    )
                    .await?
                } else {
                    updated
                }
            }
        }
    };
    print(response)
}

fn canonical_scope_key(path: &str) -> Result<String> {
    if path.trim().is_empty() {
        return Ok(String::new());
    }
    Ok(cccc_core::scope::detect(Path::new(path))?.scope_key)
}

async fn lifecycle(
    client: &DaemonClient,
    home: &HomeLayout,
    op: &str,
    target: crate::args::ActorTarget,
) -> Result<cccc_contracts::DaemonResponse> {
    call(
        client,
        op,
        json!({"group_id":group(home,target.group_id)?,"actor_id":target.actor_id,"by":target.by}),
    )
    .await
}

fn parse_command(command: &str) -> Result<Vec<String>> {
    if command.trim().is_empty() {
        return Ok(Vec::new());
    }
    #[cfg(windows)]
    return parse_windows_command(command);
    #[cfg(not(windows))]
    shell_words::split(command).map_err(Into::into)
}

/// Split the human-facing `--command` value without treating Windows path
/// separators as POSIX escapes. Quotes only group text and are not retained.
#[cfg(any(windows, test))]
fn parse_windows_command(command: &str) -> Result<Vec<String>> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    let mut characters = command.chars().peekable();
    while let Some(character) = characters.next() {
        if quote != Some('\'') && character == '\\' {
            let mut backslashes = 1;
            while characters.peek() == Some(&'\\') {
                characters.next();
                backslashes += 1;
            }
            if characters.peek() == Some(&'"') {
                current.extend(std::iter::repeat_n('\\', backslashes / 2));
                characters.next();
                if backslashes % 2 == 1 {
                    current.push('"');
                } else if quote == Some('"') {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some('"');
                } else {
                    current.push('"');
                }
                started = true;
                continue;
            }
            current.extend(std::iter::repeat_n('\\', backslashes));
            started = true;
            continue;
        }
        match (quote, character) {
            (None, '\'' | '"') => {
                quote = Some(character);
                started = true;
            }
            (Some(open), value) if value == open => quote = None,
            (None, value) if value.is_whitespace() => {
                if started {
                    values.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (_, value) => {
                current.push(value);
                started = true;
            }
        }
    }
    anyhow::ensure!(quote.is_none(), "unterminated quote in command");
    if started {
        values.push(current);
    }
    Ok(values)
}

fn optional(map: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        map.insert(key.into(), Value::String(value));
    }
}

#[cfg(test)]
mod tests {
    use super::parse_windows_command;

    #[test]
    fn windows_command_parser_preserves_path_separators() {
        assert_eq!(
            parse_windows_command(
                r#"C:\Users\USER\AppData\Roaming\npm\claude.cmd --dangerously-skip-permissions"#
            )
            .expect("command"),
            [
                r"C:\Users\USER\AppData\Roaming\npm\claude.cmd",
                "--dangerously-skip-permissions"
            ]
        );
    }

    #[test]
    fn windows_command_parser_groups_quoted_paths_and_empty_arguments() {
        assert_eq!(
            parse_windows_command(r#""C:\Program Files\Claude\claude.exe" --name ''"#)
                .expect("command"),
            [r"C:\Program Files\Claude\claude.exe", "--name", ""]
        );
    }

    #[test]
    fn windows_command_parser_rejects_unterminated_quotes() {
        assert!(parse_windows_command(r#""C:\Program Files\Claude"#).is_err());
    }

    #[test]
    fn windows_command_parser_preserves_escaped_json_quotes() {
        assert_eq!(
            parse_windows_command("tool --json \"{\\\"key\\\":\\\"value\\\"}\"").expect("command"),
            ["tool", "--json", r#"{"key":"value"}"#]
        );
    }
}
