//! Explicit, read-only setup checks. Registration and receive capability are
//! separate facts: even an exact match here cannot prove a running app reloaded.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SetupInspection {
    pub runtime: String,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    pub provider_dir: Option<PathBuf>,
    pub configured: Option<bool>,
    pub disabled: bool,
    pub reconfigure_disconnects: bool,
    pub issue: Option<String>,
    pub codex_auto_start: Option<bool>,
}

pub(crate) async fn inspect(home: &HomeLayout, runtime: &str) -> SetupInspection {
    let mut result = SetupInspection {
        runtime: runtime.into(),
        executable: cccc_runtime::resolve_executable_in_path(
            runtime,
            std::env::var("PATH").ok().as_deref(),
        ),
        version: None,
        provider_dir: None,
        configured: None,
        disabled: false,
        reconfigure_disconnects: matches!(runtime, "pi" | "opencode"),
        issue: None,
        codex_auto_start: None,
    };
    if let Err(error) = inspect_into(home, runtime, &mut result).await {
        result.issue = Some(error.to_string());
    }
    result
}

async fn inspect_into(
    home: &HomeLayout,
    runtime: &str,
    result: &mut SetupInspection,
) -> Result<()> {
    let current_executable = running_executable()?;
    let kind = match runtime {
        "codex" => LocalSessionRuntime::Codex,
        "claude" => LocalSessionRuntime::Claude,
        "pi" => LocalSessionRuntime::Pi,
        "opencode" => LocalSessionRuntime::Opencode,
        _ => bail!("Unsupported native runtime"),
    };
    if runtime != "opencode" {
        result.provider_dir = Some(provider_directory(runtime)?);
    }
    result.disabled = local_sessions::integration_disabled(home, kind)?;
    anyhow::ensure!(
        result.executable.is_some(),
        "{runtime} was not found in this process's PATH"
    );
    result.version = Some(version(runtime).await?);
    if runtime == "opencode" {
        result.provider_dir = Some(opencode::config_directory().await?);
    }
    let provider = result
        .provider_dir
        .as_ref()
        .context("Native configuration directory is unavailable")?;
    let executable = if matches!(runtime, "pi" | "opencode") {
        crate::commands::setup::public_executable()?
    } else {
        current_executable
    };
    let matching = match kind {
        LocalSessionRuntime::Codex => {
            // A disabled feature is a prerequisite to explain, never permission
            // to change a user's explicit runtime preference.
            if let Ok(output) = invoke("codex", &["features".into(), "list".into()]).await
                && output.status.success()
                && !output.stdout_truncated
            {
                result.codex_auto_start =
                    codex_auto_start(&String::from_utf8_lossy(&output.stdout));
            }
            codex_mcp_owned(home, &executable).await?
                && codex_hooks_match(
                    &optional_json(&provider.join("hooks.json"))?,
                    &codex_hook(home, &executable, provider),
                )
        }
        LocalSessionRuntime::Claude => {
            let mcp = claude_user_mcp(&claude_user_config_path()?)?;
            let plugin = home
                .root()
                .join("integrations/local-sessions/plugins/cccc-local-sessions");
            let mut current = true;
            for (path, expected) in claude_plugin_documents(home, &executable)? {
                current &= optional_json(&plugin.join(path))? == expected;
            }
            mcp.as_ref() == Some(&claude_native_mcp(home, &executable))
                && current
                && claude_plugin_owned(home).await?
        }
        LocalSessionRuntime::Pi => pi_configured(home, &executable, provider)?,
        LocalSessionRuntime::Opencode => opencode::configured(home, &executable, provider)?,
    };
    result.configured = Some(matching && !result.disabled);
    Ok(())
}

fn optional_json(path: &std::path::Path) -> Result<Value> {
    match fs::read_json(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(_) => bail!("Could not read native configuration at {}", path.display()),
    }
}

fn codex_auto_start(text: &str) -> Option<bool> {
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != "daemon_auto_start" {
            return None;
        }
        match fields.last()? {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    })
}

fn codex_hooks_match(config: &Value, expected: &Value) -> bool {
    ["SessionStart", "UserPromptSubmit", "SessionEnd"]
        .iter()
        .all(|event| {
            config["hooks"][event].as_array().is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry["hooks"]
                        .as_array()
                        .is_some_and(|hooks| hooks.contains(expected))
                })
            })
        })
}

fn pi_configured(
    home: &HomeLayout,
    executable: &std::path::Path,
    provider: &std::path::Path,
) -> Result<bool> {
    let config = optional_json(&provider.join("mcp.json"))?;
    if config["mcpServers"]["cccc"] != pi_mcp(home, executable) {
        return Ok(false);
    }
    let plugin = home.root().join("integrations/pi");
    let settings = optional_json(&provider.join("settings.json"))?;
    let registered = settings["packages"].as_array().is_some_and(|packages| {
        packages.iter().any(|package| {
            let Some(source) = package.as_str().or_else(|| package["source"].as_str()) else {
                return false;
            };
            let path = std::path::Path::new(source);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                provider.join(path)
            };
            same_path(path.to_str(), &plugin)
        })
    });
    if !registered {
        return Ok(false);
    }
    let manifest = optional_json(&plugin.join("package.json"))?;
    if manifest["name"] != "@cccc/native-pi" || manifest["version"] != env!("CARGO_PKG_VERSION") {
        return Ok(false);
    }
    let expected = include_str!("../../../resources/native/pi.ts")
        .replace("__CCCC_EXECUTABLE__", &serde_json::to_string(executable)?)
        .replace("__CCCC_HOME__", &serde_json::to_string(home.root())?);
    match std::fs::read(plugin.join("index.ts")) {
        Ok(bytes) => Ok(bytes == expected.as_bytes()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn replaced_executable_blocks_all_native_setup_before_changes() {
        const CHILD: &str = "CCCC_TEST_REPLACED_SETUP";
        if std::env::var_os(CHILD).is_some() {
            let root = std::env::current_dir().expect("isolated working directory");
            let executable = std::env::current_exe().expect("running test copy");
            assert_eq!(executable, root.join("cccc-test"));
            std::fs::remove_file(executable).expect("remove only fixture executable");
            let home = HomeLayout::from_path(root.join("cccc")).expect("fixture Home");
            for runtime in ["codex", "claude", "pi", "opencode"] {
                let failure = enable(&home, runtime)
                    .await
                    .expect_err("unusable installation");
                assert_eq!(failure.to_string(), RESTART_REQUIRED, "{runtime}");
                let inspection = inspect(&home, runtime).await;
                assert_eq!(inspection.issue.as_deref(), Some(RESTART_REQUIRED));
                assert_eq!(inspection.configured, None);
                assert!(!home.root().exists(), "{runtime} changed CCCC state");
            }
            for directory in ["codex", "claude", "pi", "config"] {
                assert!(!root.join(directory).exists(), "provider state was created");
            }
            return;
        }
        let temp = tempfile::tempdir().expect("isolated test process");
        let executable = temp.path().join("cccc-test");
        std::fs::copy(std::env::current_exe().expect("test binary"), &executable)
            .expect("copy fixture executable");
        let mut command = std::process::Command::new(executable);
        command
            .args([
                "--exact",
                "commands::sessions::setup::inspection::tests::replaced_executable_blocks_all_native_setup_before_changes",
                "--nocapture",
            ])
            .current_dir(temp.path())
            .env_clear()
            .env(CHILD, "1")
            .env("PATH", "")
            .env("HOME", temp.path().join("user"))
            .env("CCCC_HOME", temp.path().join("cccc"))
            .env("CODEX_HOME", temp.path().join("codex"))
            .env("CLAUDE_CONFIG_DIR", temp.path().join("claude"))
            .env("PI_CODING_AGENT_DIR", temp.path().join("pi"))
            .env("XDG_CONFIG_HOME", temp.path().join("config"));
        let result =
            cccc_runtime::capture_command(&mut command, None, Duration::from_secs(30), 8192)
                .await
                .expect("bounded fixture process");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    }

    #[test]
    fn auto_start_uses_the_actual_feature_value() {
        assert_eq!(
            codex_auto_start("daemon_auto_start stable false\n"),
            Some(false)
        );
        assert_eq!(
            codex_auto_start("daemon_auto_start stable true\n"),
            Some(true)
        );
        assert_eq!(codex_auto_start("something_else stable true\n"), None);
        assert_eq!(codex_auto_start("daemon_auto_start stable unknown\n"), None);
    }

    #[test]
    fn current_hooks_require_all_lifecycle_events_and_the_exact_command() {
        let hook = json!({"command":"cccc sessions hook","statusMessage":MARKER});
        let mut config = json!({"hooks":{}});
        for event in ["SessionStart", "UserPromptSubmit", "SessionEnd"] {
            config["hooks"][event] = json!([{"hooks":[hook]}]);
        }
        assert!(codex_hooks_match(&config, &hook));
        config["hooks"]["SessionEnd"][0]["hooks"][0]["command"] = json!("old-cccc sessions hook");
        assert!(!codex_hooks_match(&config, &hook));
    }
}
