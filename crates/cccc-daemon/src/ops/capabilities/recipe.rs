use crate::dispatch::OpError;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Identity of declared execution inputs, independent of recommendations and
/// translations. Keep environment references unresolved; credentials are not cache data.
pub(super) fn fingerprint(record: &Value) -> Result<String, OpError> {
    let mode = text(record, "install_mode").trim();
    let spec = &record["install_spec"];
    let execution = match mode {
        "remote_only" => {
            let url = url::Url::parse(text(spec, "url").trim())
                .map_err(|_| invalid("invalid HTTP MCP endpoint"))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(invalid("HTTP MCP endpoint must use http or https"));
            }
            json!({"url":url.as_str(),"auth":spec.get("auth"),
                "credential":record.pointer("/support/credential")})
        }
        "command" => json!({"commands":commands(spec)}),
        "package" => {
            let registry = text(spec, "registry_type").trim().to_ascii_lowercase();
            let identifier = text(spec, "identifier").trim();
            let version = text(spec, "version").trim();
            let runtime = args(&spec["runtime_arguments"]);
            let package = args(&spec["package_arguments"]);
            match registry.as_str() {
                "npm" | "javascript" | "node" | "nodejs" => {
                    let has_version = identifier
                        .strip_prefix('@')
                        .map_or_else(|| identifier.contains('@'), |name| name.contains('@'));
                    let package_name = if version.is_empty() || has_version {
                        identifier.into()
                    } else {
                        format!("{identifier}@{version}")
                    };
                    json!({"runtime":"npx","package":package_name,"arguments":package})
                }
                "pypi" | "python" | "pip" | "pipx" | "uvx" => {
                    let package_name = if version.is_empty() {
                        identifier.into()
                    } else {
                        format!("{identifier}@{version}")
                    };
                    if text(spec, "runtime_hint")
                        .trim()
                        .eq_ignore_ascii_case("pipx")
                    {
                        json!({"runtime":"pipx","package":package_name,"entry":identifier,"arguments":package})
                    } else {
                        json!({"runtime":"uvx","invocation":if runtime.is_empty(){json!([package_name])}else{json!(runtime)},"arguments":package})
                    }
                }
                "oci" | "docker" | "container" | "podman" => json!({
                    "runtime":if text(spec,"runtime_hint").trim().eq_ignore_ascii_case("podman"){"podman"}else{"docker"},
                    "image":identifier,"runtime_arguments":runtime,"arguments":package,
                    "container_env":required_env(spec)
                }),
                _ => json!({"commands":commands(spec)}),
            }
        }
        _ => return Err(invalid("capability has no supported execution recipe")),
    };
    let mut environment = required_env(record);
    environment.extend(required_env(spec));
    environment.sort();
    environment.dedup();
    let mut canonical = json!({"mode":mode,"execution":execution,"required_env":environment});
    canonical.sort_all_objects();
    let bytes = serde_json::to_vec(&canonical).map_err(|_| invalid("invalid execution recipe"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn text<'a>(record: &'a Value, key: &str) -> &'a str {
    record.get(key).and_then(Value::as_str).unwrap_or("")
}

fn args(values: &Value) -> Vec<Value> {
    values
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|value| {
            if let Some(value) = value.as_str() {
                return vec![json!(value)];
            }
            let mut result = Vec::new();
            if !text(value, "name").trim().is_empty() {
                result.push(json!(text(value, "name").trim()));
            }
            if !text(value, "value").trim().is_empty() {
                result.push(json!({"environment_template":text(value,"value").trim()}));
            }
            result
        })
        .collect()
}

fn commands(spec: &Value) -> Vec<Vec<String>> {
    ["command_candidates", "fallback_command_candidates"]
        .into_iter()
        .flat_map(|key| {
            spec[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_array)
                .map(|command| {
                    command
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
        })
        .filter(|command| !command.is_empty())
        .collect()
}

fn required_env(record: &Value) -> Vec<String> {
    let mut names = record["required_env"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn invalid(message: &str) -> OpError {
    OpError::new("capability_recipe_invalid", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_and_unused_package_fields_do_not_change_execution_identity() {
        let base = json!({"install_mode":"package","install_spec":{"registry_type":"npm","identifier":"fixture@1","version":"ignored"}});
        let mut edited = base.clone();
        edited["description_short"] = json!("new description");
        edited["l10n"] = json!({"ja":{"name":"翻訳"}});
        edited["install_spec"]["version"] = json!("also ignored");
        edited["install_spec"]["runtime_arguments"] = json!(["ignored by npm"]);
        assert_eq!(
            fingerprint(&base).expect("valid base recipe"),
            fingerprint(&edited).expect("valid edited recipe")
        );
        edited["install_spec"]["package_arguments"] = json!(["--readonly"]);
        assert_ne!(
            fingerprint(&base).expect("valid base recipe"),
            fingerprint(&edited).expect("valid edited recipe")
        );
        let docker = json!({"install_mode":"package","install_spec":{"registry_type":"oci","identifier":"fixture:1","runtime_hint":"docker"}});
        let mut podman = docker.clone();
        podman["install_spec"]["runtime_hint"] = json!(" podman ");
        assert_ne!(
            fingerprint(&docker).expect("valid docker recipe"),
            fingerprint(&podman).expect("valid podman recipe")
        );
    }

    #[test]
    fn recipe_identity_tracks_commands_urls_and_environment_references() {
        let base =
            json!({"install_mode":"remote_only","install_spec":{"url":"https://example.test/mcp"}});
        let mut edited = base.clone();
        edited["install_spec"]["url"] = json!("https://example.test/new");
        assert_ne!(
            fingerprint(&base).expect("valid base recipe"),
            fingerprint(&edited).expect("valid edited recipe")
        );
        let base = json!({"install_mode":"command","install_spec":{"command_candidates":[["fixture","one"]]},"required_env":["B","A"]});
        let mut edited = base.clone();
        edited["required_env"] = json!(["A", "B", "A"]);
        assert_eq!(
            fingerprint(&base).expect("valid base recipe"),
            fingerprint(&edited).expect("valid edited recipe")
        );
        edited["required_env"] = json!(["C"]);
        assert_ne!(
            fingerprint(&base).expect("valid base recipe"),
            fingerprint(&edited).expect("valid edited recipe")
        );
        let base = json!({"install_mode":"package","install_spec":{"registry_type":"pypi","identifier":"ignored","version":"1","runtime_arguments":["custom-runtime-entry"]}});
        let mut edited = base.clone();
        edited["install_spec"]["identifier"] = json!("also-ignored");
        assert_eq!(
            fingerprint(&base).expect("valid base recipe"),
            fingerprint(&edited).expect("valid edited recipe")
        );
    }
}
