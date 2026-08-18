use cccc_contracts::{
    DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION, DEEPSEEK_ACP_PACKAGE, DEEPSEEK_ACP_VERSION,
    DEEPSEEK_DSH_PACKAGE, DEEPSEEK_DSH_VERSION, DEEPSEEK_LLM_ADAPTER_PACKAGE,
    DEEPSEEK_LLM_ADAPTER_VERSION, DEEPSEEK_MCP_CLIENT_PACKAGE, DEEPSEEK_MCP_CLIENT_VERSION,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(120);
const NODE_USE_ENV_PROXY: &str = "NODE_USE_ENV_PROXY";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeepSeekSetupOutcome {
    pub dsh_home: PathBuf,
    pub profile: PathBuf,
    pub packages_installed: bool,
    pub profile_created: bool,
}

pub fn ensure(
    env: &mut BTreeMap<String, String>,
    cccc_executable: &Path,
) -> Result<DeepSeekSetupOutcome, String> {
    ensure_with(
        env,
        cccc_executable,
        install_packages,
        cccc_runtime::deepseek_external_preflight,
        cccc_runtime::deepseek_preflight,
    )
}

fn ensure_with(
    env: &mut BTreeMap<String, String>,
    cccc_executable: &Path,
    installer: impl Fn(&Path, &BTreeMap<String, String>) -> Result<(), String>,
    external_preflight: impl Fn(&[String], &BTreeMap<String, String>) -> Result<(), String>,
    ready_preflight: impl Fn(&[String], &BTreeMap<String, String>) -> Result<(), String>,
) -> Result<DeepSeekSetupOutcome, String> {
    let dsh_home = cccc_runtime::deepseek_home(env)
        .ok_or_else(|| "DSH_HOME cannot be inferred because HOME is not configured".to_owned())?;
    env.insert("DSH_HOME".into(), dsh_home.to_string_lossy().into_owned());
    // Node's built-in fetch only honors HTTP(S)_PROXY when this opt-in is
    // enabled. Preserve an explicit actor or user value when one exists.
    env.entry(NODE_USE_ENV_PROXY.into())
        .or_insert_with(|| "1".into());
    prepend_local_bin(env, &dsh_home);
    let command = vec!["dsh-acp-demo".into()];
    if let Err(error) = external_preflight(&command, env)
        && !error
            .to_ascii_lowercase()
            .contains("deepseek executable not found")
    {
        return Err(error);
    }
    fs::create_dir_all(&dsh_home).map_err(|error| error.to_string())?;
    let profile = dsh_home.join("profiles/cccc-acp");
    if ready_preflight(&command, env).is_ok() {
        return Ok(DeepSeekSetupOutcome {
            dsh_home,
            profile,
            packages_installed: false,
            profile_created: false,
        });
    }
    let lock_path = dsh_home.join("cccc-acp.setup.lock");
    cccc_core::fs::with_exclusive_lock(&lock_path, || {
        if ready_preflight(&command, env).is_ok() {
            return Ok(DeepSeekSetupOutcome {
                dsh_home: dsh_home.clone(),
                profile: profile.clone(),
                packages_installed: false,
                profile_created: false,
            });
        }
        let packages_installed = if packages_ready(&dsh_home) {
            false
        } else {
            installer(&dsh_home, env).map_err(std::io::Error::other)?;
            if !packages_ready(&dsh_home) {
                return Err(std::io::Error::other(
                    "DeepSeek packages remain incomplete after automatic installation",
                ));
            }
            true
        };
        let (profile, profile_created) = ensure_profile(&dsh_home, cccc_executable)?;
        ready_preflight(&command, env).map_err(std::io::Error::other)?;
        Ok(DeepSeekSetupOutcome {
            dsh_home: dsh_home.clone(),
            profile,
            packages_installed,
            profile_created,
        })
    })
    .map_err(|error| error.to_string())
}

fn prepend_local_bin(env: &mut BTreeMap<String, String>, dsh_home: &Path) {
    let local_bin = dsh_home.join("node_modules").join(".bin");
    let mut paths = vec![local_bin.clone()];
    if let Some(existing) = env.get("PATH") {
        paths.extend(std::env::split_paths(existing).filter(|path| path != &local_bin));
    }
    if let Ok(path) = std::env::join_paths(paths) {
        env.insert("PATH".into(), path.to_string_lossy().into_owned());
    }
}

fn packages_ready(dsh_home: &Path) -> bool {
    required_packages().iter().all(|(package, version)| {
        fs::read(
            dsh_home
                .join("node_modules")
                .join(package)
                .join("package.json"),
        )
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|value| {
            value
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
            == Some(*version)
    })
}

fn required_packages() -> [(&'static str, &'static str); 5] {
    [
        (DEEPSEEK_DSH_PACKAGE, DEEPSEEK_DSH_VERSION),
        (DEEPSEEK_ACP_PACKAGE, DEEPSEEK_ACP_VERSION),
        (DEEPSEEK_MCP_CLIENT_PACKAGE, DEEPSEEK_MCP_CLIENT_VERSION),
        (DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION),
        (DEEPSEEK_LLM_ADAPTER_PACKAGE, DEEPSEEK_LLM_ADAPTER_VERSION),
    ]
}

fn install_packages(dsh_home: &Path, env: &BTreeMap<String, String>) -> Result<(), String> {
    let mut command = Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" });
    configure_process_group(&mut command);
    command
        .args(["install", "--save-exact", "--no-audit", "--no-fund"])
        .args(required_packages().map(|(package, version)| format!("{package}@{version}")))
        .current_dir(dsh_home)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start npm for DeepSeek setup: {error}"))?;
    let deadline = Instant::now() + INSTALL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!("DeepSeek package installation failed: {status}"));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                terminate_process_tree(&mut child);
                return Err("DeepSeek package installation timed out after 120 seconds".into());
            }
            Err(error) => {
                terminate_process_tree(&mut child);
                return Err(format!("DeepSeek package installer failed: {error}"));
            }
        }
    }
}

fn ensure_profile(dsh_home: &Path, executable: &Path) -> std::io::Result<(PathBuf, bool)> {
    let profile_root = dsh_home.join("profiles");
    let profile = profile_root.join("cccc-acp");
    if profile.exists() {
        let existing = fs::read(profile.join("package.json"))
            .ok()
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
        if existing
            .as_ref()
            .and_then(|value| value.get("ccccManaged"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Err(std::io::Error::other(
                "existing cccc-acp profile is not managed by CCCC",
            ));
        }
        write_profile_files(&profile, executable)?;
        return Ok((profile, false));
    }
    fs::create_dir_all(&profile_root)?;
    let staging = profile_root.join(format!(".cccc-acp-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        fs::create_dir(&staging)?;
        write_profile_files(&staging, executable)?;
        fs::rename(&staging, &profile)
    })();
    if result.is_err() {
        fs::remove_dir_all(&staging).ok();
    }
    result?;
    Ok((profile, true))
}

fn write_profile_files(profile: &Path, executable: &Path) -> std::io::Result<()> {
    fs::create_dir_all(profile)?;
    cccc_core::fs::write_json(
        &profile.join("package.json"),
        &json!({
            "name":"dsh-profile-cccc-acp", "private":true, "ccccManaged":true,
            "dependencies": {
                DEEPSEEK_ACP_PACKAGE:DEEPSEEK_ACP_VERSION,
                DEEPSEEK_MCP_CLIENT_PACKAGE:DEEPSEEK_MCP_CLIENT_VERSION,
                DEEPSEEK_ACP_APP_PACKAGE:DEEPSEEK_ACP_APP_VERSION,
                DEEPSEEK_LLM_ADAPTER_PACKAGE:DEEPSEEK_LLM_ADAPTER_VERSION
            },
            "dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-headless"]}}
        }),
    )?;
    // YAML single-quoted scalars escape apostrophes by doubling them. A
    // backslash is literal in this scalar style and must not be doubled.
    let cccc_path = executable.to_string_lossy().replace('\'', "''");
    let patch = format!(
        "- insert:\n    - id: acp\n      name: '@deepseek-ai/dsh-acp'\n    - id: cccc-mcp\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        transport: stdio\n        serverName: cccc\n        command: '{cccc_path}'\n        args: [mcp]\n        env:\n          CCCC_HOME: !!js process.env.CCCC_HOME\n          CCCC_GROUP_ID: !!js process.env.CCCC_GROUP_ID\n          CCCC_ACTOR_ID: !!js process.env.CCCC_ACTOR_ID\n        failOnStartupError: true\n"
    );
    cccc_core::fs::atomic_write(&profile.join("cordis.patch.yml"), patch.as_bytes())?;
    let config = format!(
        "- id: llm-deepseek\n  name: '@deepseek-ai/dsh-llm-deepseek'\n- id: acp-demo\n  name: '@deepseek-ai/dsh-acp-demo'\n  config:\n    provider: deepseek-official\n    model: deepseek-v4-flash\n    workspaceContext: false\n- id: cccc-mcp\n  name: '@deepseek-ai/dsh-mcp-client'\n  config:\n    transport: stdio\n    serverName: cccc\n    command: '{cccc_path}'\n    args: [mcp]\n    env:\n      CCCC_HOME: !!js process.env.CCCC_HOME\n      CCCC_GROUP_ID: !!js process.env.CCCC_GROUP_ID\n      CCCC_ACTOR_ID: !!js process.env.CCCC_ACTOR_ID\n    failOnStartupError: true\n"
    );
    cccc_core::fs::atomic_write(&profile.join("cordis.yml"), config.as_bytes())
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

fn terminate_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", child.id())])
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
#[path = "deepseek_setup_tests.rs"]
mod tests;
