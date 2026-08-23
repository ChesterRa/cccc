use crate::executable::{is_executable_file, resolve_executable_in_path};
use cccc_contracts::ActorRuntime;
pub use cccc_contracts::{
    DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION, DEEPSEEK_ACP_PACKAGE,
    DEEPSEEK_ACP_SDK_VERSION, DEEPSEEK_ACP_VERSION, DEEPSEEK_LLM_ADAPTER_PACKAGE,
    DEEPSEEK_LLM_ADAPTER_VERSION, DEEPSEEK_MAX_OUTPUT_TOKENS, DEEPSEEK_MCP_CLIENT_PACKAGE,
    DEEPSEEK_MCP_CLIENT_VERSION, DEEPSEEK_NODE_RANGE, DEEPSEEK_NPM_BEFORE,
    DEEPSEEK_RELEASE_VERSION, DEEPSEEK_TURN_TIMEOUT_SECONDS,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    for (package, version) in [
        (DEEPSEEK_ACP_PACKAGE, DEEPSEEK_ACP_VERSION),
        (DEEPSEEK_MCP_CLIENT_PACKAGE, DEEPSEEK_MCP_CLIENT_VERSION),
        (DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION),
        (DEEPSEEK_LLM_ADAPTER_PACKAGE, DEEPSEEK_LLM_ADAPTER_VERSION),
    ] {
        let manifest = dsh_home
            .join("node_modules")
            .join(package)
            .join("package.json");
        let found = fs::read(&manifest)
            .ok()
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
            .and_then(|value| {
                value
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        if found.as_deref() != Some(version) {
            return Err(format!("{package}@{version} is required"));
        }
    }
    let runtime_manifest: Value = serde_json::from_slice(
        &fs::read(dsh_home.join("package.json"))
            .map_err(|_| "DeepSeek managed runtime manifest is missing")?,
    )
    .map_err(|_| "DeepSeek managed runtime manifest is invalid")?;
    if !is_canonical_deepseek_runtime_manifest(&runtime_manifest) {
        return Err("DeepSeek managed runtime dependency set is not canonical".to_owned());
    }
    if !deepseek_lockfile_is_pinned(&dsh_home) {
        return Err(format!(
            "DeepSeek dependency graph must be pinned to {DEEPSEEK_RELEASE_VERSION}"
        ));
    }
    let profile = dsh_home.join("profiles").join("cccc-acp");
    let manifest: Value = serde_json::from_slice(
        &fs::read(profile.join("package.json"))
            .map_err(|_| "deepseek cccc-acp profile is missing")?,
    )
    .map_err(|_| "deepseek cccc-acp profile manifest is invalid")?;
    if !is_canonical_deepseek_profile_manifest(&manifest) {
        return Err("deepseek cccc-acp profile is unmanaged".to_owned());
    }
    let config = fs::read_to_string(profile.join("cordis.yml"))
        .map_err(|_| "deepseek cccc-acp profile config is missing")?;
    if !is_canonical_deepseek_config(&config) {
        return Err("deepseek cccc-acp profile config is invalid".to_owned());
    }
    Ok(())
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

pub fn deepseek_external_preflight(
    command: &[String],
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let executable = command.first().map(String::as_str).unwrap_or("dsh");
    if resolve_executable_in_path(executable, env.get("PATH").map(String::as_str)).is_none() {
        return Err(format!("deepseek executable not found: {executable}"));
    }
    deepseek_node_preflight(env)
}

pub fn deepseek_bootstrap_preflight(env: &BTreeMap<String, String>) -> Result<(), String> {
    deepseek_node_preflight(env)?;
    if resolve_executable_in_path("npm", env.get("PATH").map(String::as_str)).is_none() {
        return Err("npm is required to install DeepSeek Harness".to_owned());
    }
    Ok(())
}

fn deepseek_node_preflight(env: &BTreeMap<String, String>) -> Result<(), String> {
    let mut node_command = Command::new("node");
    for (key, value) in env {
        if key != "CCCC_NODE_VERSION" {
            node_command.env(key, value);
        }
    }
    let node = node_command
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    if !node_supported(&node) {
        return Err(format!(
            "DeepSeek Harness requires Node {DEEPSEEK_NODE_RANGE} (found {})",
            if node.is_empty() { "unknown" } else { &node }
        ));
    }
    Ok(())
}

pub fn deepseek_lockfile_is_pinned(dsh_home: &std::path::Path) -> bool {
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
    let mut matched = false;
    for (lock_path, entry) in packages {
        let name = lock_path
            .rsplit("node_modules/")
            .next()
            .unwrap_or(lock_path);
        if name != "@deepseek-ai/dsh" && !name.starts_with("@deepseek-ai/dsh-") {
            continue;
        }
        matched = true;
        if entry.get("version").and_then(Value::as_str) != Some(DEEPSEEK_RELEASE_VERSION) {
            return false;
        }
    }
    matched
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
    [
        (DEEPSEEK_ACP_PACKAGE, DEEPSEEK_ACP_VERSION),
        (DEEPSEEK_MCP_CLIENT_PACKAGE, DEEPSEEK_MCP_CLIENT_VERSION),
        (DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION),
        (DEEPSEEK_LLM_ADAPTER_PACKAGE, DEEPSEEK_LLM_ADAPTER_VERSION),
    ]
    .into_iter()
    .map(|(package, version)| (package.to_owned(), Value::String(version.to_owned())))
    .collect()
}

/// Validate the dedicated ACP app composition. The old `dsh --profile
/// cccc-acp` command booted the one-shot headless bundle and exited before ACP
/// could accept initialize; the app composition is intentionally small and
/// must not contain the headless CLI runner.
pub fn is_canonical_deepseek_config(config: &str) -> bool {
    let lines = config.lines().collect::<Vec<_>>();
    let max_tokens = format!("    maxTokens: {DEEPSEEK_MAX_OUTPUT_TOKENS}");
    let expected = [
        Some("- id: llm-deepseek"),
        Some("  name: '@deepseek-ai/dsh-llm-deepseek'"),
        Some("  config:"),
        Some(max_tokens.as_str()),
        Some("- id: acp-demo"),
        Some("  name: '@deepseek-ai/dsh-acp-demo'"),
        Some("  config:"),
        Some("    provider: deepseek-official"),
        Some("    model: deepseek-v4-flash"),
        Some("    workspaceContext: false"),
        Some("    persistenceRoot: !!js process.env.CCCC_DEEPSEEK_SESSION_ROOT"),
        Some("- id: cccc-mcp"),
        Some("  name: '@deepseek-ai/dsh-mcp-client'"),
        Some("  config:"),
        Some("    transport: stdio"),
        Some("    serverName: cccc"),
        None,
        Some("    args: [mcp]"),
        Some("    env:"),
        Some("      CCCC_HOME: !!js process.env.CCCC_HOME"),
        Some("      CCCC_GROUP_ID: !!js process.env.CCCC_GROUP_ID"),
        Some("      CCCC_ACTOR_ID: !!js process.env.CCCC_ACTOR_ID"),
        Some("    failOnStartupError: true"),
    ];
    if lines.len() != expected.len()
        || lines
            .iter()
            .zip(expected)
            .any(|(actual, wanted)| wanted.is_some_and(|wanted| actual != &wanted))
    {
        return false;
    }
    let Some(command_index) = expected.iter().position(Option::is_none) else {
        return false;
    };
    let Some(path) = lines[command_index]
        .strip_prefix("    command: '")
        .and_then(|value| value.strip_suffix('\''))
    else {
        return false;
    };
    let path = path.replace("''", "'");
    is_executable_file(std::path::Path::new(&path))
}

fn node_supported(raw: &str) -> bool {
    let numbers: Vec<u32> = raw
        .trim_start_matches('v')
        .split('.')
        .take(3)
        .filter_map(|part| part.parse().ok())
        .collect();
    numbers.len() >= 2 && (numbers[0] >= 24 || (numbers[0] == 22 && numbers[1] >= 19))
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
            "codex -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox --search"
        }
        ActorRuntime::Deepseek => "dsh-acp-demo",
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
        ActorRuntime::WebModel | ActorRuntime::Custom => "",
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
        "web_model",
        "custom"
    ]))
    .unwrap_or_default()
    .into_iter()
    .map(|runtime| {
        let recommended = default_command(runtime);
        let command = recommended.first().cloned().unwrap_or_default();
        let mut discovery_env = std::env::vars().collect::<BTreeMap<_, _>>();
        if runtime == ActorRuntime::Deepseek {
            prepend_deepseek_bin(&mut discovery_env);
        }
        let path =
            resolve_executable_in_path(&command, discovery_env.get("PATH").map(String::as_str));
        let name = runtime_name(runtime).to_owned();
        RuntimeProbe {
            display_name: display_name(runtime).to_owned(),
            recommended_command: recommended.join(" "),
            name,
            available: if runtime == ActorRuntime::Deepseek {
                deepseek_catalog_available(&recommended, &discovery_env)
            } else {
                matches!(runtime, ActorRuntime::WebModel | ActorRuntime::Custom) || path.is_some()
            },
            command,
            path,
        }
    })
    .collect()
}

fn deepseek_catalog_available(command: &[String], env: &BTreeMap<String, String>) -> bool {
    deepseek_preflight(command, env).is_ok() || deepseek_bootstrap_preflight(env).is_ok()
}

fn prepend_deepseek_bin(env: &mut BTreeMap<String, String>) {
    let Some(home) = deepseek_home(env) else {
        return;
    };
    let local_bin = home.join("node_modules").join(".bin");
    let mut paths = vec![local_bin.clone()];
    if let Some(existing) = env.get("PATH") {
        paths.extend(std::env::split_paths(existing).filter(|path| path != &local_bin));
    }
    if let Ok(path) = std::env::join_paths(paths) {
        env.insert("PATH".into(), path.to_string_lossy().into_owned());
    }
}

const fn runtime_name(runtime: ActorRuntime) -> &'static str {
    match runtime {
        ActorRuntime::Amp => "amp",
        ActorRuntime::Antigravity => "antigravity",
        ActorRuntime::Auggie => "auggie",
        ActorRuntime::Claude => "claude",
        ActorRuntime::Cline => "cline",
        ActorRuntime::Codex => "codex",
        ActorRuntime::Deepseek => "deepseek",
        ActorRuntime::Copilot => "copilot",
        ActorRuntime::Cursor => "cursor",
        ActorRuntime::Devin => "devin",
        ActorRuntime::Kiro => "kiro",
        ActorRuntime::Kilo => "kilo",
        ActorRuntime::Droid => "droid",
        ActorRuntime::Grok => "grok",
        ActorRuntime::Hermes => "hermes",
        ActorRuntime::Kimi => "kimi",
        ActorRuntime::Opencode => "opencode",
        ActorRuntime::WebModel => "web_model",
        ActorRuntime::Custom => "custom",
    }
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
        ActorRuntime::Kimi => "Kimi CLI",
        ActorRuntime::Opencode => "OpenCode",
        ActorRuntime::WebModel => "Web Model",
        ActorRuntime::Custom => "Custom",
    }
}

pub fn is_canonical_deepseek_profile_manifest(manifest: &Value) -> bool {
    let Some(object) = manifest.as_object() else {
        return false;
    };
    if object.get("name") != Some(&Value::String("dsh-profile-cccc-acp".to_owned()))
        || object.get("private") != Some(&Value::Bool(true))
        || object.get("ccccManaged") != Some(&Value::Bool(true))
    {
        return false;
    }
    let Some(dependencies) = object.get("dependencies").and_then(Value::as_object) else {
        return false;
    };
    if dependencies.get(DEEPSEEK_ACP_PACKAGE)
        != Some(&Value::String(DEEPSEEK_ACP_VERSION.to_owned()))
        || dependencies.get(DEEPSEEK_MCP_CLIENT_PACKAGE)
            != Some(&Value::String(DEEPSEEK_MCP_CLIENT_VERSION.to_owned()))
        || dependencies.get(DEEPSEEK_ACP_APP_PACKAGE)
            != Some(&Value::String(DEEPSEEK_ACP_APP_VERSION.to_owned()))
        || dependencies.get(DEEPSEEK_LLM_ADAPTER_PACKAGE)
            != Some(&Value::String(DEEPSEEK_LLM_ADAPTER_VERSION.to_owned()))
    {
        return false;
    }
    dependencies.len() == 4
}

#[cfg(test)]
mod tests {
    use super::{deepseek_home, deepseek_preflight, default_command, detect_runtimes};
    use cccc_contracts::ActorRuntime;
    use std::collections::BTreeMap;

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
        assert_eq!(default_command(ActorRuntime::Deepseek), ["dsh-acp-demo"]);
    }

    #[test]
    fn deepseek_home_is_versioned_under_cccc_home() {
        let env = BTreeMap::from([("HOME".into(), "/users/test".into())]);
        assert_eq!(
            deepseek_home(&env),
            Some(std::path::PathBuf::from(
                "/users/test/.cccc/runtimes/deepseek/0.1.0-rc.6"
            ))
        );
        let env = BTreeMap::from([
            ("HOME".into(), "/users/test".into()),
            ("CCCC_HOME".into(), "/custom/cccc".into()),
        ]);
        assert_eq!(
            deepseek_home(&env),
            Some(std::path::PathBuf::from(
                "/custom/cccc/runtimes/deepseek/0.1.0-rc.6"
            ))
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
            ("node", "#!/bin/sh\nprintf 'v24.0.0\\n'\n"),
            ("npm", "#!/bin/sh\nexit 0\n"),
        ] {
            let path = temp.path().join(name);
            std::fs::write(&path, body).expect("fixture executable");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .expect("permissions");
        }
        let env = BTreeMap::from([("PATH".into(), temp.path().display().to_string())]);
        assert!(super::deepseek_bootstrap_preflight(&env).is_ok());
        assert!(super::deepseek_external_preflight(&["dsh-acp-demo".into()], &env).is_err());
        assert!(super::deepseek_catalog_available(
            &["dsh-acp-demo".into()],
            &env
        ));
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_catalog_rejects_an_unmanaged_app_without_npm() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        for (name, body) in [
            ("node", "#!/bin/sh\nprintf 'v24.0.0\\n'\n"),
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
        assert!(!super::deepseek_catalog_available(
            &["dsh-acp-demo".into()],
            &env
        ));
    }

    #[test]
    fn deepseek_lockfile_rejects_a_mixed_transitive_preview_release() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut packages = serde_json::json!({
            "": {"dependencies": super::deepseek_dependency_map()},
            "node_modules/@deepseek-ai/dsh-acp": {"version": "0.1.0-rc.6"}
        });
        std::fs::write(
            temp.path().join("package-lock.json"),
            serde_json::to_vec(&serde_json::json!({"packages": packages.clone()})).expect("json"),
        )
        .expect("lockfile");
        assert!(super::deepseek_lockfile_is_pinned(temp.path()));
        packages["node_modules/@deepseek-ai/dsh-transitive"] =
            serde_json::json!({"version": "0.1.0-rc.7"});
        std::fs::write(
            temp.path().join("package-lock.json"),
            serde_json::to_vec(&serde_json::json!({"packages": packages})).expect("json"),
        )
        .expect("lockfile");
        assert!(!super::deepseek_lockfile_is_pinned(temp.path()));
    }

    #[test]
    fn shared_manifest_vectors_close_the_bundle_value_domain() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/deepseek_manifest_vectors.json"
        )))
        .expect("manifest vectors");
        assert!(super::is_canonical_deepseek_profile_manifest(
            &vectors["valid"]
        ));
        for name in [
            "empty_shell",
            "extra_dependency",
            "wrong_version",
            "missing_app",
            "missing_adapter",
            "adapter_version_mismatch",
            "truncated",
        ] {
            assert!(
                !super::is_canonical_deepseek_profile_manifest(&vectors[name]),
                "{name} must fail closed"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_preflight_uses_real_node_and_manifests_not_env_version() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let dsh = temp.path().join("dsh");
        let app = temp.path().join("dsh-acp-demo");
        let node = temp.path().join("node");
        std::fs::write(&dsh, "#!/bin/sh\nexit 0\n").expect("dsh");
        std::fs::write(&app, "#!/bin/sh\nexit 0\n").expect("app");
        std::fs::write(&node, "#!/bin/sh\nprintf 'v24.0.0\\n'\n").expect("node");
        for path in [&dsh, &app, &node] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .expect("executable");
        }
        let cccc_home = temp.path().join("cccc-home");
        let home = cccc_home
            .join("runtimes/deepseek")
            .join(super::DEEPSEEK_RELEASE_VERSION);
        for package in [
            super::DEEPSEEK_ACP_PACKAGE,
            super::DEEPSEEK_MCP_CLIENT_PACKAGE,
            super::DEEPSEEK_ACP_APP_PACKAGE,
            super::DEEPSEEK_LLM_ADAPTER_PACKAGE,
        ] {
            let package_dir = home.join("node_modules").join(package);
            std::fs::create_dir_all(&package_dir).expect("package");
            std::fs::write(
                package_dir.join("package.json"),
                format!("{{\"version\":\"{}\"}}\n", super::DEEPSEEK_RELEASE_VERSION),
            )
            .expect("manifest");
        }
        let mut packages = [
            super::DEEPSEEK_ACP_PACKAGE,
            super::DEEPSEEK_MCP_CLIENT_PACKAGE,
            super::DEEPSEEK_ACP_APP_PACKAGE,
            super::DEEPSEEK_LLM_ADAPTER_PACKAGE,
        ]
        .into_iter()
        .map(|package| {
            (
                format!("node_modules/{package}"),
                serde_json::json!({"version": super::DEEPSEEK_RELEASE_VERSION}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
        packages.insert(
            "".into(),
            serde_json::json!({"dependencies": super::deepseek_dependency_map()}),
        );
        std::fs::write(
            home.join("package.json"),
            serde_json::to_vec(&super::canonical_deepseek_runtime_manifest())
                .expect("runtime manifest"),
        )
        .expect("runtime manifest");
        std::fs::write(
            home.join("package-lock.json"),
            serde_json::to_vec(&serde_json::json!({"lockfileVersion":3,"packages":packages}))
                .expect("lock json"),
        )
        .expect("lockfile");
        let profile = home.join("profiles/cccc-acp");
        std::fs::create_dir_all(&profile).expect("profile");
        std::fs::write(
            profile.join("package.json"),
            "{\"name\":\"dsh-profile-cccc-acp\",\"private\":true,\"ccccManaged\":true,\"dependencies\":{\"@deepseek-ai/dsh-acp\":\"0.1.0-rc.6\",\"@deepseek-ai/dsh-mcp-client\":\"0.1.0-rc.6\",\"@deepseek-ai/dsh-acp-demo\":\"0.1.0-rc.6\",\"@deepseek-ai/dsh-llm-deepseek\":\"0.1.0-rc.6\"}}\n",
        )
            .expect("profile manifest");
        std::fs::write(
            profile.join("cordis.yml"),
            format!(
                "- id: llm-deepseek\n  name: '@deepseek-ai/dsh-llm-deepseek'\n  config:\n    maxTokens: {}\n- id: acp-demo\n  name: '@deepseek-ai/dsh-acp-demo'\n  config:\n    provider: deepseek-official\n    model: deepseek-v4-flash\n    workspaceContext: false\n    persistenceRoot: !!js process.env.CCCC_DEEPSEEK_SESSION_ROOT\n- id: cccc-mcp\n  name: '@deepseek-ai/dsh-mcp-client'\n  config:\n    transport: stdio\n    serverName: cccc\n    command: '{}'\n    args: [mcp]\n    env:\n      CCCC_HOME: !!js process.env.CCCC_HOME\n      CCCC_GROUP_ID: !!js process.env.CCCC_GROUP_ID\n      CCCC_ACTOR_ID: !!js process.env.CCCC_ACTOR_ID\n    failOnStartupError: true\n",
                super::DEEPSEEK_MAX_OUTPUT_TOKENS,
                dsh.display()
            ),
        )
        .expect("profile config");
        let mut env = BTreeMap::new();
        env.insert("PATH".into(), temp.path().display().to_string());
        env.insert("CCCC_HOME".into(), cccc_home.display().to_string());
        env.insert("CCCC_NODE_VERSION".into(), "0.0.0".into());
        assert!(deepseek_preflight(&["dsh-acp-demo".into()], &env).is_ok());
        let adapter_manifest = home
            .join("node_modules")
            .join(super::DEEPSEEK_LLM_ADAPTER_PACKAGE)
            .join("package.json");
        std::fs::write(&adapter_manifest, "{\"version\":\"0.1.0-rc.7\"}\n")
            .expect("adapter mismatch");
        let mismatch = deepseek_preflight(&["dsh-acp-demo".into()], &env)
            .expect_err("mixed preview tuple must fail closed");
        assert!(mismatch.contains("@deepseek-ai/dsh-llm-deepseek@0.1.0-rc.6"));
        std::fs::remove_file(adapter_manifest).expect("remove adapter manifest");
        assert!(deepseek_preflight(&["dsh-acp-demo".into()], &env).is_err());
        std::fs::remove_dir_all(profile).expect("remove profile");
        assert!(super::deepseek_external_preflight(&["dsh-acp-demo".into()], &env,).is_ok());
        assert!(deepseek_preflight(&["dsh-acp-demo".into()], &env).is_err());
    }
}
