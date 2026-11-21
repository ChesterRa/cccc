use crate::executable::resolve_executable_in_path;
use cccc_contracts::ActorRuntime;
pub use cccc_contracts::{
    DEEPSEEK_ACP_SDK_VERSION, DEEPSEEK_MAX_OUTPUT_TOKENS, DEEPSEEK_NODE_RANGE, DEEPSEEK_NPM_BEFORE,
    DEEPSEEK_PACKAGE, DEEPSEEK_RELEASE_VERSION, DEEPSEEK_TURN_TIMEOUT_SECONDS,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The bundle that provides `dsh --profile acp`, and the ACP bridge it starts.
/// The launcher installs both at the release version.
const DEEPSEEK_ACP_PROFILE_PACKAGE: &str = "@deepseek-ai/dsh-acp-app";
const DEEPSEEK_ACP_BRIDGE_PACKAGE: &str = "@deepseek-ai/dsh-acp";

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbe {
    pub name: String,
    pub display_name: String,
    pub recommended_command: String,
    pub command: String,
    pub available: bool,
    pub path: Option<PathBuf>,
}

/// Read-only DeepSeek readiness gate shared by actor start and runtime
/// discovery.  Presence of `dsh` alone is never sufficient.
pub fn deepseek_preflight(
    command: &[String],
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    deepseek_external_preflight(command, env)?;
    let dsh_home =
        deepseek_home(env).ok_or_else(|| "CCCC_HOME and HOME are not configured".to_owned())?;
    deepseek_installation_ready(&dsh_home)
}

/// Whether the pinned installation is complete: the launcher, the ACP profile
/// bundle and the ACP bridge at the release version, the launcher's command,
/// the canonical manifest, and a lockfile in which every DeepSeek Harness
/// package has that version. Setup repairs whatever this rejects.
pub fn deepseek_installation_ready(dsh_home: &Path) -> Result<(), String> {
    for package in [
        DEEPSEEK_PACKAGE,
        DEEPSEEK_ACP_PROFILE_PACKAGE,
        DEEPSEEK_ACP_BRIDGE_PACKAGE,
    ] {
        let installed = installed_manifest(&dsh_home.join("node_modules").join(package));
        if installed
            .as_ref()
            .and_then(|manifest| manifest.get("version"))
            .and_then(Value::as_str)
            != Some(DEEPSEEK_RELEASE_VERSION)
        {
            return Err(format!("{package}@{DEEPSEEK_RELEASE_VERSION} is required"));
        }
    }
    if !deepseek_acp_profile_complete(&deepseek_acp_profile(dsh_home)) {
        return Err("the DeepSeek ACP profile bundle is incomplete".to_owned());
    }
    if deepseek_launcher(dsh_home).is_none() {
        return Err("deepseek executable not found: dsh".to_owned());
    }
    let runtime_manifest: Value = serde_json::from_slice(
        &fs::read(dsh_home.join("package.json"))
            .map_err(|_| "DeepSeek managed runtime manifest is missing")?,
    )
    .map_err(|_| "DeepSeek managed runtime manifest is invalid")?;
    if !is_canonical_deepseek_runtime_manifest(&runtime_manifest) {
        return Err("DeepSeek managed runtime dependency set is not canonical".to_owned());
    }
    if !deepseek_lockfile_is_pinned(dsh_home) {
        return Err(format!(
            "DeepSeek dependency graph must be pinned to {DEEPSEEK_RELEASE_VERSION}"
        ));
    }
    Ok(())
}

fn installed_manifest(package: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(package.join("package.json")).ok()?).ok()
}

/// The bundle names its patch layer; without it `dsh --profile acp` cannot
/// start.
fn deepseek_acp_profile_complete(profile: &Path) -> bool {
    installed_manifest(profile)
        .and_then(|manifest| {
            manifest
                .pointer("/dsh/bundle/patch")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|patch| profile.join(patch).is_file())
}

/// Installed packages that npm would keep although they are incomplete. npm
/// treats a package at the right version as installed whatever files it
/// lacks, so setup removes these and lets npm unpack them again.
#[must_use]
pub fn deepseek_packages_to_reinstall(dsh_home: &Path) -> Vec<PathBuf> {
    let profile = deepseek_acp_profile(dsh_home);
    let at_release = installed_manifest(&profile)
        .as_ref()
        .and_then(|manifest| manifest.get("version"))
        .and_then(Value::as_str)
        == Some(DEEPSEEK_RELEASE_VERSION);
    if at_release && !deepseek_acp_profile_complete(&profile) {
        vec![profile]
    } else {
        Vec::new()
    }
}

/// The installed bundle that provides `dsh --profile acp`.
#[must_use]
pub fn deepseek_acp_profile(dsh_home: &Path) -> PathBuf {
    dsh_home
        .join("node_modules")
        .join(DEEPSEEK_ACP_PROFILE_PACKAGE)
}

/// Whether an agent command names the managed launcher. Agents saved before
/// CCCC moved to the `dsh` launcher name the earlier `dsh-acp-demo` app.
#[must_use]
pub fn deepseek_managed_command(command: &[String]) -> bool {
    command
        .first()
        .and_then(|value| Path::new(value).file_stem())
        .and_then(|value| value.to_str())
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "dsh" | "dsh-acp-demo"))
}

/// The managed launcher inside the pinned installation, never a `dsh` found
/// elsewhere on PATH.
#[must_use]
pub fn deepseek_executable(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    deepseek_launcher(&deepseek_home(env)?)
}

fn deepseek_launcher(dsh_home: &Path) -> Option<PathBuf> {
    let launcher = dsh_home.join("node_modules").join(".bin").join("dsh");
    resolve_executable_in_path(launcher.to_str()?, None)
}

pub fn deepseek_home(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let root = env
        .get("CCCC_HOME")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.get("HOME")
                .or_else(|| env.get("USERPROFILE"))
                .filter(|value| !value.trim().is_empty())
                .map(|home| PathBuf::from(home).join(".cccc"))
        })?;
    Some(
        root.join("runtimes")
            .join("deepseek")
            .join(DEEPSEEK_RELEASE_VERSION),
    )
}

/// What the managed installation cannot provide: a supported Node and, for a
/// custom command, its executable on PATH. Setup checks this before it
/// downloads the installation, which provides the managed launcher.
pub fn deepseek_external_preflight(
    command: &[String],
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    deepseek_node_preflight(env)?;
    if deepseek_managed_command(command) {
        return Ok(());
    }
    let executable = command.first().map(String::as_str).unwrap_or("dsh");
    if resolve_executable_in_path(executable, env.get("PATH").map(String::as_str)).is_none() {
        return Err(format!("deepseek executable not found: {executable}"));
    }
    Ok(())
}

pub fn deepseek_bootstrap_preflight(env: &BTreeMap<String, String>) -> Result<(), String> {
    deepseek_node_preflight(env)?;
    if resolve_executable_in_path("npm", env.get("PATH").map(String::as_str)).is_none() {
        return Err("npm is required to install DeepSeek Harness".to_owned());
    }
    Ok(())
}

fn deepseek_node_preflight(env: &BTreeMap<String, String>) -> Result<(), String> {
    let mut node_command = crate::background_command("node");
    for (key, value) in env {
        if key != "CCCC_NODE_VERSION" {
            node_command.env(key, value);
        }
    }
    let output = crate::capture_command_blocking(
        node_command.arg("--version"),
        None,
        std::time::Duration::from_secs(5),
        32_768,
    )
    .map_err(|error| format!("could not determine Node version: {error}"))?;
    let node = if output.status.success() && !output.stdout_truncated {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        String::new()
    };
    if !node_supported(&node) {
        return Err(format!(
            "DeepSeek Harness requires Node {DEEPSEEK_NODE_RANGE} (found {})",
            if node.is_empty() { "unknown" } else { &node }
        ));
    }
    Ok(())
}

pub fn deepseek_lockfile_is_pinned(dsh_home: &Path) -> bool {
    let Ok(raw) = fs::read(dsh_home.join("package-lock.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&raw) else {
        return false;
    };
    let Some(packages) = value.get("packages").and_then(Value::as_object) else {
        return false;
    };
    if !packages
        .get("")
        .and_then(Value::as_object)
        .and_then(|root| root.get("dependencies"))
        .is_some_and(deepseek_dependencies_are_exact)
    {
        return false;
    }
    let pinned = |entry: &Value| {
        entry.get("version").and_then(Value::as_str) == Some(DEEPSEEK_RELEASE_VERSION)
    };
    [
        DEEPSEEK_PACKAGE,
        DEEPSEEK_ACP_PROFILE_PACKAGE,
        DEEPSEEK_ACP_BRIDGE_PACKAGE,
    ]
    .iter()
    .all(|package| {
        packages
            .get(&format!("node_modules/{package}"))
            .is_some_and(pinned)
    }) && packages.iter().all(|(lock_path, entry)| {
        let name = lock_path
            .rsplit("node_modules/")
            .next()
            .unwrap_or(lock_path);
        (name != DEEPSEEK_PACKAGE && !name.starts_with("@deepseek-ai/dsh-")) || pinned(entry)
    })
}

pub fn canonical_deepseek_runtime_manifest() -> Value {
    serde_json::json!({
        "name": "cccc-deepseek-runtime",
        "private": true,
        "ccccManaged": true,
        "dependencies": deepseek_dependency_map(),
    })
}

pub fn is_canonical_deepseek_runtime_manifest(manifest: &Value) -> bool {
    let Some(object) = manifest.as_object() else {
        return false;
    };
    object.get("name") == Some(&Value::String("cccc-deepseek-runtime".to_owned()))
        && object.get("private") == Some(&Value::Bool(true))
        && object.get("ccccManaged") == Some(&Value::Bool(true))
        && object
            .get("dependencies")
            .is_some_and(deepseek_dependencies_are_exact)
}

fn deepseek_dependencies_are_exact(value: &Value) -> bool {
    value.as_object() == Some(&deepseek_dependency_map())
}

fn deepseek_dependency_map() -> serde_json::Map<String, Value> {
    serde_json::Map::from_iter([(
        DEEPSEEK_PACKAGE.to_owned(),
        Value::String(DEEPSEEK_RELEASE_VERSION.to_owned()),
    )])
}

/// Matches `DEEPSEEK_NODE_RANGE`.
fn node_supported(raw: &str) -> bool {
    let numbers: Vec<u32> = raw
        .trim_start_matches('v')
        .split('.')
        .take(3)
        .filter_map(|part| part.parse().ok())
        .collect();
    numbers.len() >= 2 && (numbers[0] > 24 || (numbers[0] == 24 && numbers[1] >= 9))
}

#[must_use]
pub fn default_command(runtime: ActorRuntime) -> Vec<String> {
    let command = match runtime {
        ActorRuntime::Amp => "amp",
        ActorRuntime::Antigravity => "agy --dangerously-skip-permissions",
        ActorRuntime::Auggie => "auggie",
        ActorRuntime::Claude => "claude --dangerously-skip-permissions",
        ActorRuntime::Cline => "cline --tui --auto-approve true",
        ActorRuntime::Codex => {
            "codex -c check_for_update_on_startup=false -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox --search"
        }
        ActorRuntime::Deepseek => "dsh --profile acp",
        ActorRuntime::Copilot => "copilot --allow-all",
        ActorRuntime::Cursor => "cursor-agent --yolo --approve-mcps",
        ActorRuntime::Devin => "devin --permission-mode dangerous",
        ActorRuntime::Kiro => "kiro-cli chat --trust-all-tools",
        ActorRuntime::Kilo => "kilo",
        ActorRuntime::Droid => "droid --auto high",
        ActorRuntime::Grok => "grok --always-approve",
        ActorRuntime::Hermes => "hermes --tui --yolo",
        ActorRuntime::Kimi => "kimi --yolo",
        ActorRuntime::Opencode => "opencode --auto",
        ActorRuntime::Pi => "pi",
        ActorRuntime::WebModel
        | ActorRuntime::ChatgptDots
        | ActorRuntime::GrokWebModel
        | ActorRuntime::Custom => "",
    };
    command.split_whitespace().map(str::to_owned).collect()
}

#[must_use]
pub fn detect_runtimes() -> Vec<RuntimeProbe> {
    serde_json::from_value::<Vec<ActorRuntime>>(serde_json::json!([
        "claude",
        "cline",
        "codex",
        "deepseek",
        "copilot",
        "cursor",
        "devin",
        "kiro",
        "kilo",
        "antigravity",
        "droid",
        "amp",
        "auggie",
        "grok",
        "hermes",
        "kimi",
        "opencode",
        "pi",
        "web_model",
        "chatgpt_dots",
        "grok_web_model",
        "custom"
    ]))
    .unwrap_or_default()
    .into_iter()
    .map(|runtime| {
        let recommended = default_command(runtime);
        let command = recommended.first().cloned().unwrap_or_default();
        let discovery_env = std::env::vars().collect::<BTreeMap<_, _>>();
        let (available, path) = if runtime == ActorRuntime::Deepseek {
            deepseek_catalog_entry(&discovery_env)
        } else {
            let path =
                resolve_executable_in_path(&command, discovery_env.get("PATH").map(String::as_str));
            let available = matches!(
                runtime,
                ActorRuntime::WebModel
                    | ActorRuntime::ChatgptDots
                    | ActorRuntime::GrokWebModel
                    | ActorRuntime::Custom
            ) || path.is_some();
            (available, path)
        };
        RuntimeProbe {
            display_name: display_name(runtime).to_owned(),
            recommended_command: recommended.join(" "),
            name: runtime.wire_name().to_owned(),
            available,
            command,
            path,
        }
    })
    .collect()
}

/// DeepSeek can be chosen once Node and npm allow setup to install it. Its
/// path is the managed launcher, reported only for a complete installation
/// with a supported Node; a `dsh` elsewhere on PATH is never used.
fn deepseek_catalog_entry(env: &BTreeMap<String, String>) -> (bool, Option<PathBuf>) {
    let launcher = deepseek_preflight(&default_command(ActorRuntime::Deepseek), env)
        .ok()
        .and_then(|()| deepseek_executable(env));
    let available = launcher.is_some() || deepseek_bootstrap_preflight(env).is_ok();
    (available, launcher)
}

const fn display_name(runtime: ActorRuntime) -> &'static str {
    match runtime {
        ActorRuntime::Amp => "Amp",
        ActorRuntime::Antigravity => "Antigravity",
        ActorRuntime::Auggie => "Auggie",
        ActorRuntime::Claude => "Claude Code",
        ActorRuntime::Cline => "Cline CLI",
        ActorRuntime::Codex => "Codex CLI",
        ActorRuntime::Deepseek => "DeepSeek Harness",
        ActorRuntime::Copilot => "GitHub Copilot",
        ActorRuntime::Cursor => "Cursor Agent",
        ActorRuntime::Devin => "Devin",
        ActorRuntime::Kiro => "Kiro CLI",
        ActorRuntime::Kilo => "Kilo Code",
        ActorRuntime::Droid => "Factory Droid",
        ActorRuntime::Grok => "Grok",
        ActorRuntime::Hermes => "Hermes",
        ActorRuntime::Kimi => "Kimi Code",
        ActorRuntime::Opencode => "OpenCode",
        ActorRuntime::Pi => "Pi",
        ActorRuntime::WebModel => "ChatGPT Web Model",
        ActorRuntime::ChatgptDots => "ChatGPT Dots",
        ActorRuntime::GrokWebModel => "Grok Bot Web Model",
        ActorRuntime::Custom => "Custom",
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::deepseek_preflight;
    use super::{deepseek_home, default_command, detect_runtimes};
    use cccc_contracts::ActorRuntime;
    use std::collections::BTreeMap;

    #[test]
    fn codex_default_disables_startup_update_checks() {
        assert!(
            default_command(ActorRuntime::Codex)
                .windows(2)
                .any(|pair| pair == ["-c", "check_for_update_on_startup=false"])
        );
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_node_preflight_does_not_wait_indefinitely_for_version_output() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("node");
        // A supported version, so only the bounded wait can make this fail.
        std::fs::write(&path, "#!/bin/sh\n/bin/sleep 6\nprintf 'v24.9.0\\n'\n").expect("fixture");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("permissions");
        let env = BTreeMap::from([("PATH".into(), temp.path().display().to_string())]);
        let started = Instant::now();
        let result = super::deepseek_node_preflight(&env);
        assert!(result.is_err(), "a stalled version probe must fail");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn runtime_discovery_returns_frontend_contract() {
        let runtimes = detect_runtimes();
        let custom = runtimes
            .iter()
            .find(|runtime| runtime.name == "custom")
            .expect("custom runtime");
        assert_eq!(custom.display_name, "Custom");
        assert!(custom.available);
        assert!(runtimes.iter().any(|runtime| runtime.name == "codex"));
        let dots = runtimes
            .iter()
            .find(|runtime| runtime.name == "chatgpt_dots")
            .expect("Dots runtime");
        assert_eq!(dots.display_name, "ChatGPT Dots");
        assert!(dots.available);
        assert!(dots.recommended_command.is_empty());
        assert!(default_command(ActorRuntime::ChatgptDots).is_empty());
        let cline = runtimes
            .iter()
            .find(|runtime| runtime.name == "cline")
            .expect("cline runtime");
        assert_eq!(cline.display_name, "Cline CLI");
        assert_eq!(cline.recommended_command, "cline --tui --auto-approve true");
        assert_eq!(
            default_command(ActorRuntime::Cline),
            ["cline", "--tui", "--auto-approve", "true"]
        );
        assert_eq!(
            default_command(ActorRuntime::Deepseek),
            ["dsh", "--profile", "acp"]
        );
    }

    #[test]
    fn deepseek_node_range_starts_at_24_9() {
        for (version, supported) in [
            ("v22.19.0", false),
            ("v24.8.9", false),
            ("v24.9.0", true),
            ("v24.19.0", true),
            ("v25.0.0", true),
            ("", false),
        ] {
            assert_eq!(super::node_supported(version), supported, "{version}");
        }
    }

    #[test]
    fn saved_launcher_commands_are_managed_and_custom_commands_are_not() {
        let managed = |command: &[&str]| {
            super::deepseek_managed_command(
                &command
                    .iter()
                    .map(|part| (*part).to_owned())
                    .collect::<Vec<_>>(),
            )
        };
        assert!(managed(&["dsh", "--profile", "acp"]));
        assert!(managed(&["dsh-acp-demo"]));
        assert!(managed(&["/opt/node/bin/DSH"]));
        assert!(!managed(&["my-dsh"]));
        assert!(!managed(&[]));
    }

    #[test]
    fn deepseek_home_is_versioned_under_cccc_home() {
        let env = BTreeMap::from([("HOME".into(), "/users/test".into())]);
        assert_eq!(
            deepseek_home(&env),
            Some(std::path::PathBuf::from(format!(
                "/users/test/.cccc/runtimes/deepseek/{}",
                super::DEEPSEEK_RELEASE_VERSION
            )))
        );
        let env = BTreeMap::from([
            ("HOME".into(), "/users/test".into()),
            ("CCCC_HOME".into(), "/custom/cccc".into()),
        ]);
        assert_eq!(
            deepseek_home(&env),
            Some(std::path::PathBuf::from(format!(
                "/custom/cccc/runtimes/deepseek/{}",
                super::DEEPSEEK_RELEASE_VERSION
            )))
        );
    }

    #[cfg(unix)]
    #[test]
    fn executable_probe_rejects_non_executable_bare_and_absolute_paths() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let dsh = temp.path().join("dsh");
        std::fs::write(&dsh, "#!/bin/sh\n").expect("dsh");
        std::fs::set_permissions(&dsh, std::fs::Permissions::from_mode(0o644))
            .expect("permissions");
        let path = temp.path().display().to_string();
        assert!(super::resolve_executable_in_path("dsh", Some(&path)).is_none());
        assert!(
            super::resolve_executable_in_path(dsh.to_str().expect("dsh path is UTF-8"), None)
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_bootstrap_preflight_needs_node_and_npm_not_the_managed_app() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        for (name, body) in [
            ("node", "#!/bin/sh\nprintf 'v24.9.0\\n'\n"),
            ("npm", "#!/bin/sh\nexit 0\n"),
        ] {
            let path = temp.path().join(name);
            std::fs::write(&path, body).expect("fixture executable");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .expect("permissions");
        }
        let env = BTreeMap::from([
            ("PATH".into(), temp.path().display().to_string()),
            (
                "CCCC_HOME".into(),
                temp.path().join("cccc-home").display().to_string(),
            ),
        ]);
        let command = default_command(ActorRuntime::Deepseek);
        assert!(super::deepseek_bootstrap_preflight(&env).is_ok());
        assert!(super::deepseek_external_preflight(&command, &env).is_ok());
        assert!(deepseek_preflight(&command, &env).is_err());
        // Choosable, so setup can install it, but not detected.
        assert_eq!(super::deepseek_catalog_entry(&env), (true, None));
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_catalog_ignores_launchers_on_path_without_npm() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        for (name, body) in [
            ("node", "#!/bin/sh\nprintf 'v24.9.0\\n'\n"),
            ("dsh", "#!/bin/sh\nexit 0\n"),
            ("dsh-acp-demo", "#!/bin/sh\nexit 0\n"),
        ] {
            let path = temp.path().join(name);
            std::fs::write(&path, body).expect("fixture executable");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .expect("permissions");
        }
        let env = BTreeMap::from([
            ("PATH".into(), temp.path().display().to_string()),
            (
                "CCCC_HOME".into(),
                temp.path().join("cccc-home").display().to_string(),
            ),
        ]);
        assert_eq!(super::deepseek_catalog_entry(&env), (false, None));
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_catalog_detects_only_a_complete_installation_with_a_supported_node() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let env = installed_fixture(temp.path());
        let home = deepseek_home(&env).expect("installation root");
        assert_eq!(
            super::deepseek_catalog_entry(&env),
            (true, Some(home.join("node_modules/.bin/dsh")))
        );

        // An incomplete installation is not detected, even with a `dsh` on
        // PATH; npm keeps it choosable so setup can repair it.
        for name in ["dsh", "npm"] {
            let path = temp.path().join("bin").join(name);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("fixture executable");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("permissions");
        }
        std::fs::remove_file(home.join("node_modules/.bin/dsh")).expect("remove launcher");
        assert_eq!(super::deepseek_catalog_entry(&env), (true, None));

        std::fs::write(home.join("node_modules/.bin/dsh"), "#!/bin/sh\nexit 0\n")
            .expect("restore launcher");
        std::fs::set_permissions(
            home.join("node_modules/.bin/dsh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("launcher mode");
        let node = temp.path().join("bin/node");
        std::fs::write(&node, "#!/bin/sh\nprintf 'v22.19.0\\n'\n").expect("old node");
        assert_eq!(super::deepseek_catalog_entry(&env), (false, None));
    }

    #[test]
    fn deepseek_lockfile_rejects_a_mixed_transitive_preview_release() {
        let temp = tempfile::tempdir().expect("tempdir");
        let write = |packages: &serde_json::Value| {
            std::fs::write(
                temp.path().join("package-lock.json"),
                serde_json::to_vec(&serde_json::json!({"packages": packages})).expect("json"),
            )
            .expect("lockfile");
        };
        let mut packages = serde_json::json!({
            "": {"dependencies": super::deepseek_dependency_map()},
            "node_modules/@deepseek-ai/dsh": {"version": super::DEEPSEEK_RELEASE_VERSION},
            "node_modules/@deepseek-ai/dsh-acp-app": {"version": super::DEEPSEEK_RELEASE_VERSION},
            "node_modules/@deepseek-ai/dsh-acp": {"version": super::DEEPSEEK_RELEASE_VERSION}
        });
        write(&packages);
        assert!(super::deepseek_lockfile_is_pinned(temp.path()));
        // Pinned companions do not stand in for the launcher itself.
        let launcher = packages["node_modules/@deepseek-ai/dsh"].take();
        packages
            .as_object_mut()
            .expect("packages")
            .remove("node_modules/@deepseek-ai/dsh");
        write(&packages);
        assert!(!super::deepseek_lockfile_is_pinned(temp.path()));
        packages["node_modules/@deepseek-ai/dsh"] = launcher;
        packages["node_modules/@deepseek-ai/dsh-transitive"] =
            serde_json::json!({"version": "0.2.2-rc.4"});
        write(&packages);
        assert!(!super::deepseek_lockfile_is_pinned(temp.path()));
        packages["node_modules/@deepseek-ai/dsh-transitive"] =
            serde_json::json!({"version": super::DEEPSEEK_RELEASE_VERSION});
        packages[""] =
            serde_json::json!({"dependencies": {"@deepseek-ai/dsh-acp-demo": "0.1.0-rc.6"}});
        write(&packages);
        assert!(!super::deepseek_lockfile_is_pinned(temp.path()));
    }

    #[test]
    fn shared_manifest_vectors_close_the_runtime_value_domain() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/deepseek_manifest_vectors.json"
        )))
        .expect("manifest vectors");
        assert_eq!(
            vectors["valid"],
            super::canonical_deepseek_runtime_manifest()
        );
        assert!(super::is_canonical_deepseek_runtime_manifest(
            &vectors["valid"]
        ));
        for name in [
            "empty_shell",
            "unmanaged",
            "extra_dependency",
            "wrong_version",
            "previous_release",
            "truncated",
        ] {
            assert!(
                !super::is_canonical_deepseek_runtime_manifest(&vectors[name]),
                "{name} must fail closed"
            );
        }
    }

    /// A complete pinned installation as the launcher leaves it, with a
    /// supported Node on PATH. Returns the environment that finds both.
    #[cfg(unix)]
    fn installed_fixture(root: &std::path::Path) -> BTreeMap<String, String> {
        use std::os::unix::fs::PermissionsExt;

        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let node = bin.join("node");
        std::fs::write(&node, "#!/bin/sh\nprintf 'v24.9.0\\n'\n").expect("node");
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).expect("node mode");
        let env = BTreeMap::from([
            ("PATH".into(), bin.display().to_string()),
            (
                "CCCC_HOME".into(),
                root.join("cccc-home").display().to_string(),
            ),
            // Readiness comes from the real `node --version`, never this.
            ("CCCC_NODE_VERSION".into(), "0.0.0".into()),
        ]);
        let home = deepseek_home(&env).expect("installation root");
        let mut packages = serde_json::Map::from_iter([(
            String::new(),
            serde_json::json!({"dependencies": super::deepseek_dependency_map()}),
        )]);
        for package in [
            super::DEEPSEEK_PACKAGE,
            super::DEEPSEEK_ACP_PROFILE_PACKAGE,
            super::DEEPSEEK_ACP_BRIDGE_PACKAGE,
        ] {
            let dir = home.join("node_modules").join(package);
            std::fs::create_dir_all(&dir).expect("package");
            let mut manifest = serde_json::json!({"version": super::DEEPSEEK_RELEASE_VERSION});
            if package == super::DEEPSEEK_ACP_PROFILE_PACKAGE {
                manifest["dsh"] = serde_json::json!({"bundle": {"patch": "./cordis.patch.yml"}});
                std::fs::write(dir.join("cordis.patch.yml"), "[]\n").expect("bundle patch");
            }
            std::fs::write(
                dir.join("package.json"),
                serde_json::to_vec(&manifest).expect("json"),
            )
            .expect("manifest");
            packages.insert(
                format!("node_modules/{package}"),
                serde_json::json!({"version": super::DEEPSEEK_RELEASE_VERSION}),
            );
        }
        let launcher = home.join("node_modules/.bin/dsh");
        std::fs::create_dir_all(launcher.parent().expect("launcher dir")).expect(".bin");
        std::fs::write(&launcher, "#!/bin/sh\nexit 0\n").expect("launcher");
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))
            .expect("launcher mode");
        std::fs::write(
            home.join("package.json"),
            serde_json::to_vec(&super::canonical_deepseek_runtime_manifest()).expect("json"),
        )
        .expect("runtime manifest");
        std::fs::write(
            home.join("package-lock.json"),
            serde_json::to_vec(&serde_json::json!({"lockfileVersion": 3, "packages": packages}))
                .expect("json"),
        )
        .expect("lockfile");
        env
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_preflight_checks_the_pinned_installation_not_path() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let env = installed_fixture(temp.path());
        let home = deepseek_home(&env).expect("installation root");
        let command = default_command(ActorRuntime::Deepseek);
        for saved in [command.clone(), vec!["dsh-acp-demo".into()]] {
            deepseek_preflight(&saved, &env).expect("complete installation");
        }
        assert_eq!(
            super::deepseek_executable(&env),
            Some(home.join("node_modules/.bin/dsh"))
        );

        let bridge = home
            .join("node_modules")
            .join(super::DEEPSEEK_ACP_BRIDGE_PACKAGE)
            .join("package.json");
        std::fs::write(&bridge, "{\"version\":\"0.2.2-rc.4\"}\n").expect("bridge mismatch");
        let error = deepseek_preflight(&command, &env).expect_err("mixed release");
        assert!(
            error.contains(&format!(
                "@deepseek-ai/dsh-acp@{}",
                super::DEEPSEEK_RELEASE_VERSION
            )),
            "{error}"
        );
        std::fs::write(
            &bridge,
            format!("{{\"version\":\"{}\"}}\n", super::DEEPSEEK_RELEASE_VERSION),
        )
        .expect("restore bridge");

        assert!(super::deepseek_packages_to_reinstall(&home).is_empty());
        let profile = super::deepseek_acp_profile(&home);
        let patch = profile.join("cordis.patch.yml");
        std::fs::remove_file(&patch).expect("remove bundle patch");
        let error = deepseek_preflight(&command, &env).expect_err("incomplete profile bundle");
        assert!(error.contains("ACP profile bundle"), "{error}");
        // npm would keep this package as it is, so setup unpacks it again.
        assert_eq!(super::deepseek_packages_to_reinstall(&home), [profile]);
        std::fs::write(&patch, "[]\n").expect("restore bundle patch");
        deepseek_preflight(&command, &env).expect("restored installation");
        assert!(super::deepseek_packages_to_reinstall(&home).is_empty());

        // Without the managed launcher, a dsh elsewhere on PATH does not count.
        let path_dsh = temp.path().join("bin/dsh");
        std::fs::write(&path_dsh, "#!/bin/sh\nexit 0\n").expect("dsh on PATH");
        std::fs::set_permissions(&path_dsh, std::fs::Permissions::from_mode(0o755))
            .expect("dsh mode");
        std::fs::remove_file(home.join("node_modules/.bin/dsh")).expect("remove launcher");
        let error = deepseek_preflight(&command, &env).expect_err("managed launcher missing");
        assert_eq!(error, "deepseek executable not found: dsh");
        // The installation check reaches the same verdict, so setup repairs it.
        assert_eq!(super::deepseek_installation_ready(&home), Err(error));
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_preflight_reports_an_unsupported_node_before_the_installation() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let env = installed_fixture(temp.path());
        let node = temp.path().join("bin/node");
        std::fs::write(&node, "#!/bin/sh\nprintf 'v22.19.0\\n'\n").expect("old node");
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).expect("node mode");
        let command = default_command(ActorRuntime::Deepseek);
        let error = deepseek_preflight(&command, &env).expect_err("old node");
        assert!(
            error.contains(">=24.9.0") && error.contains("v22.19.0"),
            "{error}"
        );
        std::fs::remove_dir_all(deepseek_home(&env).expect("installation root"))
            .expect("uninstall");
        let error = super::deepseek_external_preflight(&command, &env).expect_err("old node");
        assert!(error.contains(">=24.9.0"), "{error}");
    }
}
