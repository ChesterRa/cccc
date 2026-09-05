use super::super::{SessionPurpose, launch::ANALYST_INSTRUCTIONS, launch_command};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

mod network_environment;

const MIN_VERSION: (u64, u64, u64) = (2, 1, 259);
const VERSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub(super) type Version = (u64, u64, u64);

pub(in crate::ops::codex_voice_analyst) struct PreparedClaude {
    pub(super) executable: String,
    pub(super) arguments: Vec<String>,
    pub(super) launch_environment: BTreeMap<String, String>,
    pub(super) config_dir: PathBuf,
    #[cfg(test)]
    pub(super) settings_path: PathBuf,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::ops::codex_voice_analyst) fn prepare(
    home: &HomeLayout,
    configured: &[String],
    environment: &BTreeMap<String, String>,
    cwd: &Path,
    settings_owner: &str,
    purpose: SessionPurpose,
    mcp_server: Value,
) -> io::Result<PreparedClaude> {
    let default_command = ["claude".to_owned()];
    let configured = if configured.is_empty() {
        &default_command[..]
    } else {
        configured
    };
    let executable = launch_command::resolve_runtime_executable(&configured[0], environment)?;
    let filename = executable
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !direct_claude_filename(&filename, cfg!(windows)) {
        return invalid(
            "Claude managed sessions require the direct claude executable; wrappers and renamed binaries are not supported",
        );
    }
    let parsed = parse_arguments(&configured[1..])?;
    let mut settings = match parsed.settings {
        Some(value) => load_settings(&value, cwd)?,
        None => Map::new(),
    };
    merge_environment(&mut settings, environment)?;
    network_environment::inherit(&mut settings, std::env::vars())?;
    let config_dir = config_dir(environment)?;
    let settings_root = home.daemon_dir().join("claude-managed");
    std::fs::create_dir_all(&settings_root)?;
    set_private_directory(&settings_root)?;
    let settings_digest = format!("{:x}", Sha256::digest(settings_owner.as_bytes()));
    // Agent View stores this path in the durable session's respawn metadata.
    // Keep one stable, owner-scoped file so stop/start can resume the same
    // session while still allowing actor removal to erase the private copy.
    let settings_path = settings_root.join(format!("{}.json", &settings_digest[..24]));
    let mut launch_environment = launcher_environment(environment);
    network_environment::extend_launcher(&mut launch_environment, &settings);
    cccc_core::fs::write_secret_json(&settings_path, &Value::Object(settings))?;

    let mut arguments = parsed.arguments;
    arguments.extend([
        "--settings".into(),
        settings_path.to_string_lossy().into_owned(),
        "--mcp-config".into(),
        json!({"mcpServers":{"cccc":mcp_server}}).to_string(),
    ]);
    if purpose == SessionPurpose::VoiceAnalyst {
        arguments.extend(["--append-system-prompt".into(), ANALYST_INSTRUCTIONS.into()]);
    }
    arguments.push("--dangerously-skip-permissions".into());

    Ok(PreparedClaude {
        executable: executable.to_string_lossy().into_owned(),
        arguments,
        launch_environment,
        config_dir,
        #[cfg(test)]
        settings_path,
    })
}

pub(super) fn remove_settings_owner(home: &HomeLayout, settings_owner: &str) -> io::Result<()> {
    let digest = format!("{:x}", Sha256::digest(settings_owner.as_bytes()));
    let root = home.daemon_dir().join("claude-managed");
    remove_file_if_present(&root.join(format!("{}.json", &digest[..24])))?;
    let path = root.join(&digest[..24]);
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) async fn require_supported_version(
    executable: &str,
    cwd: &Path,
    environment: &BTreeMap<String, String>,
) -> io::Result<Version> {
    let mut command = process_command(executable, &["--version".into()], environment)?;
    command
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .kill_on_drop(true);
    let output = tokio::time::timeout(VERSION_TIMEOUT, command.output())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Claude version probe timed out"))??;
    if !output.status.success() {
        return invalid("Claude Code version could not be verified for an Agent View session");
    }
    let version = parse_version(&String::from_utf8_lossy(&output.stdout))
        .or_else(|| parse_version(&String::from_utf8_lossy(&output.stderr)))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "Claude Code returned an unrecognized version for an Agent View session",
            )
        })?;
    if version < MIN_VERSION {
        return invalid(format!(
            "Claude Code {}.{}.{} cannot host the verified managed session; upgrade to 2.1.259 or newer",
            version.0, version.1, version.2
        ));
    }
    Ok(version)
}

pub(super) fn process_command(
    executable: &str,
    arguments: &[String],
    environment: &BTreeMap<String, String>,
) -> io::Result<tokio::process::Command> {
    let mut command = Vec::with_capacity(arguments.len() + 1);
    command.push(executable.to_owned());
    command.extend_from_slice(arguments);
    let command = cccc_runtime::prepare_pty_command(&command, environment);
    let (program, arguments) = command.split_first().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Claude managed-session command is empty",
        )
    })?;
    let mut process = tokio::process::Command::new(program);
    process.args(arguments);
    Ok(process)
}

fn direct_claude_filename(filename: &str, windows: bool) -> bool {
    matches!(filename, "claude" | "claude.exe")
        || windows && matches!(filename, "claude.cmd" | "claude.bat")
}

#[derive(Default)]
struct ParsedArguments {
    arguments: Vec<String>,
    settings: Option<String>,
}

fn parse_arguments(arguments: &[String]) -> io::Result<ParsedArguments> {
    const VALUES: &[&str] = &[
        "--model",
        "-m",
        "--agent",
        "--agents",
        "--routine",
        "--effort",
        "--autocompact",
        "--fallback-model",
        "--advisor",
        "--debug-file",
        "--json-schema",
        "--max-budget-usd",
        "--max-thinking-tokens",
        "--max-turns",
        "--task-budget",
        "--plan-mode-instructions",
        "--rewind-files",
        "--thinking",
        "--thinking-display",
        "--setting-sources",
        "--system-prompt",
        "--system-prompt-file",
        "--append-system-prompt",
        "--append-system-prompt-file",
        "--append-subagent-system-prompt",
        "--plugin-dir",
        "--plugin-dir-no-mcp",
        "--plugin-url",
    ];
    const MULTI_VALUES: &[&str] = &[
        "--add-dir",
        "--allowed-tools",
        "--allowedTools",
        "--disallowed-tools",
        "--disallowedTools",
        "--tools",
        "--mcp-config",
        "--betas",
        "--file",
        "--channels",
    ];
    const FLAGS: &[&str] = &[
        "--bare",
        "--brief",
        "--chrome",
        "--no-chrome",
        "--ide",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--verbose",
        "--ax-screen-reader",
        "--exclude-dynamic-system-prompt-sections",
        "--dangerously-allow-browser-network-access",
    ];
    const STRIPPED_FLAGS: &[&str] = &[
        "--dangerously-skip-permissions",
        "--allow-dangerously-skip-permissions",
    ];
    const OWNED_VALUES: &[&str] = &[
        "--resume",
        "-r",
        "--session-id",
        "--name",
        "-n",
        "--permission-mode",
        "--permission-prompts",
        "--input-format",
        "--output-format",
        "--remote-control-session-name-prefix",
        "--cloud",
        "--environment",
        "--exec",
    ];
    const OWNED_FLAGS: &[&str] = &[
        "--bg",
        "--background",
        "--continue",
        "-c",
        "--fork-session",
        "--remote-control",
        "--rc",
        "--print",
        "-p",
        "--include-hook-events",
        "--include-partial-messages",
        "--replay-user-messages",
        "--no-session-persistence",
        "--reply-on-resume",
    ];

    let mut parsed = ParsedArguments::default();
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "--" || !argument.starts_with('-') {
            return invalid(format!(
                "Claude runtime command contains a prompt, subcommand, or positional argument that CCCC cannot own safely: {argument}"
            ));
        }
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument, None), |(flag, value)| (flag, Some(value)));
        if flag == "--settings" {
            let value = inline
                .map(str::to_owned)
                .or_else(|| arguments.get(index + 1).cloned());
            let value = value
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--settings requires a value")
                })?;
            if parsed.settings.replace(value).is_some() {
                return invalid("Claude runtime command may contain only one --settings source");
            }
            index += if inline.is_some() { 1 } else { 2 };
            continue;
        }
        if STRIPPED_FLAGS.contains(&flag) {
            if inline.is_some() {
                return invalid(format!("Claude flag {flag} does not accept a value"));
            }
            index += 1;
            continue;
        }
        if flag == "--restricted" {
            return invalid(
                "Claude --restricted refuses the CCCC-owned YOLO permission mode and cannot be used by a managed session",
            );
        }
        if OWNED_FLAGS.contains(&flag) || OWNED_VALUES.contains(&flag) {
            return invalid(format!(
                "Claude runtime command contains a CCCC-owned session or transport argument: {flag}"
            ));
        }
        if FLAGS.contains(&flag) {
            if inline.is_some() {
                return invalid(format!("Claude flag {flag} does not accept a value"));
            }
            parsed.arguments.push(argument.to_owned());
            index += 1;
            continue;
        }
        if VALUES.contains(&flag) {
            if inline.is_some() {
                parsed.arguments.push(argument.to_owned());
                index += 1;
            } else {
                let value = required_next(arguments, index, flag)?;
                parsed.arguments.extend([flag.to_owned(), value.to_owned()]);
                index += 2;
            }
            continue;
        }
        if MULTI_VALUES.contains(&flag) {
            parsed.arguments.push(argument.to_owned());
            if inline.is_some() {
                index += 1;
                continue;
            }
            let first = required_next(arguments, index, flag)?;
            parsed.arguments.push(first.to_owned());
            index += 2;
            while let Some(value) = arguments.get(index).filter(|value| !value.starts_with('-')) {
                parsed.arguments.push(value.clone());
                index += 1;
            }
            continue;
        }
        if flag == "--debug" || flag == "-d" || flag == "--prompt-suggestions" {
            parsed.arguments.push(argument.to_owned());
            if inline.is_none()
                && let Some(value) = arguments
                    .get(index + 1)
                    .filter(|value| !value.starts_with('-'))
            {
                parsed.arguments.push(value.clone());
                index += 1;
            }
            index += 1;
            continue;
        }
        return invalid(format!(
            "Claude runtime command contains an unsupported managed-session argument: {argument}"
        ));
    }
    Ok(parsed)
}

fn required_next<'a>(arguments: &'a [String], index: usize, flag: &str) -> io::Result<&'a str> {
    arguments
        .get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty() && !value.starts_with('-'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Claude runtime command flag {flag} requires a value"),
            )
        })
}

fn load_settings(value: &str, cwd: &Path) -> io::Result<Map<String, Value>> {
    let source = if value.trim_start().starts_with('{') {
        value.to_owned()
    } else {
        let path = PathBuf::from(value);
        let path = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        std::fs::read_to_string(&path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to read Claude settings {}: {error}", path.display()),
            )
        })?
    };
    serde_json::from_str::<Value>(&source)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .as_object()
        .cloned()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Claude settings must be a JSON object",
            )
        })
}

fn merge_environment(
    settings: &mut Map<String, Value>,
    environment: &BTreeMap<String, String>,
) -> io::Result<()> {
    let value = settings
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()));
    if value.is_null() {
        *value = Value::Object(Map::new());
    }
    let target = value.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Claude settings env must be an object",
        )
    })?;
    for (key, value) in environment {
        target.insert(key.clone(), Value::String(value.clone()));
    }
    Ok(())
}

fn launcher_environment(environment: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    const PRELAUNCH_KEYS: &[&str] = &[
        "PATH",
        "HOME",
        "USERPROFILE",
        "CLAUDE_CONFIG_DIR",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "SHELL",
        "USER",
        "LOGNAME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "SystemRoot",
        "ComSpec",
        "PATHEXT",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
    ];
    PRELAUNCH_KEYS
        .iter()
        .filter_map(|key| {
            environment
                .get(*key)
                .cloned()
                .or_else(|| std::env::var_os(key).map(|value| value.to_string_lossy().into_owned()))
                .map(|value| ((*key).to_owned(), value))
        })
        .collect()
}

fn config_dir(environment: &BTreeMap<String, String>) -> io::Result<PathBuf> {
    let configured = environment.get("CLAUDE_CONFIG_DIR").cloned().or_else(|| {
        environment
            .get("HOME")
            .or_else(|| environment.get("USERPROFILE"))
            .map(|home| {
                Path::new(home)
                    .join(".claude")
                    .to_string_lossy()
                    .into_owned()
            })
    });
    let configured = configured.or_else(|| {
        std::env::var_os("CLAUDE_CONFIG_DIR").map(|value| value.to_string_lossy().into_owned())
    });
    let configured = configured.or_else(|| {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| {
                Path::new(&home)
                    .join(".claude")
                    .to_string_lossy()
                    .into_owned()
            })
    });
    let value = configured.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Claude config directory cannot be resolved without CLAUDE_CONFIG_DIR, HOME, or USERPROFILE",
        )
    })?;
    let path = cccc_core::path_input::expand_user_path(&value)?;
    if !path.is_absolute() {
        return invalid("CLAUDE_CONFIG_DIR must resolve to an absolute path");
    }
    std::fs::create_dir_all(&path)?;
    path.canonicalize()
}

pub(super) fn parse_version(text: &str) -> Option<Version> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word
            .trim_matches(|character: char| !character.is_ascii_digit() && character != '.')
            .split('.');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    })
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_runtime_configuration_but_rejects_session_ownership() {
        let parsed = parse_arguments(&[
            "--model".into(),
            "opus".into(),
            "--effort=max".into(),
            "--add-dir".into(),
            "/one".into(),
            "/two".into(),
            "--dangerously-skip-permissions".into(),
        ])
        .expect("supported arguments");
        assert_eq!(
            parsed.arguments,
            [
                "--model",
                "opus",
                "--effort=max",
                "--add-dir",
                "/one",
                "/two"
            ]
        );
        for arguments in [
            vec!["--bg".into()],
            vec!["--resume".into(), "session".into()],
            vec!["-p".into()],
            vec!["--restricted".into()],
            vec!["attach".into(), "session".into()],
            vec!["hello".into()],
        ] {
            assert!(
                parse_arguments(&arguments).is_err(),
                "accepted {arguments:?}"
            );
        }
    }

    #[test]
    fn merges_profile_environment_without_overwriting_other_settings() {
        let mut settings = serde_json::from_value::<Map<String, Value>>(json!({
            "model":"sonnet",
            "env":{"EXISTING":"kept","TOKEN":"old"}
        }))
        .expect("settings object");
        merge_environment(
            &mut settings,
            &BTreeMap::from([
                ("TOKEN".into(), "private".into()),
                ("EXTRA".into(), "value".into()),
            ]),
        )
        .expect("merge environment");
        assert_eq!(settings["model"], "sonnet");
        assert_eq!(settings["env"]["EXISTING"], "kept");
        assert_eq!(settings["env"]["TOKEN"], "private");
        assert_eq!(settings["env"]["EXTRA"], "value");
    }

    #[test]
    fn parses_verified_agent_view_version() {
        assert_eq!(parse_version("2.1.259 (Claude Code)"), Some(MIN_VERSION));
        assert_eq!(parse_version("Claude Code v2.2.0"), Some((2, 2, 0)));
        assert_eq!(parse_version("unknown"), None);
    }

    #[test]
    fn accepts_only_direct_platform_claude_launchers() {
        assert!(direct_claude_filename("claude", false));
        assert!(direct_claude_filename("claude.exe", true));
        assert!(direct_claude_filename("claude.cmd", true));
        assert!(direct_claude_filename("claude.bat", true));
        assert!(!direct_claude_filename("claude.cmd", false));
        assert!(!direct_claude_filename("my-claude", true));
    }

    #[test]
    fn actor_settings_cleanup_removes_stable_and_previous_generation_copies() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let owner = "actor:g_demo:claude-1";
        let digest = format!("{:x}", Sha256::digest(owner.as_bytes()));
        let root = home.daemon_dir().join("claude-managed");
        let generation_dir = root.join(&digest[..24]);
        std::fs::create_dir_all(&generation_dir).expect("generation directory");
        std::fs::write(generation_dir.join("generation.json"), "secret")
            .expect("generation settings");
        let legacy = root.join(format!("{}.json", &digest[..24]));
        std::fs::write(&legacy, "legacy secret").expect("legacy settings");

        remove_settings_owner(&home, owner).expect("remove settings owner");

        assert!(!generation_dir.exists());
        assert!(!legacy.exists());
    }

    #[test]
    fn managed_settings_path_is_stable_across_process_generations() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let executable = temp.path().join(if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        });
        std::fs::write(&executable, "fixture").expect("fake Claude executable");
        let command = vec![executable.to_string_lossy().into_owned()];
        let environment = BTreeMap::from([("PRIVATE_TOKEN".into(), "secret".into())]);
        let first = prepare(
            &home,
            &command,
            &environment,
            temp.path(),
            "actor:g1:claude-1",
            SessionPurpose::Actor,
            json!({}),
        )
        .expect("first settings");
        let second = prepare(
            &home,
            &command,
            &environment,
            temp.path(),
            "actor:g1:claude-1",
            SessionPurpose::Actor,
            json!({}),
        )
        .expect("second settings");
        assert_eq!(first.settings_path, second.settings_path);
        assert!(first.settings_path.is_file());
        remove_settings_owner(&home, "actor:g1:claude-1").expect("remove owner settings");
        assert!(!first.settings_path.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_an_outdated_agent_view_before_session_launch() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp.path().join("claude");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '2.1.258 (Claude Code)\\n'\n",
        )
        .expect("fake Claude");
        let mut permissions = executable.metadata().expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("permissions");

        let error =
            require_supported_version(&executable.to_string_lossy(), temp.path(), &BTreeMap::new())
                .await
                .expect_err("outdated Agent View must fail closed");
        assert!(error.to_string().contains("upgrade to 2.1.259 or newer"));
    }
}
