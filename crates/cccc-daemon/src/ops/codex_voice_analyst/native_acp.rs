//! Native, official ACP entry points. Commands express launch intent; CCCC owns
//! the transport, cwd, MCP binding and session identity.
use super::{AcpClient, ActorRuntime};
use serde_json::{Value, json};
use std::{io, path::Path, time::Duration};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(40);

pub(super) struct Options {
    pub command: Vec<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub unrestricted: bool,
}

pub(crate) fn name(runtime: ActorRuntime) -> &'static str {
    match runtime {
        ActorRuntime::Copilot => "copilot",
        ActorRuntime::Devin => "devin",
        ActorRuntime::Cursor => "cursor",
        _ => unreachable!("native ACP runtime was validated at launch"),
    }
}

pub(super) fn options(runtime: ActorRuntime, command: &[String]) -> io::Result<Options> {
    let default = cccc_runtime::default_command(runtime);
    let command = if command.is_empty() {
        &default
    } else {
        command
    };
    let program = command
        .first()
        .ok_or_else(|| invalid("Empty ACP command"))?;
    let stem = Path::new(program).file_stem().and_then(|s| s.to_str());
    let valid = match runtime {
        ActorRuntime::Copilot => stem == Some("copilot"),
        ActorRuntime::Devin => stem == Some("devin"),
        ActorRuntime::Cursor => matches!(stem, Some("agent" | "cursor-agent")),
        _ => false,
    };
    if !valid {
        return Err(invalid(
            "ACP requires the native executable, without a shell wrapper",
        ));
    }
    let mut result = Options {
        command: vec![program.clone()],
        model: None,
        mode: None,
        unrestricted: false,
    };
    let mut args = command.iter().skip(1);
    while let Some(argument) = args.next() {
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(k, v)| (k, Some(v)));
        match (runtime, flag) {
            (_, "--model") => {
                let value = value(inline, &mut args)?;
                result.model = Some(value.clone());
                // Devin/Cursor root CLI model flags do not select ACP models.
                // Apply the advertised ID after new/load instead.
                if runtime == ActorRuntime::Copilot {
                    result.command.extend([flag.into(), value]);
                }
            }
            (ActorRuntime::Devin, "--permission-mode") => {
                let value = value(inline, &mut args)?;
                result.mode = Some(
                    match value.as_str() {
                        "dangerous" => {
                            result.unrestricted = true;
                            "bypass"
                        }
                        "auto" | "accept-edits" => "accept-edits",
                        "smart" => "smart",
                        _ => return Err(invalid("Unsupported Devin ACP permission mode")),
                    }
                    .into(),
                );
                // This native global option applies the configured permission
                // policy as well as selecting the corresponding ACP mode.
                result.command.extend([flag.into(), value]);
            }
            (ActorRuntime::Cursor, "--mode") | (ActorRuntime::Copilot, "--mode") => {
                let value = value(inline, &mut args)?;
                let mode = match (runtime, value.as_str()) {
                    (ActorRuntime::Cursor, "ask" | "plan") => value.clone(),
                    (ActorRuntime::Copilot, "interactive") => "agent".into(),
                    (ActorRuntime::Copilot, "plan") => value.clone(),
                    _ => return Err(invalid("Unsupported ACP session mode")),
                };
                result.mode = Some(mode);
                result.command.extend([flag.into(), value]);
            }
            (ActorRuntime::Cursor, "--force" | "--yolo" | "-f")
            | (ActorRuntime::Copilot, "--allow-all" | "--yolo")
                if inline.is_none() =>
            {
                result.unrestricted = true;
                result.command.push(flag.into());
            }
            (ActorRuntime::Cursor, "--plan") | (ActorRuntime::Copilot, "--plan")
                if inline.is_none() =>
            {
                result.mode = Some("plan".into());
                result.command.push(flag.into());
            }
            (ActorRuntime::Cursor, "--approve-mcps" | "--trust" | "--auto-review")
            | (
                ActorRuntime::Copilot,
                "--allow-all-tools"
                | "--allow-all-paths"
                | "--allow-all-urls"
                | "--disable-builtin-mcps"
                | "--no-auto-update"
                | "--no-custom-instructions",
            ) if inline.is_none() => result.command.push(flag.into()),
            (
                ActorRuntime::Copilot,
                "--reasoning-effort" | "--effort" | "--available-tools" | "--excluded-tools"
                | "--allow-tool" | "--deny-tool" | "--allow-url" | "--deny-url" | "--add-dir"
                | "--context",
            )
            | (ActorRuntime::Cursor, "--sandbox" | "--add-dir")
            | (ActorRuntime::Devin, "--config") => result
                .command
                .extend([flag.into(), value(inline, &mut args)?]),
            _ => {
                return Err(invalid(
                    "Unsupported ACP launch argument. Use model/permission options; CCCC owns prompt, resume, transport and MCP configuration",
                ));
            }
        }
    }
    match runtime {
        ActorRuntime::Copilot => {
            result.command.extend(["--acp".into(), "--stdio".into()]);
            if !result.command.iter().any(|arg| arg == "--no-auto-update") {
                result.command.push("--no-auto-update".into());
            }
        }
        ActorRuntime::Devin | ActorRuntime::Cursor => result.command.push("acp".into()),
        _ => unreachable!(),
    }
    Ok(result)
}

fn value<'a>(
    inline: Option<&str>,
    args: &mut impl Iterator<Item = &'a String>,
) -> io::Result<String> {
    inline
        .or_else(|| args.next().map(String::as_str))
        .filter(|v| !v.is_empty() && !v.starts_with('-'))
        .map(str::to_owned)
        .ok_or_else(|| invalid("ACP launch option requires a value"))
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub(super) async fn initialize(
    protocol: &AcpClient,
    runtime: ActorRuntime,
    cwd: &Path,
    resume: Option<&str>,
    mcp: Value,
    options: &Options,
) -> io::Result<(String, bool)> {
    let capabilities = protocol.request("initialize", json!({"protocolVersion":1,"clientInfo":{"name":"cccc","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false}}), HANDSHAKE_TIMEOUT).await?;
    if resume.is_some()
        && capabilities
            .pointer("/agentCapabilities/loadSession")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(io::Error::other(
            "Official ACP server cannot resume this attempted session; use New Session explicitly",
        ));
    }
    if runtime == ActorRuntime::Cursor {
        protocol
            .request(
                "authenticate",
                json!({"methodId":"cursor_login"}),
                HANDSHAKE_TIMEOUT,
            )
            .await?;
    }
    let mut params = json!({"cwd":cwd.to_string_lossy(),"mcpServers":if runtime == ActorRuntime::Copilot { vec![] } else { vec![mcp] }});
    let method = if let Some(id) = resume {
        params["sessionId"] = json!(id);
        "session/load"
    } else {
        "session/new"
    };
    let result = protocol.request(method, params, HANDSHAKE_TIMEOUT).await?;
    let id = resume
        .or_else(|| result["sessionId"].as_str())
        .filter(|id| valid_id(id))
        .ok_or_else(|| io::Error::other("Official ACP server returned an invalid session ID"))?
        .to_owned();
    if let Some(model) = &options.model {
        let model = advertised_model(runtime, &result, model)?;
        // Reapply explicit intent on both new and load; CLI flags alone do not
        // override the model stored by a resumed ACP session.
        if runtime == ActorRuntime::Devin {
            protocol
                .request(
                    "session/set_config_option",
                    json!({"sessionId":id,"configId":"model","value":model}),
                    HANDSHAKE_TIMEOUT,
                )
                .await
                .map_err(|error| io::Error::other(format!("Devin ACP model selection failed: {error}. Use an exact model ID or advertised name from `devin models`; terminal fuzzy aliases are not ACP model IDs")))?;
        } else if runtime == ActorRuntime::Cursor {
            protocol
                .request(
                    "session/set_model",
                    json!({"sessionId":id,"modelId":model}),
                    HANDSHAKE_TIMEOUT,
                )
                .await?;
        }
    }
    if let Some(mode) = &options.mode {
        let mode = if runtime == ActorRuntime::Copilot {
            format!("https://agentclientprotocol.com/protocol/session-modes#{mode}")
        } else {
            mode.clone()
        };
        protocol
            .request(
                "session/set_mode",
                json!({"sessionId":id,"modeId":mode}),
                HANDSHAKE_TIMEOUT,
            )
            .await?;
    }
    // Copilot stores this setting in session history; reapply it on load too.
    if runtime == ActorRuntime::Copilot {
        protocol.request("session/set_config_option",json!({"sessionId":id,"configId":"allow_all","value":if options.unrestricted {"on"} else {"off"}}),HANDSHAKE_TIMEOUT).await?;
    }
    Ok((id, resume.is_some()))
}

fn advertised_model<'a>(
    runtime: ActorRuntime,
    result: &'a Value,
    requested: &'a str,
) -> io::Result<&'a str> {
    let choices: Vec<(&str, &str)> = match runtime {
        ActorRuntime::Cursor => result
            .pointer("/models/availableModels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|choice| Some((choice["modelId"].as_str()?, choice["name"].as_str()?)))
            .collect(),
        ActorRuntime::Devin => result["configOptions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|option| option["id"] == "model")
            .filter_map(|option| option["options"].as_array())
            .flatten()
            .filter_map(|choice| Some((choice["value"].as_str()?, choice["name"].as_str()?)))
            .collect(),
        _ => return Ok(requested),
    };
    if choices.iter().any(|(id, _)| *id == requested) {
        return Ok(requested);
    }
    let mut named = choices
        .iter()
        .filter(|(_, name)| name.eq_ignore_ascii_case(requested));
    if let Some((id, _)) = named.next() {
        if named.any(|(other, _)| other != id) {
            return Err(invalid(
                "ACP model name is ambiguous; use an exact model ID",
            ));
        }
        return Ok(id);
    }
    // The provider remains authoritative, including when it omits its catalog.
    // Never substitute the default model for unrecognized explicit intent.
    Ok(requested)
}

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_models_use_advertised_ids_without_guessing_or_defaulting() {
        let cursor = json!({"models":{"availableModels":[
            {"modelId":"default[]","name":"Auto"},
            {"modelId":"grok-4.7[reasoning_effort=high]","name":"grok-4.7"}
        ]}});
        assert_eq!(
            advertised_model(ActorRuntime::Cursor, &cursor, "grok-4.7")
                .expect("valid model selection"),
            "grok-4.7[reasoning_effort=high]"
        );
        assert_eq!(
            advertised_model(ActorRuntime::Cursor, &cursor, "default[]")
                .expect("valid model selection"),
            "default[]"
        );
        assert_eq!(
            advertised_model(ActorRuntime::Cursor, &cursor, "unknown")
                .expect("valid model selection"),
            "unknown"
        );
        let devin = json!({"configOptions":[{"id":"model","options":[
            {"value":"claude-sonnet-medium","name":"Claude Sonnet"},
            {"value":"a","name":"Repeated"},{"value":"b","name":"Repeated"}
        ]}]});
        assert_eq!(
            advertised_model(ActorRuntime::Devin, &devin, "Claude Sonnet")
                .expect("valid model selection"),
            "claude-sonnet-medium"
        );
        assert_eq!(
            advertised_model(ActorRuntime::Devin, &devin, "sonnet").expect("valid model selection"),
            "sonnet"
        );
        assert!(advertised_model(ActorRuntime::Devin, &devin, "Repeated").is_err());
        for (runtime, program) in [
            (ActorRuntime::Cursor, "cursor-agent"),
            (ActorRuntime::Devin, "devin"),
        ] {
            let intent = options(runtime, &[program.into(), "--model=exact-id".into()])
                .expect("valid model selection");
            assert_eq!(intent.model.as_deref(), Some("exact-id"));
            assert!(!intent.command.iter().any(|arg| arg.starts_with("--model")));
        }
    }
    #[test]
    fn defaults_keep_native_autonomy_and_explicit_commands_keep_their_policy() {
        for (runtime, program) in [
            (ActorRuntime::Copilot, "copilot"),
            (ActorRuntime::Devin, "devin"),
            (ActorRuntime::Cursor, "cursor-agent"),
        ] {
            assert!(options(runtime, &[]).expect("default").unrestricted);
            assert!(
                !options(runtime, &[program.into()])
                    .expect("explicit interactive")
                    .unrestricted
            );
            for forbidden in [
                "--resume",
                "--prompt",
                "--port",
                "--additional-mcp-config",
                "--workspace",
            ] {
                assert!(
                    options(runtime, &[program.into(), forbidden.into(), "value".into()]).is_err()
                );
            }
            assert!(options(runtime, &["sh".into(), program.into()]).is_err());
        }
        let copilot = options(
            ActorRuntime::Copilot,
            &["copilot".into(), "--no-auto-update".into()],
        )
        .expect("options");
        assert_eq!(
            copilot
                .command
                .iter()
                .filter(|arg| *arg == "--no-auto-update")
                .count(),
            1
        );
    }
}
