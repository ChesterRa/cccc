use crate::dispatch::OpError;
use serde_json::{Map, Value, json};
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
        "command" => json!({"commands":commands(spec)?}),
        "package" => {
            let registry = text(spec, "registry_type").trim().to_ascii_lowercase();
            let identifier = text(spec, "identifier").trim();
            let version = text(spec, "version").trim();
            let runtime = args(&spec["runtime_arguments"]);
            let package = args(&spec["package_arguments"]);
            if identifier.is_empty() {
                json!({"commands":commands(spec)?})
            } else {
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
                    _ => json!({"commands":commands(spec)?}),
                }
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

fn commands(spec: &Value) -> Result<Vec<Vec<String>>, OpError> {
    command_candidates(
        spec.as_object()
            .ok_or_else(|| invalid("install_spec must be an object"))?,
    )
}

/// The same declared argv order drives validation, execution and identity.
/// A string names one executable; only an array supplies its arguments.
pub(super) fn command_candidates(spec: &Map<String, Value>) -> Result<Vec<Vec<String>>, OpError> {
    let mut commands = Vec::new();
    for (key, multiple) in [
        ("command_candidates", true),
        ("command", false),
        ("fallback_command_candidates", true),
        ("fallback_command", false),
    ] {
        let Some(value) = spec.get(key).filter(|value| !value.is_null()) else {
            continue;
        };
        if multiple {
            let candidates = value.as_array().ok_or_else(|| {
                invalid(format!(
                    "{key} must be an array of executable strings or argv arrays"
                ))
            })?;
            for candidate in candidates {
                commands.push(command_argv(candidate, key)?);
            }
        } else {
            commands.push(command_argv(value, key)?);
        }
    }
    Ok(commands)
}

fn command_argv(value: &Value, key: &str) -> Result<Vec<String>, OpError> {
    let argv = match value {
        Value::String(executable) => vec![executable.clone()],
        Value::Array(argv) => argv
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(format!("{key} argv must contain only strings")))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(invalid(format!(
                "{key} must be an executable string or argv array"
            )));
        }
    };
    if argv
        .first()
        .is_none_or(|executable| executable.trim().is_empty())
    {
        return Err(invalid(format!("{key} must name a non-empty executable")));
    }
    Ok(argv)
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

fn invalid(message: impl Into<String>) -> OpError {
    OpError::new("capability_recipe_invalid", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_commands_preserve_literal_argv_and_existing_candidate_priority() {
        let spec = json!({
            "command_candidates":[["primary", "", "$HOME; echo untouched"], "second"],
            "command":"a program with spaces",
            "fallback_command_candidates":[["fallback", "--two"]],
            "fallback_command":["last", "argument with spaces"]
        });
        assert_eq!(
            command_candidates(spec.as_object().expect("spec")).expect("argv"),
            vec![
                vec!["primary", "", "$HOME; echo untouched"],
                vec!["second"],
                vec!["a program with spaces"],
                vec!["fallback", "--two"],
                vec!["last", "argument with spaces"],
            ]
        );
        assert!(
            command_candidates(
                json!({"command":null,"command_candidates":null})
                    .as_object()
                    .expect("spec")
            )
            .expect("omitted optional fields")
            .is_empty()
        );
        for key in ["command", "fallback_command"] {
            for bad in [json!(1), json!([]), json!(["", "arg"]), json!(["ok", 1])] {
                let mut spec = Map::new();
                spec.insert(key.into(), bad);
                assert!(command_candidates(&spec).is_err(), "{key}");
            }
        }
        for bad in [
            json!("program"),
            json!([[]]),
            json!(["ok", null]),
            json!([["ok", 1]]),
        ] {
            assert!(
                command_candidates(json!({"command_candidates":bad}).as_object().expect("spec"))
                    .is_err()
            );
        }
    }

    #[test]
    fn command_identity_matches_legacy_candidates_and_tracks_single_command_changes() {
        let legacy = json!({"install_mode":"command", "required_env":["Z","A"],
            "install_spec":{"command_candidates":[["primary","--one"]],
                "fallback_command_candidates":[["fallback","--two"]]}});
        assert_eq!(
            fingerprint(&legacy).expect("legacy recipe"),
            "4a9d2e2d3db143f81da8c1f297e91934d581b12cc7bd0899505b3ed632f4dab6"
        );
        let single =
            json!({"install_mode":"command", "install_spec":{"command":["primary","--one"]}});
        let candidates = json!({"install_mode":"command", "install_spec":{"command_candidates":[["primary","--one"]]}});
        assert_eq!(
            fingerprint(&single).expect("single"),
            fingerprint(&candidates).expect("candidates")
        );
        let mut changed = single.clone();
        changed["install_spec"]["command"][1] = json!("--changed");
        assert_ne!(
            fingerprint(&single).expect("single"),
            fingerprint(&changed).expect("changed")
        );
        let fallback = json!({"install_mode":"package", "install_spec":{"registry_type":"npm", "fallback_command":["primary","--one"]}});
        changed = fallback.clone();
        changed["install_spec"]["fallback_command"][1] = json!("--changed");
        assert_ne!(
            fingerprint(&fallback).expect("fallback"),
            fingerprint(&changed).expect("changed fallback")
        );
    }

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
