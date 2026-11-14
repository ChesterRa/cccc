use cccc_core::HomeLayout;
use cccc_core::fs::{read_json, with_exclusive_lock, write_json};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::dispatch::OpError;

pub(super) fn ensure_installed(
    home: &HomeLayout,
    capability_id: &str,
    record: &Value,
) -> Result<Option<Value>, OpError> {
    let mode = text(record, "install_mode");
    if !matches!(mode.as_str(), "package" | "command" | "remote_only") {
        return Ok(None);
    }
    if let Some(artifact) = installed_artifact(home, capability_id)? {
        return Ok(Some(artifact));
    }
    let artifact = prepare(capability_id, record)?;
    persist(home, capability_id, &artifact)?;
    Ok(Some(artifact))
}

/// Prepare without changing the active installation. Explicit updates commit
/// their candidate together with every affected pointer in one state write.
pub(super) fn prepare(capability_id: &str, record: &Value) -> Result<Value, OpError> {
    let mode = text(record, "install_mode");
    let mut artifact = match mode.as_str() {
        "remote_only" => install_remote(capability_id, record)?,
        "command" | "package" => install_stdio(capability_id, &mode, record)?,
        _ => return Err(install_error("unsupported execution recipe")),
    };
    let fingerprint = super::recipe::fingerprint(record)?;
    // Different recipes must not overwrite an older artifact still referenced
    // elsewhere, even when they happen to resolve to the same command today.
    let identity = Sha256::digest(format!(
        "{capability_id}:{fingerprint}:{}",
        artifact["invoker"]
    ));
    let artifact_id = format!("art_{identity:x}");
    artifact["artifact_id"] = json!(artifact_id);
    artifact["install_key"] = json!(artifact_id);
    artifact["recipe_fingerprint"] = json!(fingerprint);
    Ok(artifact)
}

fn install_stdio(capability_id: &str, mode: &str, record: &Value) -> Result<Value, OpError> {
    let spec = record
        .get("install_spec")
        .and_then(Value::as_object)
        .ok_or_else(|| install_error("install_spec is required"))?;
    require_environment(record)?;
    let command = resolve_command(mode, spec)?;
    ensure_command_available(&command[0])?;
    let environment = required_environment_values(record);
    let tools = probe_stdio(capability_id, &command, &environment)?;
    let installer = installer_name(&command[0], mode);
    Ok(artifact(
        capability_id,
        mode,
        &installer,
        json!({"type":if mode=="command"{"command_stdio"}else{"package_stdio"},"command":command}),
        tools,
    ))
}

/// Read inherited values only when starting a probe or call; artifacts retain no values.
pub(super) fn required_environment_values(record: &Value) -> Map<String, Value> {
    required_environment_names(record)
        .into_iter()
        .filter_map(|name| {
            std::env::var(&name)
                .ok()
                .map(|value| (name, Value::String(value)))
        })
        .collect()
}

fn install_remote(capability_id: &str, record: &Value) -> Result<Value, OpError> {
    let spec = record
        .get("install_spec")
        .and_then(Value::as_object)
        .ok_or_else(|| install_error("install_spec is required"))?;
    let url = spec
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
        .ok_or_else(|| install_error("remote MCP URL must use http or https"))?;
    let credential = if super::credentials::bearer(record) {
        let env = super::credentials::env_name(record).ok_or_else(|| {
            install_error("Bearer MCP requires a valid environment-key reference")
        })?;
        Some(json!({"kind":"bearer","env":env}))
    } else {
        None
    };
    let tools = if credential.is_some() {
        Vec::new()
    } else {
        probe_http(capability_id, url)?
    };
    let mut invoker = json!({"type":"remote_http","url":url});
    if let Some(credential) = credential {
        invoker["credential"] = credential;
    }
    Ok(artifact(
        capability_id,
        "remote_only",
        "remote_http",
        invoker,
        tools,
    ))
}

fn resolve_command(mode: &str, spec: &Map<String, Value>) -> Result<Vec<String>, OpError> {
    if mode == "command" {
        return command_candidates(spec)
            .into_iter()
            .find(|command| command.first().is_some_and(|item| command_available(item)))
            .ok_or_else(|| install_error("no available command candidate"));
    }
    let registry = text_map(spec, "registry_type").to_ascii_lowercase();
    let identifier = text_map(spec, "identifier");
    if identifier.is_empty() {
        return Err(install_error("package identifier is required"));
    }
    let version = text_map(spec, "version");
    let runtime = arguments(spec.get("runtime_arguments"));
    let package = arguments(spec.get("package_arguments"));
    match registry.as_str() {
        "npm" | "javascript" | "node" | "nodejs" => {
            let package_name = if version.is_empty() || package_has_version(&identifier) {
                identifier
            } else {
                format!("{identifier}@{version}")
            };
            Ok([vec!["npx".into(), "-y".into(), package_name], package].concat())
        }
        "pypi" | "python" | "pip" | "pipx" | "uvx" => {
            let package_name = if version.is_empty() {
                identifier.clone()
            } else {
                format!("{identifier}@{version}")
            };
            let hint = text_map(spec, "runtime_hint").to_ascii_lowercase();
            if hint == "pipx" {
                return Ok([
                    vec![
                        "pipx".into(),
                        "run".into(),
                        "--spec".into(),
                        package_name,
                        identifier,
                    ],
                    package,
                ]
                .concat());
            }
            Ok([
                vec!["uvx".into()],
                if runtime.is_empty() {
                    vec![package_name]
                } else {
                    runtime
                },
                package,
            ]
            .concat())
        }
        "oci" | "docker" | "container" | "podman" => {
            let hint = text_map(spec, "runtime_hint").to_ascii_lowercase();
            let engine = if hint == "podman" { "podman" } else { "docker" };
            let mut command = vec![engine.into(), "run".into(), "-i".into(), "--rm".into()];
            for name in required_environment(spec) {
                command.extend(["-e".into(), name]);
            }
            command.extend(runtime);
            command.push(identifier);
            command.extend(package);
            Ok(command)
        }
        _ => command_candidates(spec)
            .into_iter()
            .next()
            .ok_or_else(|| install_error(format!("unsupported package registry: {registry}"))),
    }
}

pub(super) fn prerequisites_available(record: &Value) -> bool {
    let Some(spec) = record.get("install_spec").and_then(Value::as_object) else {
        return false;
    };
    resolve_command(&text(record, "install_mode"), spec)
        .ok()
        .and_then(|command| command.into_iter().next())
        .is_some_and(|command| command_available(&command))
}

fn probe_stdio(
    capability_id: &str,
    command: &[String],
    environment: &Map<String, Value>,
) -> Result<Vec<Value>, OpError> {
    let tools = super::mcp_stdio::list_tools(command, environment, timeout())
        .map_err(|error| install_error(error.message))?;
    Ok(normalize_tools(capability_id, Some(&Value::Array(tools))))
}

fn probe_http(capability_id: &str, url: &str) -> Result<Vec<Value>, OpError> {
    let tools = super::mcp_http::list_tools(url, timeout())
        .map_err(|error| install_error(error.message))?;
    Ok(normalize_tools(capability_id, Some(&Value::Array(tools))))
}

pub(super) fn normalize_tools(capability_id: &str, tools: Option<&Value>) -> Vec<Value> {
    tools
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let real = tool.get("name")?.as_str()?.trim();
            if real.is_empty() {
                return None;
            }
            Some(json!({
                "name":cccc_core::capabilities::external_tool_name(capability_id, real),
                "real_tool_name":real,
                "description":tool.get("description").and_then(Value::as_str).unwrap_or(""),
                "inputSchema":tool.get("inputSchema").filter(|value| value.is_object()).cloned().unwrap_or_else(||json!({"type":"object","properties":{},"required":[]})),
            }))
        })
        .collect()
}

fn artifact(
    capability_id: &str,
    mode: &str,
    installer: &str,
    invoker: Value,
    tools: Vec<Value>,
) -> Value {
    let digest = Sha256::digest(format!("{capability_id}:{mode}:{invoker}"));
    let artifact_id = format!(
        "art_{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7]
    );
    json!({
        "artifact_id":artifact_id,"install_key":artifact_id,"state":"installed",
        "installer":installer,"install_mode":mode,"invoker":invoker,"tools":tools,
        "last_error":"","last_error_code":"","updated_at":cccc_contracts::utc_now(),
        "capability_ids":[capability_id],
    })
}

fn persist(home: &HomeLayout, capability_id: &str, artifact: &Value) -> Result<(), OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut runtime = if path.exists() {
            read_json::<Value>(&path)?
        } else {
            json!({"v":2,"created_at":cccc_contracts::utc_now()})
        };
        runtime["v"] = json!(2);
        runtime["updated_at"] = json!(cccc_contracts::utc_now());
        let artifact_id = artifact["artifact_id"].as_str().unwrap_or_default();
        object(&mut runtime, "artifacts").insert(artifact_id.into(), artifact.clone());
        object(&mut runtime, "capability_artifacts")
            .insert(capability_id.into(), json!(artifact_id));
        write_json(&path, &runtime)
    })
    .map_err(OpError::io)
}

pub(super) fn installed_artifact(
    home: &HomeLayout,
    capability_id: &str,
) -> Result<Option<Value>, OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    if !path.exists() {
        return Ok(None);
    }
    let runtime: Value = read_json(&path).map_err(OpError::io)?;
    let Some(id) = runtime
        .pointer(&format!(
            "/capability_artifacts/{}",
            escape_pointer(capability_id)
        ))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    Ok(runtime
        .get("artifacts")
        .and_then(|items| items.get(id))
        .filter(|item| {
            matches!(
                item["state"].as_str(),
                Some("installed" | "ready" | "active")
            )
        })
        .cloned())
}

fn required_environment_names(record: &Value) -> Vec<String> {
    let mut names = record
        .get("install_spec")
        .and_then(Value::as_object)
        .map(required_environment)
        .unwrap_or_default();
    if let Some(record) = record.as_object() {
        names.extend(required_environment(record));
    }
    names.sort();
    names.dedup();
    names
}

/// Return key names only; suitability reads must never expose environment values.
pub(super) fn missing_environment(record: &Value) -> Vec<String> {
    required_environment_names(record)
        .into_iter()
        .filter(|name| std::env::var(name).map_or(true, |value| value.trim().is_empty()))
        .collect()
}

fn require_environment(record: &Value) -> Result<(), OpError> {
    let missing = missing_environment(record);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(install_error(format!(
            "missing required environment: {}",
            missing.join(", ")
        )))
    }
}

fn required_environment(spec: &Map<String, Value>) -> Vec<String> {
    spec.get("required_env")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn arguments(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            if let Some(value) = entry.as_str() {
                return vec![value.to_owned()];
            }
            let Some(entry) = entry.as_object() else {
                return Vec::new();
            };
            let name = text_map(entry, "name");
            let value = expand_environment(&text_map(entry, "value"));
            [name, value]
                .into_iter()
                .filter(|item| !item.is_empty())
                .collect()
        })
        .collect()
}

fn command_candidates(spec: &Map<String, Value>) -> Vec<Vec<String>> {
    ["command_candidates", "fallback_command_candidates"]
        .into_iter()
        .flat_map(|key| {
            spec.get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|candidate| candidate.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .collect::<Vec<_>>()
        })
        .filter(|items: &Vec<String>| !items.is_empty())
        .collect()
}

fn expand_environment(value: &str) -> String {
    let mut output = value.to_owned();
    for (key, current) in std::env::vars() {
        output = output.replace(&format!("{{{key}}}"), &current);
    }
    output
}

fn ensure_command_available(command: &str) -> Result<(), OpError> {
    if command_available(command) {
        Ok(())
    } else {
        Err(install_error(format!(
            "required command not found: {command}"
        )))
    }
}

fn command_available(command: &str) -> bool {
    let path = std::path::Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .any(|directory| {
            if directory.join(command).is_file() {
                return true;
            }
            cfg!(windows)
                && ["exe", "cmd", "bat"]
                    .iter()
                    .any(|extension| directory.join(format!("{command}.{extension}")).is_file())
        })
}

fn installer_name(command: &str, mode: &str) -> String {
    match command {
        "npx" => "npm_npx",
        "uvx" => "pypi_uvx",
        "pipx" => "pypi_pipx",
        "docker" => "oci_docker",
        "podman" => "oci_podman",
        _ if mode == "command" => "command_stdio",
        _ => "package_stdio",
    }
    .into()
}

fn package_has_version(identifier: &str) -> bool {
    identifier
        .strip_prefix('@')
        .map_or_else(|| identifier.contains('@'), |rest| rest.contains('@'))
}

fn timeout() -> Duration {
    Duration::from_secs(
        std::env::var("CCCC_CAPABILITY_INSTALL_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(45)
            .clamp(5, 180),
    )
}

fn object<'a>(root: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    if !root.is_object() {
        *root = json!({});
    }
    let value = root
        .as_object_mut()
        .expect("root object")
        .entry(key)
        .or_insert_with(|| json!({}));
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("nested object")
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned()
}
fn text_map(value: &Map<String, Value>, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned()
}
fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
fn install_error(error: impl std::fmt::Display) -> OpError {
    OpError::new("capability_install_failed", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ensure_installed, normalize_tools, resolve_command};
    use cccc_core::HomeLayout;
    use serde_json::json;

    #[test]
    fn resolves_supported_package_commands() {
        let npm = json!({"registry_type":"npm","identifier":"@example/mcp","version":"1.2.3"});
        assert_eq!(
            resolve_command("package", npm.as_object().expect("npm object")).expect("npm command"),
            ["npx", "-y", "@example/mcp@1.2.3"]
        );
        let pypi = json!({"registry_type":"pypi","identifier":"example-mcp","version":"1.2.3"});
        assert_eq!(
            resolve_command("package", pypi.as_object().expect("pypi object"))
                .expect("pypi command"),
            ["uvx", "example-mcp@1.2.3"]
        );
        let oci = json!({"registry_type":"oci","identifier":"ghcr.io/example/mcp:1"});
        assert_eq!(
            resolve_command("package", oci.as_object().expect("oci object")).expect("oci command"),
            ["docker", "run", "-i", "--rm", "ghcr.io/example/mcp:1"]
        );
    }

    #[test]
    fn normalizes_external_tool_names() {
        let tools = normalize_tools(
            "mcp:test",
            Some(&json!([{"name":"create-issue","description":"Create"}])),
        );
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["real_tool_name"], "create-issue");
        assert_eq!(tools[0]["name"], "cccc_ext_b309fafe_create_issue");
    }

    #[test]
    fn missing_required_environment_is_shared_with_installation() {
        let record = json!({
            "install_mode":"command",
            "required_env":["CCCC_FIXTURE_MISSING_ENV_B_4610", "CCCC_FIXTURE_MISSING_ENV_A_4610"],
            "install_spec":{
                "command_candidates":[[std::env::current_exe().expect("test executable")]],
                "required_env":[" CCCC_FIXTURE_MISSING_ENV_A_4610 "]
            }
        });
        assert_eq!(
            super::missing_environment(&record),
            [
                "CCCC_FIXTURE_MISSING_ENV_A_4610",
                "CCCC_FIXTURE_MISSING_ENV_B_4610"
            ]
        );
        let error = super::prepare("mcp:fixture:environment", &record)
            .expect_err("missing environment must prevent starting the process");
        assert_eq!(error.code, "capability_install_failed");
        assert_eq!(
            error.message,
            "missing required environment: CCCC_FIXTURE_MISSING_ENV_A_4610, CCCC_FIXTURE_MISSING_ENV_B_4610"
        );
        assert!(
            super::missing_environment(&json!({
                "required_env":["PATH"], "install_spec":{"required_env":[" PATH "]}
            }))
            .is_empty()
        );
    }

    #[test]
    fn command_install_probes_and_persists_python_compatible_artifact() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize home");
        let probe = temp.path().join("probe-mcp.py");
        std::fs::write(&probe, super::super::mcp_stdio::STDIO_FIXTURE).expect("write probe");
        let evidence = temp.path().join("evidence.jsonl");
        let python = ["python3", "python"]
            .into_iter()
            .find(|name| {
                std::process::Command::new(name)
                    .arg("--version")
                    .output()
                    .is_ok_and(|output| output.status.success())
            })
            .expect("Python is required for the protocol fixture");

        let record = json!({
            "install_mode":"command",
            "install_spec":{"command_candidates":[[python,"-u",probe,"normal",evidence]]},
        });
        let artifact = ensure_installed(&home, "mcp:test", &record)
            .expect("install")
            .expect("artifact");
        assert_eq!(artifact["state"], "installed");
        assert_eq!(artifact["tools"][0]["real_tool_name"], "echo");

        let runtime: serde_json::Value =
            cccc_core::fs::read_json(&home.root().join("state/capabilities/runtime.json"))
                .expect("runtime");
        let artifact_id = runtime["capability_artifacts"]["mcp:test"]
            .as_str()
            .expect("artifact id");
        assert_eq!(runtime["artifacts"][artifact_id]["state"], "installed");
    }
}
