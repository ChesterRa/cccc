use cccc_contracts::{
    ActorRuntime, DEEPSEEK_NPM_BEFORE, DEEPSEEK_PACKAGE, DEEPSEEK_RELEASE_VERSION,
};
use cccc_core::HomeLayout;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The first install downloads about 300 MB and uses about 1.3 GB of disk
/// (measured for this release on Linux x64), so a slow network needs room.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const NODE_USE_ENV_PROXY: &str = "NODE_USE_ENV_PROXY";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeepSeekSetupOutcome {
    pub dsh_home: PathBuf,
    /// The installed bundle that provides `dsh --profile acp`.
    pub profile: PathBuf,
    pub packages_installed: bool,
    /// Always false: each agent's launch writes its own profile layer. Kept so
    /// `cccc setup` output keeps its fields.
    pub profile_created: bool,
}

/// Installs the pinned DeepSeek Harness under `CCCC_HOME` when it is missing
/// or incomplete, and prepares the environment its agents start with. Used by
/// `cccc setup --runtime deepseek`, which may wait for the whole download.
pub fn ensure(
    home: &HomeLayout,
    env: &mut BTreeMap<String, String>,
) -> Result<DeepSeekSetupOutcome, String> {
    ensure_with(
        home,
        env,
        install_packages,
        cccc_runtime::deepseek_external_preflight,
        cccc_runtime::deepseek_installation_ready,
    )
}

/// Prepares an agent start with its command: the same environment as setup,
/// the command's own checks, and a complete installation. It never installs,
/// because a start holds its Group's operations while it runs and the first
/// download takes minutes. Read-only setup reports use it too.
pub fn prepare_for_start(
    home: &HomeLayout,
    env: &mut BTreeMap<String, String>,
    command: &[String],
) -> Result<DeepSeekSetupOutcome, String> {
    prepare_for_start_with(
        home,
        env,
        command,
        cccc_runtime::deepseek_external_preflight,
        cccc_runtime::deepseek_installation_ready,
    )
}

fn prepare_for_start_with(
    home: &HomeLayout,
    env: &mut BTreeMap<String, String>,
    command: &[String],
    external_preflight: impl Fn(&[String], &BTreeMap<String, String>) -> Result<(), String>,
    installation_ready: impl Fn(&Path) -> Result<(), String>,
) -> Result<DeepSeekSetupOutcome, String> {
    let dsh_home = prepare_environment(home, env, command, external_preflight)?;
    installation_ready(&dsh_home).map_err(|reason| {
        format!(
            "DeepSeek Harness {DEEPSEEK_RELEASE_VERSION} is not ready: {reason}. Run `cccc setup --runtime deepseek` (the first install downloads about 300 MB), then start the agent again."
        )
    })?;
    Ok(outcome(&dsh_home, false))
}

fn outcome(dsh_home: &Path, packages_installed: bool) -> DeepSeekSetupOutcome {
    DeepSeekSetupOutcome {
        dsh_home: dsh_home.to_owned(),
        profile: cccc_runtime::deepseek_acp_profile(dsh_home),
        packages_installed,
        profile_created: false,
    }
}

/// Sets what setup and agents share, checks what the installation cannot
/// provide (Node, and a custom command's executable), and returns the
/// installation root.
fn prepare_environment(
    home: &HomeLayout,
    env: &mut BTreeMap<String, String>,
    command: &[String],
    external_preflight: impl Fn(&[String], &BTreeMap<String, String>) -> Result<(), String>,
) -> Result<PathBuf, String> {
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    let dsh_home =
        cccc_runtime::deepseek_home(env).ok_or_else(|| "CCCC_HOME is not configured".to_owned())?;
    // Node's built-in fetch only honors HTTP(S)_PROXY when this opt-in is
    // enabled. Preserve an explicit actor or user value when one exists.
    env.entry(NODE_USE_ENV_PROXY.into())
        .or_insert_with(|| "1".into());
    bypass_proxy_for_loopback_base(env);
    // Checked before anything is downloaded.
    external_preflight(command, env)?;
    Ok(dsh_home)
}

fn ensure_with(
    home: &HomeLayout,
    env: &mut BTreeMap<String, String>,
    installer: impl Fn(&Path, &BTreeMap<String, String>) -> Result<(), String>,
    external_preflight: impl Fn(&[String], &BTreeMap<String, String>) -> Result<(), String>,
    installation_ready: impl Fn(&Path) -> Result<(), String>,
) -> Result<DeepSeekSetupOutcome, String> {
    let dsh_home = prepare_environment(
        home,
        env,
        &cccc_runtime::default_command(ActorRuntime::Deepseek),
        external_preflight,
    )?;
    if installation_ready(&dsh_home).is_ok() {
        return Ok(outcome(&dsh_home, false));
    }
    fs::create_dir_all(&dsh_home).map_err(|error| error.to_string())?;
    cccc_core::fs::with_exclusive_lock(&dsh_home.join("cccc-setup.lock"), || {
        if installation_ready(&dsh_home).is_ok() {
            return Ok(outcome(&dsh_home, false));
        }
        for package in cccc_runtime::deepseek_packages_to_reinstall(&dsh_home) {
            fs::remove_dir_all(&package)?;
        }
        installer(&dsh_home, env).map_err(std::io::Error::other)?;
        installation_ready(&dsh_home).map_err(|reason| {
            std::io::Error::other(format!(
                "DeepSeek Harness is still incomplete after installation: {reason}"
            ))
        })?;
        Ok(outcome(&dsh_home, true))
    })
    .map_err(|error| error.to_string())
}

fn bypass_proxy_for_loopback_base(env: &mut BTreeMap<String, String>) {
    let Some(url) = env
        .get("DEEPSEEK_BASE_URL")
        .and_then(|base| url::Url::parse(base).ok())
        .filter(cccc_core::local_network::is_loopback_url)
    else {
        return;
    };
    let Some(host) = url.host_str() else {
        return;
    };
    // Node prefers lowercase no_proxy whenever it is present, even when empty.
    // Updating only the effective list preserves its other exclusions.
    let key = if env.contains_key("no_proxy") {
        "no_proxy"
    } else {
        "NO_PROXY"
    };
    let exclusions = env.entry(key.into()).or_default();
    // Node treats a bare `*` as bypass-all, but not `*,other-host`.
    if exclusions.as_str() == "*" {
        return;
    }
    if !exclusions
        .split(',')
        .any(|entry| entry.trim().eq_ignore_ascii_case(host))
    {
        if !exclusions.is_empty() {
            exclusions.push(',');
        }
        exclusions.push_str(host);
    }
}

fn install_packages(dsh_home: &Path, env: &BTreeMap<String, String>) -> Result<(), String> {
    cccc_core::fs::write_json(
        &dsh_home.join("package.json"),
        &cccc_runtime::canonical_deepseek_runtime_manifest(),
    )
    .map_err(|error| format!("failed to write DeepSeek runtime manifest: {error}"))?;
    let log_path = dsh_home.join("npm-install.log");
    let log = fs::File::create(&log_path)
        .map_err(|error| format!("failed to create {}: {error}", log_path.display()))?;
    let log_err = log
        .try_clone()
        .map_err(|error| format!("failed to open {}: {error}", log_path.display()))?;
    let mut command =
        cccc_runtime::background_command(if cfg!(windows) { "npm.cmd" } else { "npm" });
    configure_process_group(&mut command);
    command
        .args([
            "install",
            "--save-exact",
            "--no-audit",
            "--no-fund",
            // In this release the dependency install scripts only check for a
            // prebuilt native module (compiling one when it is missing), set an
            // executable bit the published package already has, or print
            // notices. The packages ship prebuilt modules for Linux, macOS and
            // Windows on x64 and arm64.
            "--ignore-scripts",
            "--before",
            DEEPSEEK_NPM_BEFORE,
            &format!("{DEEPSEEK_PACKAGE}@{DEEPSEEK_RELEASE_VERSION}"),
        ])
        .current_dir(dsh_home)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start npm for DeepSeek setup: {error}"))?;
    let deadline = Instant::now() + INSTALL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "DeepSeek package installation failed ({status}); npm output is in {}",
                    log_path.display()
                ));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                terminate_process_tree(&mut child);
                return Err(format!(
                    "DeepSeek package installation timed out after {} seconds; npm output is in {}",
                    INSTALL_TIMEOUT.as_secs(),
                    log_path.display()
                ));
            }
            Err(error) => {
                terminate_process_tree(&mut child);
                return Err(format!("DeepSeek package installer failed: {error}"));
            }
        }
    }
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
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;

        if let Ok(pid) = i32::try_from(child.id()) {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = cccc_runtime::background_command("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
#[path = "deepseek_setup_tests.rs"]
mod tests;
