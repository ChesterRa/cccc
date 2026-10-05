use super::{AcpClient, acp};
use serde_json::{Value, json};
use std::io;
use std::path::Path;
use std::time::Duration;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(40);

pub(super) struct Options {
    pub model: Option<String>,
    pub unrestricted: bool,
}

pub(super) fn options(command: &[String]) -> io::Result<Options> {
    // Empty is the persisted use-default sentinel, shared with Actor commands.
    let default_command = cccc_runtime::default_command(cccc_contracts::ActorRuntime::Antigravity);
    let command = if command.is_empty() {
        default_command.as_slice()
    } else {
        command
    };
    let mut result = Options {
        model: None,
        unrestricted: false,
    };
    if let Some(program) = command.first() {
        let stem = Path::new(program)
            .file_stem()
            .and_then(|value| value.to_str());
        if stem != Some("agy") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Antigravity ACP command must start with agy; wrappers and TUI subcommands are not supported",
            ));
        }
    }
    let mut args = command.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dangerously-skip-permissions" => result.unrestricted = true,
            "--model" | "-m" => {
                result.model = Some(
                    args.next()
                        .filter(|value| !value.trim().is_empty() && !value.starts_with('-'))
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "Antigravity --model requires a model ID",
                            )
                        })?
                        .clone(),
                )
            }
            _ if arg.starts_with("--model=") && arg.len() > "--model=".len() => {
                result.model = Some(arg["--model=".len()..].to_owned())
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unsupported Antigravity ACP argument; use agy [--model MODEL] [--dangerously-skip-permissions]",
                ));
            }
        }
    }
    Ok(result)
}

pub(super) async fn initialize(
    protocol: &AcpClient,
    cwd: &Path,
    resume: Option<&str>,
    mcp: Value,
    options: &Options,
) -> io::Result<(String, bool)> {
    protocol.request("initialize", json!({"protocolVersion":1,"clientInfo":{"name":"cccc","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false}}), HANDSHAKE_TIMEOUT).await?;
    let mut params = json!({"cwd":cwd.to_string_lossy(),"mcpServers":[mcp]});
    let method = if let Some(id) = resume {
        params["sessionId"] = json!(id);
        "session/load"
    } else {
        "session/new"
    };
    let result = protocol.request(method, params, HANDSHAKE_TIMEOUT).await.map_err(|error| io::Error::new(error.kind(), format!("Antigravity ACP session initialization failed. Complete separate ACP login with `cccc setup --runtime antigravity --runtime-mode acp --login`. {error}")))?;
    let id = if let Some(id) = resume {
        id.to_owned()
    } else {
        result
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|value| uuid::Uuid::parse_str(value).is_ok())
            .ok_or_else(|| {
                io::Error::other("Official Antigravity ACP returned an invalid session ID")
            })?
            .to_owned()
    };
    // Official 1.3.0 resets model/mode on load. Reapply configured intent for both paths.
    if let Some(model) = &options.model {
        set_option(protocol, &id, "model", model).await?;
    }
    set_option(
        protocol,
        &id,
        "mode",
        if options.unrestricted {
            "yolo"
        } else {
            "default"
        },
    )
    .await?;
    Ok((id, resume.is_some()))
}

async fn set_option(protocol: &AcpClient, id: &str, option: &str, value: &str) -> io::Result<()> {
    protocol
        .request(
            "session/set_config_option",
            json!({"sessionId":id,"configId":option,"value":value}),
            HANDSHAKE_TIMEOUT,
        )
        .await
        .map(|_| ())
}

pub(super) fn permission_policy(options: &Options) -> acp::PermissionPolicy {
    if options.unrestricted {
        acp::PermissionPolicy::AllowOnce
    } else {
        acp::PermissionPolicy::Interactive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_mapped_native_intent() {
        let args = [
            "agy",
            "--model=gemini-test",
            "--dangerously-skip-permissions",
        ]
        .map(str::to_owned);
        let options = options(&args).expect("fixture operation");
        assert_eq!(options.model.as_deref(), Some("gemini-test"));
        assert!(options.unrestricted);
        for args in [
            vec!["agy", "--resume", "id"],
            vec!["agy", "--model"],
            vec!["wrapper", "agy"],
        ] {
            assert!(
                self::options(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err()
            );
        }
    }
}
