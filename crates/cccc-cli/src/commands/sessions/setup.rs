use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use cccc_core::{fs, local_sessions::powershell_quote};
use std::process::Command;

#[path = "setup_opencode.rs"]
mod opencode;

#[path = "setup_inspect.rs"]
mod inspection;
pub(crate) use inspection::{SetupInspection, inspect};

const MARKER: &str = "CCCC local session connection";
const PLUGIN: &str = "cccc-local-sessions@cccc-local";

const RESTART_REQUIRED: &str = "This running CCCC executable was replaced or removed. Restart CCCC before running setup again. No provider configuration was changed";

fn running_executable() -> Result<PathBuf> {
    let executable = std::env::current_exe().context(RESTART_REQUIRED)?;
    // In particular, Linux current_exe retains a deleted inode's name after
    // an atomic upgrade. Never install that name or guess its replacement.
    anyhow::ensure!(executable.is_file(), RESTART_REQUIRED);
    Ok(executable)
}

#[derive(Clone, Debug)]
pub(crate) struct SetupResult {
    pub provider_dir: PathBuf,
    pub output: Value,
    pub warnings: Vec<String>,
}

impl SetupResult {
    pub(super) fn print(self) -> Result<()> {
        for warning in self.warnings {
            eprintln!("{warning}");
        }
        println!("{}", serde_json::to_string_pretty(&self.output)?);
        Ok(())
    }
}

fn pi_mcp(home: &HomeLayout, executable: &std::path::Path) -> Value {
    json!({"command":executable,"args":["mcp","--local-session-runtime","pi"],
        "env":{"CCCC_HOME":home.root()},"exposure":"direct"})
}

async fn configure_pi(home: &HomeLayout, enabled: bool) -> Result<SetupResult> {
    platform()?;
    home.initialize()?;
    if !enabled {
        local_sessions::set_integration_disabled(home, LocalSessionRuntime::Pi, true)?;
    }
    version("pi").await?;
    let executable = crate::commands::setup::public_executable()?;
    let provider_home = native_home("PI_CODING_AGENT_DIR", ".pi/agent")?;
    let mcp_path = provider_home.join("mcp.json");
    let config = match fs::read_json::<Value>(&mcp_path) {
        Ok(config) => config,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error.into()),
    };
    let existing = config
        .pointer("/mcpServers/cccc")
        .filter(|value| !value.is_null());
    let expected = pi_mcp(home, &executable);
    let mut warnings = Vec::new();
    if enabled {
        if let Some(entry) = existing {
            anyhow::ensure!(
                entry["command"] == expected["command"]
                    && entry["env"]["CCCC_HOME"] == expected["env"]["CCCC_HOME"]
                    && ![
                        "CCCC_ACTOR_ID",
                        "CCCC_SECRETARY_TASK_TOKEN",
                        "CCCC_SECRETARY_TASK_TOKEN_FILE",
                    ]
                    .iter()
                    .any(|key| entry["env"][key].as_str().is_some_and(|v| !v.is_empty()))
                    && entry["env"]["CCCC_MCP_TOOL_PROFILE"] != "secretary-task",
                "Pi has a conflicting cccc MCP entry (another installation or scoped identity); resolve it in Pi before enabling native access"
            );
        }
        local_sessions::set_integration_disabled(home, LocalSessionRuntime::Pi, true)?;
        let plugin = home.root().join("integrations/pi");
        std::fs::create_dir_all(&plugin)?;
        let source = include_str!("../../../resources/native/pi.ts")
            .replace("__CCCC_EXECUTABLE__", &serde_json::to_string(&executable)?)
            .replace("__CCCC_HOME__", &serde_json::to_string(home.root())?);
        fs::atomic_write(&plugin.join("index.ts"), source.as_bytes())?;
        fs::write_json(
            &plugin.join("package.json"),
            &json!({
                "name":"@cccc/native-pi","version":env!("CARGO_PKG_VERSION"),"type":"module",
                "pi":{"extensions":["./index.ts"]},
            }),
        )?;
        checked(
            "pi",
            &[
                "install".into(),
                plugin.to_string_lossy().into_owned(),
                "--no-approve".into(),
            ],
        )
        .await?;
        checked(
            "pi",
            &[
                "mcp".into(),
                "add".into(),
                "cccc".into(),
                "--exposure".into(),
                "direct".into(),
                "--env".into(),
                format!("CCCC_HOME={}", home.root().display()),
                "--".into(),
                executable.to_string_lossy().into_owned(),
                "mcp".into(),
                "--local-session-runtime".into(),
                "pi".into(),
            ],
        )
        .await?;
        local_sessions::set_integration_disabled(home, LocalSessionRuntime::Pi, false)?;
    } else {
        let plugin = home.root().join("integrations/pi");
        let settings: Value = match fs::read_json(&provider_home.join("settings.json")) {
            Ok(settings) => settings,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
            Err(error) => return Err(error.into()),
        };
        let installed = settings["packages"].as_array().is_some_and(|packages| {
            packages.iter().any(|package| {
                let Some(source) = package.as_str().or_else(|| package["source"].as_str()) else {
                    return false;
                };
                let path = std::path::Path::new(source);
                let path = if path.is_absolute() {
                    path.to_owned()
                } else {
                    provider_home.join(path)
                };
                path.canonicalize()
                    .is_ok_and(|path| plugin.canonicalize().is_ok_and(|plugin| path == plugin))
            })
        });
        if installed {
            checked(
                "pi",
                &[
                    "remove".into(),
                    plugin.to_string_lossy().into_owned(),
                    "--no-approve".into(),
                ],
            )
            .await?;
        }
        if existing == Some(&expected) {
            checked("pi", &["mcp".into(), "remove".into(), "cccc".into()]).await?;
        } else if existing.is_some() {
            warnings.push(
                "Native Pi access is disabled; the modified cccc MCP entry was preserved.".into(),
            );
        }
    }
    Ok(SetupResult {
        provider_dir: provider_home,
        output: json!({"runtime":"pi","enabled":enabled,
        "note":if enabled {"Configuration saved. Run /reload in existing Pi sessions, or start Pi normally. Its native extension and MCP server load together; CCCC does not own the Pi terminal."} else {"Native access is disabled. Run /reload in Pi to unload the extension and MCP server. Original sessions are retained."}}),
        warnings,
    })
}

fn platform() -> Result<()> {
    NativeHost::capture(std::process::id()).context("Native process identity is unavailable")?;
    Ok(())
}

async fn invoke(program: &str, args: &[String]) -> Result<cccc_runtime::CapturedOutput> {
    let program =
        cccc_runtime::resolve_executable_in_path(program, std::env::var("PATH").ok().as_deref())
            .context("Native CLI is unavailable")?;
    cccc_runtime::capture_command(
        Command::new(program).args(args),
        None,
        Duration::from_secs(30),
        32_768,
    )
    .await
    .map_err(Into::into)
}

async fn checked(program: &str, args: &[String]) -> Result<()> {
    let output = invoke(program, args).await?;
    anyhow::ensure!(
        output.status.success(),
        "Native {program} configuration failed; check its CLI configuration before retrying"
    );
    Ok(())
}

async fn version(runtime: &str) -> Result<String> {
    let output = invoke(runtime, &["--version".into()]).await?;
    anyhow::ensure!(
        output.status.success() && !output.stdout_truncated,
        "Native CLI version is unavailable"
    );
    let text = String::from_utf8(output.stdout)?;
    let version = text
        .split_whitespace()
        .find_map(|word| semver::Version::parse(word.trim_start_matches('v')).ok())
        .context("Native CLI version is unavailable")?;
    let minimum = semver::Version::parse(match runtime {
        "codex" => "0.163.0",
        "pi" => "1.4.0",
        "opencode" => "2.1.4",
        "claude" => "2.1.306",
        _ => bail!("Unsupported native runtime"),
    })?;
    anyhow::ensure!(
        version >= minimum,
        "{runtime} {minimum} or newer is required for native session integration"
    );
    Ok(version.to_string())
}

fn prefix(home: &HomeLayout, executable: &std::path::Path) -> String {
    format!(
        "export CCCC_HOME={}; exec {}",
        shell_words::quote(&home.root().to_string_lossy()),
        shell_words::quote(&executable.to_string_lossy())
    )
}

fn windows_command(script: &str) -> String {
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    // EncodedCommand is one shell-neutral argv token, including paths with
    // spaces, quotes, Unicode or metacharacters in the generated script.
    format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(bytes)
    )
}

fn claude_native_mcp(home: &HomeLayout, executable: &std::path::Path) -> Value {
    json!({"type":"stdio","command":executable,"args":["mcp","--local-session-runtime","claude"],"env":{"CCCC_HOME":home.root()}})
}

fn claude_user_config_path() -> Result<PathBuf> {
    let custom =
        std::env::var_os("CLAUDE_CODE_CUSTOM_OAUTH_URL").is_some_and(|value| !value.is_empty());
    let filename = if custom {
        ".claude-custom-oauth.json"
    } else {
        ".claude.json"
    };
    let (directory, modern) = if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        let directory = PathBuf::from(path);
        anyhow::ensure!(
            directory.is_absolute(),
            "Automatic Claude MCP cleanup requires an absolute, nonempty CLAUDE_CONFIG_DIR"
        );
        let modern = directory.join(filename);
        (directory, modern)
    } else {
        let user_home = std::env::home_dir().context("User home is unavailable")?;
        (user_home.join(".claude"), user_home.join(filename))
    };
    let legacy = directory.join(".config.json");
    match std::fs::metadata(&legacy) {
        Ok(metadata) if metadata.is_file() => {
            #[cfg(unix)]
            let writable = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o222 != 0
            };
            #[cfg(not(unix))]
            let writable = !metadata.permissions().readonly();
            // Claude 2.1.306+ prefers a nonempty legacy file, or a writable
            // empty legacy file when the modern file does not exist.
            if metadata.len() > 0 || (writable && !modern.try_exists()?) {
                return Ok(legacy);
            }
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(modern)
}

fn claude_user_mcp(path: &std::path::Path) -> Result<Option<Value>> {
    let config: Value = match fs::read_json(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let config = config
        .as_object()
        .context("Claude user configuration must be an object")?;
    let Some(servers) = config.get("mcpServers") else {
        return Ok(None);
    };
    let servers = servers
        .as_object()
        .context("Claude user MCP configuration must be an object")?;
    Ok(servers.get("cccc").cloned())
}

async fn remove_claude_native_mcp(
    home: &HomeLayout,
    executable: &std::path::Path,
) -> Result<Vec<String>> {
    // `claude mcp get/list` can start servers and merges configuration scopes.
    // Inspect only the effective user file, without printing its private data.
    let path = claude_user_config_path()?;
    let Some(entry) = claude_user_mcp(&path)? else {
        return Ok(Vec::new());
    };
    if entry != claude_native_mcp(home, executable) {
        return Ok(vec!["Native integration disabled; the user MCP entry was left unchanged because it is not this installation's unmodified native registration".into()]);
    }
    // The official remove command has no compare-and-remove operation. Do not
    // hold Claude's file lock while invoking it or retry against a changed entry.
    invoke(
        "claude",
        &[
            "mcp".into(),
            "remove".into(),
            "--scope".into(),
            "user".into(),
            "cccc".into(),
        ],
    )
    .await?;
    anyhow::ensure!(
        claude_user_config_path()? == path && claude_user_mcp(&path)?.is_none(),
        "Claude user MCP removal was not confirmed; configuration may have changed and no further removal was attempted"
    );
    // Confirmed absence is idempotent even after a nonzero CLI exit. Do not
    // classify an English diagnostic as proof of absence or print provider output.
    Ok(Vec::new())
}

async fn configure_mcp(
    home: &HomeLayout,
    runtime: &str,
    executable: &std::path::Path,
    enabled: bool,
) -> Result<Vec<String>> {
    if runtime == "codex" {
        let mut args = vec![
            "mcp".into(),
            "add".into(),
            "cccc".into(),
            "--env".into(),
            format!("CCCC_HOME={}", home.root().display()),
            "--".into(),
            executable.to_string_lossy().into_owned(),
            "mcp".into(),
        ];
        if enabled {
            args.extend(["--local-session-runtime".into(), "codex".into()]);
        }
        checked(runtime, &args).await?;
        Ok(Vec::new())
    } else {
        if !enabled {
            return remove_claude_native_mcp(home, executable).await;
        }
        let config = claude_native_mcp(home, executable);
        let args = vec![
            "mcp".into(),
            "add-json".into(),
            "--scope".into(),
            "user".into(),
            "cccc".into(),
            serde_json::to_string(&config)?,
        ];
        let output = invoke(runtime, &args).await?;
        if output.status.success() {
            return Ok(Vec::new());
        }
        let detail = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .to_ascii_lowercase();
        anyhow::ensure!(
            detail.contains("already exists"),
            "Native Claude MCP configuration failed; existing configuration was left unchanged"
        );
        checked(
            runtime,
            &[
                "mcp".into(),
                "remove".into(),
                "--scope".into(),
                "user".into(),
                "cccc".into(),
            ],
        )
        .await?;
        checked(runtime, &args).await?;
        Ok(Vec::new())
    }
}

fn codex_hooks(home: &HomeLayout, executable: &std::path::Path, enabled: bool) -> Result<()> {
    let provider_home = std::path::absolute(native_home("CODEX_HOME", ".codex")?)?;
    let path = provider_home.join("hooks.json");
    let mut config: Value = match fs::read_json(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !enabled {
                return Ok(());
            }
            json!({"hooks":{}})
        }
        Err(error) => return Err(error.into()),
    };
    let original = config.clone();
    let hooks = config
        .get_mut("hooks")
        .and_then(Value::as_object_mut)
        .context("Native hooks.json must contain a hooks object")?;
    let owned = codex_hook(home, executable, &provider_home);
    for event in ["SessionStart", "UserPromptSubmit", "SessionEnd"] {
        if !enabled && !hooks.contains_key(event) {
            continue;
        }
        let entries = hooks
            .entry(event)
            .or_insert(json!([]))
            .as_array_mut()
            .context("Native hook events must be arrays")?;
        for entry in entries.iter_mut() {
            if let Some(handlers) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                handlers.retain(|handler| {
                    if enabled {
                        handler["statusMessage"] != MARKER
                    } else {
                        handler != &owned
                    }
                });
            }
        }
        entries.retain(|entry| {
            entry["hooks"]
                .as_array()
                .is_none_or(|handlers| !handlers.is_empty())
        });
        if enabled {
            entries.push(json!({"hooks":[owned]}));
        }
    }
    if config == original {
        return Ok(());
    }
    let mut bytes = serde_json::to_vec_pretty(&config)?;
    bytes.push(b'\n');
    fs::atomic_write_preserving_mode(&path, &bytes)?;
    Ok(())
}

fn codex_hook(
    home: &HomeLayout,
    executable: &std::path::Path,
    provider_home: &std::path::Path,
) -> Value {
    let command = format!(
        "{} sessions hook --runtime codex --provider-home {}",
        prefix(home, executable),
        shell_words::quote(&provider_home.to_string_lossy())
    );
    let command_windows = windows_command(&format!(
        "$utf8 = [System.Text.UTF8Encoding]::new($false)\n[Console]::InputEncoding = $utf8\n$OutputEncoding = $utf8\n$env:CCCC_HOME = {}\n[Console]::In.ReadToEnd() | & {} sessions hook --runtime codex --host-shell-pid $PID --provider-home {}\nexit $LASTEXITCODE",
        powershell_quote(&home.root().to_string_lossy()),
        powershell_quote(&executable.to_string_lossy()),
        powershell_quote(&provider_home.to_string_lossy()),
    ));
    json!({"type":"command","command":command,"commandWindows":command_windows,"timeout":3,"statusMessage":MARKER})
}

async fn codex_mcp_owned(home: &HomeLayout, executable: &std::path::Path) -> Result<bool> {
    let Some(entry) =
        crate::commands::setup::codex_mcp_entry_async(home, &std::env::current_dir()?).await?
    else {
        return Ok(false);
    };
    let transport = &entry["transport"];
    Ok(transport["type"] == "stdio"
        && transport["command"] == json!(executable)
        && transport["args"] == json!(["mcp", "--local-session-runtime", "codex"])
        && transport["env"] == json!({"CCCC_HOME":home.root()})
        && transport["env_vars"].as_array().is_none_or(Vec::is_empty)
        && transport["cwd"].is_null()
        && entry["enabled"] != false
        && [
            "enabled_tools",
            "disabled_tools",
            "startup_timeout_sec",
            "tool_timeout_sec",
        ]
        .iter()
        .all(|key| entry[key].is_null()))
}

async fn claude_registry(marketplaces: bool) -> Result<Vec<Value>> {
    let args = if marketplaces {
        vec!["plugin", "marketplace", "list", "--json"]
    } else {
        vec!["plugin", "list", "--json"]
    };
    let output = invoke(
        "claude",
        &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
    )
    .await?;
    let error = "Could not inspect Claude plugin registration; configuration was left unchanged";
    anyhow::ensure!(
        output.status.success() && !output.stdout_truncated && !output.stderr_truncated,
        "{error}"
    );
    let entries: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| anyhow::anyhow!(error))?;
    let entries = entries.as_array().context(error)?;
    let key = if marketplaces { "name" } else { "id" };
    anyhow::ensure!(
        entries.iter().all(|entry| entry[key].is_string()),
        "{error}"
    );
    Ok(entries.clone())
}

async fn claude_marketplace() -> Result<Option<Value>> {
    let entries = claude_registry(true).await?;
    let mut entries = entries
        .into_iter()
        .filter(|entry| entry["name"] == "cccc-local");
    let entry = entries.next();
    anyhow::ensure!(
        entries.next().is_none(),
        "Claude has conflicting cccc-local marketplace registrations"
    );
    Ok(entry)
}

fn same_path(value: Option<&str>, expected: &std::path::Path) -> bool {
    value.is_some_and(|value| {
        let path = std::path::Path::new(value);
        path == expected
            || std::fs::canonicalize(path)
                .ok()
                .zip(std::fs::canonicalize(expected).ok())
                .is_some_and(|(a, b)| a == b)
    })
}

fn owned_claude_marketplace(home: &HomeLayout, entry: &Value) -> bool {
    entry["source"] == "directory"
        && same_path(
            entry["path"].as_str(),
            &home.root().join("integrations/local-sessions"),
        )
}

async fn claude_plugin_owned(home: &HomeLayout) -> Result<bool> {
    let Some(marketplace) = claude_marketplace().await? else {
        return Ok(false);
    };
    if !owned_claude_marketplace(home, &marketplace) {
        return Ok(false);
    }
    let plugin = home
        .root()
        .join("integrations/local-sessions/plugins/cccc-local-sessions");
    let entries = claude_registry(false).await?;
    let owned = entries
        .iter()
        .filter(|entry| entry["id"] == PLUGIN && entry["scope"] == "user")
        .collect::<Vec<_>>();
    if owned.len() != 1 {
        return Ok(false);
    }
    let entry = owned[0];
    if let Some(folder) = entry["readFromFolder"].as_str() {
        return Ok(same_path(Some(folder), &plugin));
    }
    let Some(installed) = entry["installPath"].as_str() else {
        return Ok(false);
    };
    Ok(["hooks/hooks.json", "monitors/monitors.json"]
        .iter()
        .all(|file| {
            fs::read_json::<Value>(&plugin.join(file))
                .ok()
                .zip(fs::read_json::<Value>(&std::path::Path::new(installed).join(file)).ok())
                .is_some_and(|(a, b)| a == b)
        }))
}

async fn claude_plugin_command(args: &[String]) -> Result<()> {
    let mut args = args.to_vec();
    args.push("--json".into());
    let output = invoke("claude", &args).await?;
    anyhow::ensure!(
        !output.stdout_truncated && !output.stderr_truncated,
        "Claude plugin configuration response was incomplete"
    );
    let result = output
        .stdout
        .split(|byte| *byte == b'\n')
        .rfind(|line| !line.iter().all(u8::is_ascii_whitespace))
        .and_then(|line| serde_json::from_slice::<Value>(line).ok())
        .context("Claude plugin configuration did not return a structured result")?;
    anyhow::ensure!(
        (output.status.success() && result["outcome"] == "ok")
            || (result["failureCode"] == "already_in_goal_state"
                && result["alreadyInGoalState"] == true),
        "Claude plugin configuration failed; inspect its plugin settings before retrying"
    );
    Ok(())
}

fn claude_plugin_documents(
    home: &HomeLayout,
    executable: &std::path::Path,
) -> Result<[(&'static str, Value); 3]> {
    let provider_home = std::path::absolute(native_home("CLAUDE_CONFIG_DIR", ".claude")?)?;
    let mut hooks = serde_json::Map::new();
    for event in ["SessionStart", "UserPromptSubmit", "SessionEnd"] {
        hooks.insert(event.into(),json!([{"hooks":[{"type":"command","command":executable,"args":["sessions","hook","--runtime","claude","--cccc-home",home.root(),"--provider-home",provider_home],"timeout":3}]}]));
    }
    let monitor = if cfg!(windows) {
        windows_command(&format!(
            "$env:CCCC_HOME = {}\n& {} sessions receive\nexit $LASTEXITCODE",
            powershell_quote(&home.root().to_string_lossy()),
            powershell_quote(&executable.to_string_lossy())
        ))
    } else {
        format!("{} sessions receive", prefix(home, executable))
    };
    Ok([
        (
            ".claude-plugin/plugin.json",
            json!({"name":"cccc-local-sessions","version":env!("CARGO_PKG_VERSION"),"description":"Same-user native session collaboration through CCCC","license":"MIT"}),
        ),
        ("hooks/hooks.json", json!({"hooks":hooks})),
        (
            "monitors/monitors.json",
            json!([{"name":"cccc-incoming","command":monitor,"description":"Incoming CCCC collaboration messages","when":"always"}]),
        ),
    ])
}

fn claude_plugin(home: &HomeLayout, executable: &std::path::Path) -> Result<PathBuf> {
    let marketplace = home.root().join("integrations/local-sessions");
    let plugin = marketplace.join("plugins/cccc-local-sessions");
    fs::write_json(
        &marketplace.join(".claude-plugin/marketplace.json"),
        &json!({"name":"cccc-local","owner":{"name":"CCCC"},"plugins":[{"name":"cccc-local-sessions","source":"./plugins/cccc-local-sessions"}]}),
    )?;
    for (path, value) in claude_plugin_documents(home, executable)? {
        fs::write_json(&plugin.join(path), &value)?;
    }
    fs::atomic_write(&plugin.join("README.md"),b"# CCCC local sessions\n\nConnects this user-owned native session to the local CCCC daemon. Uses official lifecycle hooks and the experimental persistent Plugin Monitor. CCCC does not import conversation history or own the native runtime.\n")?;
    Ok(marketplace)
}

pub(crate) async fn enable(home: &HomeLayout, runtime: &str) -> Result<SetupResult> {
    // Check before any provider command, write or integration fence change,
    // including the Pi/OpenCode branches that use the public launcher.
    let executable = running_executable()?;
    if runtime == "opencode" {
        return opencode::configure(home, true).await;
    }
    if runtime == "pi" {
        return configure_pi(home, true).await;
    }
    platform()?;
    version(runtime).await?;
    // Native identity relies on direct process ancestry; a Python/npm launcher
    // may add wrappers, especially on Windows. Install this actual Rust binary.
    home.initialize()?;
    let marketplace = if runtime == "claude" {
        let path = home.root().join("integrations/local-sessions");
        let registered = claude_marketplace().await?;
        anyhow::ensure!(
            registered
                .as_ref()
                .is_none_or(|entry| owned_claude_marketplace(home, entry)),
            "The cccc-local marketplace belongs to another CCCC installation. Use that installation, or explicitly remove its marketplace with `claude plugin marketplace remove cccc-local` before enabling this one. No provider configuration was changed"
        );
        let installed = claude_registry(false)
            .await?
            .iter()
            .any(|entry| entry["id"] == PLUGIN && entry["scope"] == "user");
        anyhow::ensure!(
            !installed || claude_plugin_owned(home).await?,
            "The installed CCCC plugin does not match this installation. Inspect `claude plugin list --json` and explicitly reinstall it before enabling. No provider configuration was changed"
        );
        Some((path, registered.is_some()))
    } else {
        None
    };
    // Establish source-aware tools before enabling callbacks/receiving. If
    // installation stops midway, an unregistered source stays unavailable
    // instead of receiving work through an old active-Group MCP identity.
    let warnings = configure_mcp(home, runtime, &executable, true).await?;
    if runtime == "codex" {
        codex_hooks(home, &executable, true)?;
    } else {
        let (marketplace, registered) = marketplace.expect("Claude marketplace preflight");
        claude_plugin(home, &executable)?;
        if !registered {
            claude_plugin_command(&[
                "plugin".into(),
                "marketplace".into(),
                "add".into(),
                marketplace.to_string_lossy().into_owned(),
            ])
            .await?;
        }
        claude_plugin_command(&[
            "plugin".into(),
            "install".into(),
            PLUGIN.into(),
            "--scope".into(),
            "user".into(),
        ])
        .await?;
        claude_plugin_command(&[
            "plugin".into(),
            "enable".into(),
            PLUGIN.into(),
            "--scope".into(),
            "user".into(),
        ])
        .await?;
        anyhow::ensure!(
            claude_plugin_owned(home).await?,
            "Claude installed plugin provenance was not confirmed; reload or repair the CCCC plugin before enabling native access"
        );
    }
    local_sessions::set_integration_disabled(
        home,
        if runtime == "codex" {
            LocalSessionRuntime::Codex
        } else {
            LocalSessionRuntime::Claude
        },
        false,
    )?;
    let note = if runtime == "codex" {
        "Configuration saved. After upgrading CCCC in existing Codex CLI sessions, let active and queued work finish, then use Codex /daemon > Restart background server > Restart to load the new MCP process; this affects other chats on that server. Check cccc with /mcp and reload/review hooks in /hooks. Restarting only CCCC does not update a running MCP server."
    } else {
        "Configuration saved. In existing sessions, reconnect the cccc MCP server to load its current tools and reload the CCCC plugin/Monitor, or resume the original native session. Plugin disable/enable alone may leave an old Monitor running. Monitor availability is required."
    };
    Ok(SetupResult {
        provider_dir: provider_directory(runtime)?,
        output: json!({"runtime":runtime,"enabled":true,"note":note}),
        warnings,
    })
}

pub(crate) async fn disable(home: &HomeLayout, runtime: &str) -> Result<SetupResult> {
    if runtime == "opencode" {
        return opencode::configure(home, false).await;
    }
    if runtime == "pi" {
        return configure_pi(home, false).await;
    }
    platform()?;
    local_sessions::set_integration_disabled(
        home,
        if runtime == "codex" {
            LocalSessionRuntime::Codex
        } else {
            LocalSessionRuntime::Claude
        },
        true,
    )?;
    let executable = std::env::current_exe()?;
    let mut warnings = Vec::new();
    if runtime == "codex" {
        let owned = codex_mcp_owned(home, &executable).await?;
        codex_hooks(home, &executable, false)?;
        if owned {
            warnings.extend(
                configure_mcp(
                    home,
                    runtime,
                    &crate::commands::setup::public_executable()?,
                    false,
                )
                .await?,
            );
        }
    } else {
        if claude_plugin_owned(home).await? {
            claude_plugin_command(&[
                "plugin".into(),
                "disable".into(),
                PLUGIN.into(),
                "--scope".into(),
                "user".into(),
            ])
            .await?;
        }
        warnings.extend(configure_mcp(home, runtime, &executable, false).await?);
    }
    Ok(SetupResult {
        provider_dir: provider_directory(runtime)?,
        output: json!({"runtime":runtime,"enabled":false,"note":"Reload native configuration to apply. Original sessions and conversation history are retained."}),
        warnings,
    })
}

fn provider_directory(runtime: &str) -> Result<PathBuf> {
    let (variable, fallback) = match runtime {
        "codex" => ("CODEX_HOME", ".codex"),
        "claude" => ("CLAUDE_CONFIG_DIR", ".claude"),
        "pi" => ("PI_CODING_AGENT_DIR", ".pi/agent"),
        _ => bail!("Unsupported native runtime"),
    };
    std::path::absolute(native_home(variable, fallback)?).map_err(Into::into)
}
