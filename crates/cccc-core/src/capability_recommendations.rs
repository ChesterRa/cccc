//! Versioned recommendations are release data, independent of local catalog snapshots.
use crate::capabilities::{Capability, capability_from_record};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::io;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/capability_skill_files.rs"));
const MANIFEST: &str = include_str!("../../../resources/capability_recommendations.json");

pub const NOTE_FIELDS: [&str; 3] = ["avoid_when", "gotchas", "requirements_summary"];
pub const MAX_BROWSE_ENTRIES: usize = 10;

/// Select recommendations this member might use. Ties keep the input's manifest order.
/// Presentation budgets and skills already shown elsewhere belong to the caller.
pub fn for_member(entries: &[Value]) -> Vec<&Value> {
    let mut entries = entries
        .iter()
        .filter(|entry| {
            entry["for_you"]["blocked"] != true
                && matches!(
                    entry["for_you"]["availability"].as_str(),
                    Some("compatible" | "unknown" | "needs_setup")
                )
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| {
        let pending = entry["for_you"]["activation_pending"] == true;
        let primary = entry["recommendation"]
            .as_array()
            .is_some_and(|roles| roles.iter().any(|role| role["role"] == "primary"));
        (!pending, !primary)
    });
    entries
}

/// CCCC's notes accompany release content without modifying its body or tool result.
pub fn notes(capability: &Capability) -> Map<String, Value> {
    if !capability.is_release_recommendation() {
        return Map::new();
    }
    NOTE_FIELDS
        .into_iter()
        .map(|key| {
            let value = capability.metadata.get(key).cloned().unwrap_or_else(|| {
                if key == "requirements_summary" {
                    json!("")
                } else {
                    json!([])
                }
            });
            (key.into(), value)
        })
        .collect()
}

#[derive(Clone, Deserialize)]
struct Manifest {
    schema_version: u64,
    domains: Vec<Value>,
    entries: Vec<Value>,
}

fn manifest() -> io::Result<&'static Manifest> {
    static DATA: OnceLock<Result<Manifest, String>> = OnceLock::new();
    DATA.get_or_init(|| parse(MANIFEST, SKILL_FILES).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.clone()))
}

pub fn catalog() -> io::Result<Vec<Capability>> {
    manifest()?
        .entries
        .iter()
        .map(|record| {
            let mut capability = capability_from_record(record)?;
            capability.release_owned = true;
            Ok(capability)
        })
        .collect()
}

pub fn record(id: &str) -> io::Result<Option<Value>> {
    Ok(manifest()?
        .entries
        .iter()
        .find(|record| record["capability_id"].as_str() == Some(id))
        .cloned())
}

pub fn records() -> io::Result<Vec<Value>> {
    Ok(manifest()?.entries.clone())
}

/// Public credential metadata names one environment key; it never contains a value.
pub fn bearer_env(record: &Value) -> Option<&str> {
    let credential = record.pointer("/support/credential")?.as_object()?;
    let key = credential.get("env")?.as_str()?;
    (credential.len() == 2
        && credential.get("kind")?.as_str()? == "bearer"
        && !key.is_empty()
        && key.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        }))
    .then_some(key)
}

pub fn domains(locale: &str) -> io::Result<Vec<Value>> {
    Ok(manifest()?
        .domains
        .iter()
        .map(|domain| {
            let language = language(locale);
            json!({
                "domain_id":domain["domain_id"],
                "name":domain.get("l10n").and_then(|all| all.get(language))
                    .and_then(|text| text.get("name")).filter(|text| text.as_str().is_some_and(|text| !text.is_empty()))
                    .unwrap_or(&domain["name"])
            })
        })
        .collect())
}

/// Translate presentation fields only. Execution recipes and verification remain unchanged.
pub fn localized(capability: &Capability, locale: &str) -> Capability {
    let mut result = capability.clone();
    let Some(translation) = capability
        .metadata
        .get("l10n")
        .and_then(|all| all.get(language(locale)))
        .and_then(Value::as_object)
    else {
        return result;
    };
    if let Some(name) = translation
        .get("name")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        result.name = name.into();
    }
    if let Some(text) = translation
        .get("description_short")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        result.description = text.into();
    }
    for key in ["use_when", "avoid_when", "gotchas", "requirements_summary"] {
        if let Some(text) = translation.get(key) {
            result.metadata.insert(key.into(), text.clone());
        }
    }
    if let Some(reasons) = translation.get("recommendation").and_then(Value::as_array)
        && let Some(items) = result
            .metadata
            .get_mut("recommendation")
            .and_then(Value::as_array_mut)
    {
        for item in items {
            if let Some(reason) = reasons
                .iter()
                .find(|reason| reason["domain_id"] == item["domain_id"])
                .and_then(|reason| reason.get("reason"))
                .filter(|reason| reason.is_string())
            {
                item["reason"] = reason.clone();
            }
        }
    }
    result
}

fn language(locale: &str) -> &str {
    match locale.split(['-', '_']).next().unwrap_or("en") {
        "zh" => "zh",
        "ja" => "ja",
        _ => "en",
    }
}

fn parse(text: &str, files: &[(&str, &str)]) -> io::Result<Manifest> {
    let mut manifest: Manifest = serde_json::from_str(text).map_err(invalid)?;
    require(
        manifest.schema_version == 1,
        "unsupported recommendation schema version",
    )?;
    let mut domains = BTreeSet::new();
    for domain in &manifest.domains {
        require(
            domains.insert(required_text(domain, "domain_id")?.to_owned()),
            "duplicate domain id",
        )?;
        required_text(domain, "name")?;
    }
    let mut ids = BTreeSet::new();
    let mut declared_files = BTreeSet::new();
    for record in &mut manifest.entries {
        let id = required_text(record, "capability_id")?.to_owned();
        require(ids.insert(id.clone()), "duplicate recommendation id")?;
        let kind = required_text(record, "kind")?.to_owned();
        let prefix = match kind.as_str() {
            "skill" => "skill:recommended:",
            "mcp_toolpack" => "mcp:recommended:",
            _ => return Err(invalid("unsupported recommendation kind")),
        };
        let slug = id
            .strip_prefix(prefix)
            .filter(|slug| !slug.is_empty())
            .ok_or_else(|| invalid("recommendation id must use its release namespace"))?;
        require(
            slug.bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_')),
            "invalid recommendation slug",
        )?;
        for key in [
            "name",
            "description_short",
            "requirements_summary",
            "source_uri",
            "license",
        ] {
            required_text(record, key)?;
        }
        let source = url::Url::parse(required_text(record, "source_uri")?).map_err(invalid)?;
        require(
            matches!(source.scheme(), "https" | "http")
                && source.username().is_empty()
                && source.password().is_none(),
            "recommendation source must be a public HTTP URL",
        )?;
        for key in ["use_when", "avoid_when", "gotchas"] {
            string_list(record.get(key), key)?;
        }
        validate_avoid_when(&record["avoid_when"])?;
        for translation in record
            .get("l10n")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|languages| languages.values())
        {
            if let Some(avoid_when) = translation.get("avoid_when") {
                string_list(Some(avoid_when), "avoid_when")?;
                validate_avoid_when(avoid_when)?;
            }
        }
        let recommendations = record["recommendation"]
            .as_array()
            .filter(|items| !items.is_empty())
            .ok_or_else(|| invalid("recommendation must name at least one domain"))?;
        let mut recommended_domains = BTreeSet::new();
        for recommendation in recommendations {
            let domain = required_text(recommendation, "domain_id")?;
            require(
                domains.contains(domain) && recommended_domains.insert(domain),
                "unknown or repeated recommendation domain",
            )?;
            require(
                matches!(
                    required_text(recommendation, "role")?,
                    "primary" | "alternative"
                ),
                "invalid recommendation role",
            )?;
            required_text(recommendation, "reason")?;
        }
        let support = record
            .get("support")
            .filter(|support| support.is_object())
            .ok_or_else(|| invalid("recommendation support is required"))?;
        let auth = required_text(support, "auth")?.to_owned();
        require(
            matches!(
                auth.as_str(),
                "none" | "bearer" | "api_key" | "oauth" | "account"
            ),
            "unsupported recommendation authentication",
        )?;
        for key in ["prerequisites", "limitations"] {
            string_list(support.get(key), key)?;
        }
        let delivery_supported = match (
            kind.as_str(),
            required_text(support, "form")?,
            required_text(support, "runs_on")?,
        ) {
            ("skill", "text_skill", "actor_tools") => {
                let path = format!("{slug}/SKILL.md");
                require(
                    record["capsule_path"].as_str() == Some(&path),
                    "skill capsule_path must name its embedded SKILL.md",
                )?;
                let license = format!("{slug}/LICENSE");
                let body = asset(files, &path)?;
                asset(files, &license)?;
                require(
                    !body.trim().is_empty() && body.len() <= 2 * 1024 * 1024,
                    "skill body is empty or exceeds 2 MiB",
                )?;
                declared_files.extend([path, license]);
                record["capsule_text"] = json!(body);
                true
            }
            ("mcp_toolpack", "remote_http", "daemon_host") => {
                require(
                    record["install_mode"] == "remote_only",
                    "HTTP recommendation must use remote_only",
                )?;
                let url = url::Url::parse(required_text(&record["install_spec"], "url")?)
                    .map_err(invalid)?;
                require(
                    matches!(url.scheme(), "http" | "https")
                        && url.username().is_empty()
                        && url.password().is_none(),
                    "remote recommendation URL must not contain credentials",
                )?;
                require(
                    record["install_spec"].get("token").is_none()
                        && record["install_spec"].get("bearer_token").is_none(),
                    "release recipes must not contain credentials",
                )?;
                if support.get("credential").is_some() {
                    require(
                        auth == "bearer" && bearer_env(record).is_some(),
                        "credential must be a Bearer environment-key reference",
                    )?;
                }
                auth == "none" || (auth == "bearer" && bearer_env(record).is_some())
            }
            ("mcp_toolpack", "stdio", "daemon_host") => {
                validate_stdio(record)?;
                auth == "none"
            }
            ("skill", "skill_bundle" | "reference", "actor_tools")
            | ("mcp_toolpack", "stateful_stdio" | "reference", "daemon_host") => false,
            _ => return Err(invalid("unsupported recommendation delivery shape")),
        };
        let observations = record["verification"]
            .as_array()
            .ok_or_else(|| invalid("verification must be an array"))?;
        for observation in observations {
            require(
                matches!(
                    required_text(observation, "level")?,
                    "listed" | "install_checked" | "tool_call_checked" | "task_checked"
                ),
                "unknown verification level",
            )?;
            require(
                matches!(required_text(observation, "outcome")?, "passed" | "failed"),
                "verification outcome is required",
            )?;
            for key in ["build", "path", "result"] {
                required_text(observation, key)?;
            }
            chrono::DateTime::parse_from_rfc3339(required_text(observation, "observed_at")?)
                .map_err(invalid)?;
        }
        record["source_id"] = json!("cccc_recommended");
        record["qualification_status"] = json!("qualified");
        record["enable_supported"] = json!(delivery_supported);
    }
    let embedded_files = files
        .iter()
        .map(|(path, _)| (*path).to_owned())
        .collect::<BTreeSet<_>>();
    require(
        declared_files == embedded_files,
        "embedded skill files and recommendation entries must match in both directions",
    )?;
    Ok(manifest)
}

fn validate_stdio(record: &Value) -> io::Result<()> {
    let spec = &record["install_spec"];
    match required_text(record, "install_mode")? {
        "command" => {
            let commands = spec["command_candidates"]
                .as_array()
                .filter(|commands| !commands.is_empty())
                .ok_or_else(|| invalid("command recommendation requires command_candidates"))?;
            for command in commands {
                string_list(Some(command), "command candidate")?;
                require(
                    command.as_array().is_some_and(|argv| {
                        !argv.is_empty()
                            && argv
                                .iter()
                                .all(|arg| arg.as_str().is_some_and(|arg| !arg.is_empty()))
                    }),
                    "command candidate must contain nonempty arguments",
                )?;
            }
            Ok(())
        }
        "package" => {
            for key in ["registry_type", "identifier", "version"] {
                required_text(spec, key)?;
            }
            Ok(())
        }
        _ => Err(invalid("unsupported stdio install mode")),
    }
}

fn validate_avoid_when(value: &Value) -> io::Result<()> {
    require(
        value.as_array().is_some_and(|items| items.len() <= 2),
        "recommendation avoid_when must contain at most two entries",
    )
}

fn asset<'a>(files: &[(&str, &'a str)], path: &str) -> io::Result<&'a str> {
    files
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, body)| *body)
        .filter(|body| !body.trim().is_empty())
        .ok_or_else(|| invalid(format!("missing embedded capability asset: {path}")))
}

fn required_text<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| invalid(format!("recommendation {key} must be a nonempty string")))
}

fn string_list(value: Option<&Value>, key: &str) -> io::Result<()> {
    require(
        value
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().all(Value::is_string)),
        &format!("recommendation {key} must be a string array"),
    )
}

fn require(valid: bool, message: &str) -> io::Result<()> {
    if valid { Ok(()) } else { Err(invalid(message)) }
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_recommendations_keep_setup_and_unknown_and_prioritize_stably() {
        let row = |id: &str, availability: Value, blocked: bool, pending: bool, primary: bool| {
            json!({"id": id,
                "for_you": {"availability": availability, "blocked": blocked,
                    "activation_pending": pending, "enabled": true},
                "recommendation": [{"role": if primary {"primary"} else {"alternative"}}]})
        };
        let entries = vec![
            row("alt", json!("compatible"), false, false, false),
            row("setup", json!("needs_setup"), false, false, false),
            row("unknown", json!("unknown"), false, false, false),
            row("blocked", json!("compatible"), true, true, true),
            row("unsupported", json!("unsupported"), false, true, true),
            row("unreported", Value::Null, false, false, false),
            row("primary", json!("compatible"), false, false, true),
            row("pending-alt", json!("compatible"), false, true, false),
            row("pending-primary", json!("compatible"), false, true, true),
        ];

        let selected = for_member(&entries);

        assert_eq!(
            selected
                .iter()
                .map(|entry| entry["id"].as_str().expect("id"))
                .collect::<Vec<_>>(),
            [
                "pending-primary",
                "pending-alt",
                "primary",
                "alt",
                "setup",
                "unknown"
            ]
        );
        assert!(for_member(&[]).is_empty());
    }

    #[test]
    fn release_manifest_has_complete_bodies_licenses_and_no_unregistered_assets() {
        let manifest = parse(MANIFEST, SKILL_FILES).expect("valid release manifest");
        assert!(!manifest.domains.is_empty());
        assert!(!manifest.entries.is_empty());
        assert!(
            manifest.entries.len() <= MAX_BROWSE_ENTRIES,
            "expanding the shipped list also requires reviewing the browse budget and its description"
        );
        for entry in &manifest.entries {
            if entry["kind"] == "skill" {
                let path = entry["capsule_path"].as_str().expect("path");
                assert_eq!(
                    entry["capsule_text"],
                    asset(SKILL_FILES, path).expect("body")
                );
            }
        }
        let mut extra = SKILL_FILES.to_vec();
        extra.push(("unregistered/LICENSE", "fixture license"));
        assert!(parse(MANIFEST, &extra).is_err());
        assert!(parse(MANIFEST, &SKILL_FILES[1..]).is_err());
    }

    #[test]
    fn malformed_domains_outcomes_and_command_recipes_are_rejected() {
        let original: Value = serde_json::from_str(MANIFEST).expect("JSON");
        let mut unknown_domain = original.clone();
        unknown_domain["entries"][0]["recommendation"][0]["domain_id"] = json!("missing");
        assert!(parse(&unknown_domain.to_string(), SKILL_FILES).is_err());
        let mut duplicate = original.clone();
        duplicate["entries"]
            .as_array_mut()
            .expect("entries")
            .push(original["entries"][0].clone());
        assert!(parse(&duplicate.to_string(), SKILL_FILES).is_err());
        let mut wrong_outcome = original.clone();
        wrong_outcome["entries"][0]["verification"] =
            json!([{"level":"install_checked","outcome":"unknown"}]);
        assert!(parse(&wrong_outcome.to_string(), SKILL_FILES).is_err());
        let mut too_many_cases = original.clone();
        too_many_cases["entries"][0]["avoid_when"] = json!(["one", "two", "three"]);
        assert!(parse(&too_many_cases.to_string(), SKILL_FILES).is_err());
        let mut too_many_translated_cases = original;
        too_many_translated_cases["entries"][0]["l10n"]["zh"]["avoid_when"] =
            json!(["一", "二", "三"]);
        assert!(parse(&too_many_translated_cases.to_string(), SKILL_FILES).is_err());
        let bad = json!({"install_mode":"command","install_spec":{"command":["fixture"]}});
        assert!(validate_stdio(&bad).is_err());
    }

    #[test]
    fn fresh_catalog_uses_release_content_without_overwriting_local_records() {
        use crate::{HomeLayout, capabilities::CapabilityStore};
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("layout");
        home.initialize().expect("initialize");
        let store = CapabilityStore::new(home.clone());
        let published = catalog().expect("release catalog");
        let skill = published
            .iter()
            .find(|entry| entry.kind == "skill")
            .expect("skill");
        assert!(
            store
                .require(&skill.id)
                .expect("fresh skill")
                .is_release_recommendation()
        );
        assert!(
            store
                .catalog_record(&skill.id)
                .expect("private catalog")
                .is_none()
        );
        assert!(
            store
                .import_record(
                    json!({"capability_id":skill.id,"kind":"skill","name":"replacement"})
                )
                .is_err()
        );
        let mcp = published
            .iter()
            .find(|entry| entry.kind == "mcp_toolpack")
            .expect("MCP");
        assert!(
            CapabilityStore::builtin(&mcp.id).is_none(),
            "external recommendations must use the installer"
        );
        assert!(
            store
                .resolved_record(&mcp.id)
                .expect("execution record")
                .is_some()
        );

        let raw_path = home.root().join("state/capabilities/catalog.json");
        let user_record = json!({"capability_id":skill.id,"kind":"skill","name":"my custom skill","capsule_text":"my exact rules\n"});
        crate::fs::write_json(
            &raw_path,
            &json!({"records":{skill.id.clone():user_record}}),
        )
        .expect("existing collision");
        let before = std::fs::read(&raw_path).expect("saved bytes");
        let resolved = store.require(&skill.id).expect("local record wins");
        assert_eq!(resolved.capsule_text, "my exact rules\n");
        assert!(!resolved.is_release_recommendation());
        assert!(
            notes(&resolved).is_empty(),
            "an ID collision is not release content"
        );
        assert_eq!(std::fs::read(&raw_path).expect("unchanged bytes"), before);
        assert!(store.import_record(json!({"capability_id":skill.id,"kind":"skill","name":"my custom skill","capsule_text":"my next rules"})).is_ok());
        assert_eq!(store.search(&mcp.id).expect("exact search")[0].id, mcp.id);
    }

    #[test]
    fn localization_changes_presentation_but_never_execution_or_verification() {
        let mut original = capability_from_record(&json!({
            "capability_id":"mcp:test","name":"English","description_short":"Default",
            "recommendation":[{"domain_id":"research","role":"primary","reason":"English reason"}],
            "support":{"form":"remote_http"},"verification":[{"outcome":"failed"}],
            "l10n":{"zh":{"name":"中文名称","description_short":"中文说明",
                "recommendation":[{"domain_id":"research","reason":"中文理由","role":"alternative"}],
                "support":{"form":"stdio"},"verification":[]}}
        })).expect("record");
        original.release_owned = true;
        let translated = localized(&original, "zh-CN");
        assert_eq!(translated.name, "中文名称");
        assert_eq!(
            translated.metadata["recommendation"][0]["reason"],
            "中文理由"
        );
        assert_eq!(translated.metadata["recommendation"][0]["role"], "primary");
        assert_eq!(translated.metadata["support"], original.metadata["support"]);
        assert_eq!(
            translated.metadata["verification"],
            original.metadata["verification"]
        );
        assert_eq!(localized(&original, "ja").name, "English");
    }

    #[test]
    fn release_notes_are_english_metadata_without_body_recipe_or_credentials() {
        for capability in catalog().expect("release catalog") {
            let notes = notes(&capability);
            assert_eq!(notes.len(), NOTE_FIELDS.len());
            for key in NOTE_FIELDS {
                assert_eq!(notes[key], capability.metadata[key]);
            }
            assert!(!notes.contains_key("capsule_text"));
            assert!(!notes.contains_key("install_spec"));
            assert!(!notes.contains_key("support"));
        }
    }
}
