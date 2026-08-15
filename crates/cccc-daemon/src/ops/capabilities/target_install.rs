use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::HomeLayout;
use cccc_core::capabilities::CapabilityStore;
use reqwest::blocking::Client;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, string_arg};

use super::package_install;

const MAX_GITHUB_SKILLS: usize = 64;

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let access = super::authorize_scope_mutation(home, request, "actor")?;
    let group_id = access.actor.group_id;
    let actor_id = access.actor.actor_id;
    let by = access.actor.by;
    let scope = access.scope;
    let target = string_arg(request, "target")
        .or_else(|| string_arg(request, "source_uri"))
        .or_else(|| string_arg(request, "capability_id"))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("missing_install_target", "missing install target"))?;
    let ttl_seconds = request
        .args
        .get("ttl_seconds")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    let action_id = format!("cins_{}", &Uuid::new_v4().simple().to_string()[..16]);
    let kind = classify(&target);
    let records = match kind {
        TargetKind::CapabilityId => {
            let store = CapabilityStore::new(home.clone());
            let already_enabled = store
                .is_enabled_for(&target, &group_id, &actor_id, &scope)
                .map_err(OpError::io)?;
            let was_hidden = !actor_id.is_empty()
                && store
                    .is_hidden_for(&target, &group_id, &actor_id)
                    .map_err(OpError::io)?;
            let enabled = enable(home, &group_id, &actor_id, &scope, ttl_seconds, &target)?;
            let use_ready = enabled
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let enabled_ids = if use_ready {
                vec![target.clone()]
            } else {
                Vec::new()
            };
            let changed_ids = if use_ready && (!already_enabled || was_hidden) {
                enabled_ids.clone()
            } else {
                Vec::new()
            };
            let result = object(json!({
                "action_id":action_id,"group_id":group_id,"actor_id":actor_id,
                "target":target,"target_kind":"capability_id","scope":scope,
                "installed_capability_ids":[target],"enabled_capability_ids":enabled_ids,
                "use_ready_capability_ids":enabled_ids,
                "requires_setup":!use_ready,
                "refresh_required":!changed_ids.is_empty(),
                "state":if use_ready {"ready"} else {"needs_setup"},
                "enable_result":enabled
            }))?;
            return super::install_events::finish(
                home,
                result,
                &super::install_events::InstallChange {
                    action_id: &action_id,
                    group_id: &group_id,
                    actor_id: &actor_id,
                    by: &by,
                    scope: &scope,
                    capability_ids: &changed_ids,
                },
            );
        }
        TargetKind::Local => local_records(&target)?,
        TargetKind::Url => vec![url_record(&target)?],
        TargetKind::Github => github_records(&target)?,
        TargetKind::Unsupported => {
            return Err(OpError::new(
                "unsupported_install_target",
                format!("unsupported install target: {target}"),
            ));
        }
    };
    if records.is_empty() {
        return Err(OpError::new(
            "capability_install_invalid",
            "install target did not contain any SKILL.md records",
        ));
    }
    let store = CapabilityStore::new(home.clone());
    let snapshots = validate_install_batch(&store, &records, &group_id, &scope, &actor_id)?;
    let mut imported = Vec::new();
    let mut installed = Vec::new();
    let mut enabled = Vec::new();
    let mut changed = Vec::new();
    for (index, record) in records.into_iter().enumerate() {
        let snapshot = &snapshots[index];
        let capability = match store.import_record(record.clone()) {
            Ok(capability) => capability,
            Err(error) => {
                rollback_install(
                    home,
                    &store,
                    &snapshots[..index],
                    &group_id,
                    &actor_id,
                    &scope,
                )?;
                store
                    .restore_record(&snapshot.capability_id, snapshot.previous_record.clone())
                    .map_err(|rollback| {
                        OpError::new(
                            "capability_install_rollback_failed",
                            format!("{error}; failed to restore catalog record: {rollback}"),
                        )
                    })?;
                return Err(OpError::invalid(error));
            }
        };
        let enable_result = match enable(
            home,
            &group_id,
            &actor_id,
            &scope,
            ttl_seconds,
            &capability.id,
        ) {
            Ok(result) => result,
            Err(error) => {
                rollback_install(
                    home,
                    &store,
                    &snapshots[..=index],
                    &group_id,
                    &actor_id,
                    &scope,
                )?;
                return Err(error);
            }
        };
        let active_after_import = enable_result
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        installed.push(capability.id.clone());
        let record_changed =
            super::install_events::records_differ(snapshot.previous_record.as_ref(), Some(&record));
        if active_after_import {
            enabled.push(capability.id.clone());
        }
        if record_changed || (active_after_import && (!snapshot.enabled || snapshot.hidden)) {
            changed.push(capability.id.clone());
        }
        imported.push(json!({
            "capability_id":capability.id,"ok":true,
            "state":enable_result.get("state").and_then(Value::as_str).unwrap_or("blocked"),
            "active_after_import":active_after_import,"record":record,"enable_result":enable_result
        }));
    }
    let requires_setup = enabled.len() < installed.len();
    let result = object(json!({
        "action_id":action_id,"group_id":group_id,"actor_id":actor_id,
        "target":target,"target_kind":kind.as_str(),"scope":scope,
        "installed_capability_ids":installed,"enabled_capability_ids":enabled,
        "use_ready_capability_ids":enabled,"requires_setup":requires_setup,
        "refresh_required":!changed.is_empty(),"imported_capabilities":imported,
        "state":if requires_setup {"needs_setup"} else {"ready"}
    }))?;
    super::install_events::finish(
        home,
        result,
        &super::install_events::InstallChange {
            action_id: &action_id,
            group_id: &group_id,
            actor_id: &actor_id,
            by: &by,
            scope: &scope,
            capability_ids: &changed,
        },
    )
}

pub(super) fn enable(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    scope: &str,
    ttl_seconds: i64,
    capability_id: &str,
) -> Result<Map<String, Value>, OpError> {
    let store = CapabilityStore::new(home.clone());
    if let Some((blocked_scope, block)) = store
        .blocked_for_group(capability_id, group_id)
        .map_err(OpError::io)?
    {
        let reason = if blocked_scope == "global" {
            "blocked_by_global_policy"
        } else {
            "blocked_by_group_policy"
        };
        return object(json!({
            "group_id":group_id,"actor_id":actor_id,"capability_id":capability_id,
            "scope":scope,"enabled":false,"state":"blocked",
            "refresh_required":false,"reason":reason,
            "policy_level":"blocked","blocked_scope":blocked_scope,
            "blocked_reason":block.get("reason").and_then(Value::as_str).unwrap_or("")
        }));
    }
    let record = store.catalog_record(capability_id).map_err(OpError::io)?;
    if let Some(record) = record.as_ref() {
        validate_enableable(record, capability_id)?;
        package_install::ensure_installed(home, capability_id, record)?;
    }
    let already_enabled = store
        .is_enabled_for(capability_id, group_id, actor_id, scope)
        .map_err(OpError::io)?;
    let was_hidden = !actor_id.is_empty()
        && store
            .is_hidden_for(capability_id, group_id, actor_id)
            .map_err(OpError::io)?;
    let state = store
        .enable_and_unhide_for(capability_id, group_id, actor_id, scope, ttl_seconds)
        .map_err(OpError::invalid)?;
    object(json!({
        "capability_id":capability_id,"state":"ready","enabled":true,
        "scope":scope,"refresh_required":!already_enabled || was_hidden,
        "already_enabled":already_enabled,"visibility_changed":was_hidden,
        "capability_state":state
    }))
}

fn validate_install_batch(
    store: &CapabilityStore,
    records: &[Value],
    group_id: &str,
    scope: &str,
    actor_id: &str,
) -> Result<Vec<InstallSnapshot>, OpError> {
    if !matches!(scope, "group" | "actor" | "session") {
        return Err(OpError::new(
            "capability_install_invalid",
            "scope must be group, actor, or session",
        ));
    }
    if matches!(scope, "actor" | "session") && actor_id.is_empty() {
        return Err(OpError::new(
            "capability_install_invalid",
            format!("actor_id is required for {scope} scope"),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut snapshots = Vec::new();
    for record in records {
        let capability = store.validate_record(record).map_err(OpError::invalid)?;
        validate_enableable(record, &capability.id)?;
        if !ids.insert(capability.id.clone()) {
            return Err(OpError::new(
                "capability_install_invalid",
                format!(
                    "duplicate capability id in install target: {}",
                    capability.id
                ),
            ));
        }
        snapshots.push(InstallSnapshot {
            previous_record: store.catalog_record(&capability.id).map_err(OpError::io)?,
            enabled: store
                .is_enabled_for(&capability.id, group_id, actor_id, scope)
                .map_err(OpError::io)?,
            hidden: !actor_id.is_empty()
                && store
                    .is_hidden_for(&capability.id, group_id, actor_id)
                    .map_err(OpError::io)?,
            capability_id: capability.id,
        });
    }
    Ok(snapshots)
}

struct InstallSnapshot {
    capability_id: String,
    previous_record: Option<Value>,
    enabled: bool,
    hidden: bool,
}

fn rollback_install(
    home: &HomeLayout,
    store: &CapabilityStore,
    snapshots: &[InstallSnapshot],
    group_id: &str,
    actor_id: &str,
    scope: &str,
) -> Result<(), OpError> {
    let mut failures = Vec::new();
    for snapshot in snapshots.iter().rev() {
        if !snapshot.enabled {
            if let Err(error) = store.set_enabled_for(
                &snapshot.capability_id,
                false,
                group_id,
                actor_id,
                scope,
                3600,
            ) {
                failures.push(error.to_string());
            }
        }
        if snapshot.hidden
            && let Err(error) =
                store.set_hidden_for(&snapshot.capability_id, true, group_id, actor_id)
        {
            failures.push(error.to_string());
        }
        if let Err(error) =
            store.restore_record(&snapshot.capability_id, snapshot.previous_record.clone())
        {
            failures.push(error.to_string());
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(OpError::new(
            "capability_install_rollback_failed",
            format!(
                "capability install rollback failed under {}: {}",
                home.root().display(),
                failures.join("; ")
            ),
        ))
    }
}

fn validate_enableable(record: &Value, capability_id: &str) -> Result<(), OpError> {
    if record.get("enable_supported").and_then(Value::as_bool) == Some(false)
        || record.get("qualification_status").and_then(Value::as_str) == Some("blocked")
    {
        return Err(OpError::new(
            "capability_not_enableable",
            format!("capability is not qualified for activation: {capability_id}"),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum TargetKind {
    CapabilityId,
    Github,
    Url,
    Local,
    Unsupported,
}

impl TargetKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CapabilityId => "capability_id",
            Self::Github => "github",
            Self::Url => "url",
            Self::Local => "local_path",
            Self::Unsupported => "unsupported",
        }
    }
}

fn classify(target: &str) -> TargetKind {
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("skill:") || lower.starts_with("mcp:") || lower.starts_with("pack:") {
        TargetKind::CapabilityId
    } else if lower.starts_with("file://")
        || lower.starts_with("./")
        || lower.starts_with("../")
        || lower.starts_with('/')
        || lower.starts_with('~')
    {
        TargetKind::Local
    } else if github_reference(target).is_some() {
        TargetKind::Github
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        TargetKind::Url
    } else {
        TargetKind::Unsupported
    }
}

fn local_records(target: &str) -> Result<Vec<Value>, OpError> {
    let path = local_path(target)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(OpError::io)?;
    if metadata.file_type().is_symlink() {
        return Err(OpError::new(
            "capability_install_invalid",
            "local install target cannot be a symlink",
        ));
    }
    let skill = if metadata.is_dir() {
        path.join("SKILL.md")
    } else {
        path
    };
    if skill.file_name().and_then(|value| value.to_str()) != Some("SKILL.md") || !skill.is_file() {
        return Err(OpError::new(
            "capability_install_invalid",
            "local path must be a SKILL.md file or directory containing SKILL.md",
        ));
    }
    let markdown = std::fs::read_to_string(&skill).map_err(OpError::io)?;
    let directory = skill
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .unwrap_or("local-skill");
    Ok(vec![skill_record(
        &markdown,
        "local_import",
        &skill.to_string_lossy(),
        "local",
        directory,
    )?])
}

fn local_path(target: &str) -> Result<PathBuf, OpError> {
    let value = if let Some(value) = target.strip_prefix("file://") {
        value.to_owned()
    } else if let Some(value) = target.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| OpError::new("capability_install_invalid", "HOME is not available"))?;
        return Ok(home.join(value));
    } else {
        target.to_owned()
    };
    Ok(PathBuf::from(value))
}

fn url_record(target: &str) -> Result<Value, OpError> {
    let url = reqwest::Url::parse(target).map_err(OpError::invalid)?;
    let markdown = client()?
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "text/markdown, text/plain")
        .send()
        .map_err(OpError::invalid)?
        .error_for_status()
        .map_err(OpError::invalid)?
        .text()
        .map_err(OpError::invalid)?;
    let name = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .filter(|value| !value.eq_ignore_ascii_case("SKILL.md"))
        .or_else(|| url.path_segments().and_then(|parts| parts.rev().nth(1)))
        .unwrap_or("url-skill");
    skill_record(&markdown, "url_import", target, "url", name)
}

fn github_records(target: &str) -> Result<Vec<Value>, OpError> {
    let (owner, repo, requested_ref) = github_reference(target)
        .ok_or_else(|| OpError::new("capability_install_invalid", "invalid GitHub target"))?;
    let client = client()?;
    let branch = if let Some(reference) = requested_ref {
        reference
    } else {
        let value = github_get(
            &client,
            &format!("https://api.github.com/repos/{owner}/{repo}"),
        )?;
        value["default_branch"]
            .as_str()
            .unwrap_or("main")
            .to_owned()
    };
    let tree = github_get(
        &client,
        &format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{branch}?recursive=1"),
    )?;
    let paths = tree["tree"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "blob")
        .filter_map(|item| item["path"].as_str())
        .filter(|path| {
            *path == "SKILL.md" || (path.starts_with("skills/") && path.ends_with("/SKILL.md"))
        })
        .take(MAX_GITHUB_SKILLS)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for path in paths {
        let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}");
        let markdown = github_text(&client, &url)?;
        let directory = Path::new(&path)
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap_or(&repo);
        let mut record = skill_record(
            &markdown,
            "github_import",
            &format!(
                "https://github.com/{owner}/{repo}/tree/{branch}/{}",
                Path::new(&path).parent().unwrap_or(Path::new("")).display()
            ),
            "github",
            directory,
        )?;
        let name = record["name"].as_str().unwrap_or(directory);
        record["capability_id"] = json!(format!(
            "skill:github:{}:{}",
            slug(&owner, "github"),
            slug(name, directory)
        ));
        records.push(record);
    }
    Ok(records)
}

fn github_reference(target: &str) -> Option<(String, String, Option<String>)> {
    let trimmed = target.trim().trim_end_matches('/').trim_end_matches(".git");
    if !trimmed.contains("://") && !trimmed.starts_with("git@") {
        let parts = trimmed.split('/').collect::<Vec<_>>();
        if parts.len() == 2 && parts.iter().all(|part| valid_github_token(part)) {
            return Some((parts[0].into(), parts[1].into(), None));
        }
    }
    let url = reqwest::Url::parse(trimmed).ok()?;
    if url.host_str()? != "github.com" {
        return None;
    }
    let parts = url.path_segments()?.collect::<Vec<_>>();
    if parts.len() < 2 || !valid_github_token(parts[0]) || !valid_github_token(parts[1]) {
        return None;
    }
    let reference = (parts.get(2) == Some(&"tree") || parts.get(2) == Some(&"blob"))
        .then(|| parts.get(3).copied())
        .flatten()
        .map(str::to_owned);
    Some((
        parts[0].into(),
        parts[1].trim_end_matches(".git").into(),
        reference,
    ))
}

fn valid_github_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn client() -> Result<Client, OpError> {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent("cccc-capability-installer/1.0")
        .build()
        .map_err(OpError::invalid)
}

fn github_get(client: &Client, url: &str) -> Result<Value, OpError> {
    github_request(client, url)
        .send()
        .map_err(OpError::invalid)?
        .error_for_status()
        .map_err(OpError::invalid)?
        .json()
        .map_err(OpError::invalid)
}

fn github_text(client: &Client, url: &str) -> Result<String, OpError> {
    github_request(client, url)
        .send()
        .map_err(OpError::invalid)?
        .error_for_status()
        .map_err(OpError::invalid)?
        .text()
        .map_err(OpError::invalid)
}

fn github_request(client: &Client, url: &str) -> reqwest::blocking::RequestBuilder {
    let mut request = client.get(url);
    if let Some(token) = std::env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| std::env::var("GH_TOKEN").ok())
        .filter(|value| !value.trim().is_empty())
    {
        request = request.bearer_auth(token);
    }
    request
}

fn skill_record(
    markdown: &str,
    source_id: &str,
    source_uri: &str,
    namespace: &str,
    fallback_name: &str,
) -> Result<Value, OpError> {
    if markdown.len() > 2 * 1024 * 1024 {
        return Err(OpError::new(
            "capability_install_invalid",
            "SKILL.md exceeds the 2 MiB limit",
        ));
    }
    let (frontmatter, _) = frontmatter(markdown)?;
    let name = frontmatter
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_name);
    let description = frontmatter
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let mut reasons = Vec::new();
    if name.is_empty() {
        reasons.push("frontmatter.name is required");
    }
    if description.is_empty() {
        reasons.push("frontmatter.description is required");
    }
    let tags = frontmatter
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut all_tags = vec![
        "skill".to_owned(),
        "external".to_owned(),
        namespace.to_owned(),
    ];
    all_tags.extend(tags);
    let version = format!("{:x}", Sha256::digest(markdown.as_bytes()));
    let qualified = reasons.is_empty();
    Ok(json!({
        "capability_id":format!("skill:{namespace}:{}",slug(name,fallback_name)),
        "kind":"skill","name":name,
        "description_short":if description.is_empty(){format!("{namespace} skill {name}")}else{description.to_owned()},
        "tags":all_tags,
        "source_id":source_id,"source_uri":source_uri,"source_record_id":source_uri,
        "source_record_version":version,"updated_at_source":utc_now(),"last_synced_at":utc_now(),
        "sync_state":if source_id=="local_import"{"local"}else{"remote"},
        "install_mode":"builtin","install_spec":{},"requirements":{},"trust_tier":"tier2",
        "qualification_status":if qualified{"qualified"}else{"blocked"},
        "qualification_reasons":reasons,"enable_supported":qualified,
        "capsule_text":markdown
    }))
}

fn frontmatter(markdown: &str) -> Result<(Map<String, Value>, &str), OpError> {
    let normalized = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Ok((Map::new(), normalized));
    };
    let Some((yaml, body)) = rest.split_once("\n---\n") else {
        return Err(OpError::new(
            "capability_install_invalid",
            "SKILL.md frontmatter is not terminated",
        ));
    };
    let value: Value = serde_yaml::from_str(yaml).map_err(OpError::invalid)?;
    Ok((value.as_object().cloned().unwrap_or_default(), body))
}

fn slug(value: &str, fallback: &str) -> String {
    let slug = value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        fallback.to_owned()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_supported_install_targets() {
        assert!(matches!(classify("skill:test"), TargetKind::CapabilityId));
        assert!(matches!(classify("owner/repo"), TargetKind::Github));
        assert!(matches!(
            classify("https://example.test/SKILL.md"),
            TargetKind::Url
        ));
        assert!(matches!(classify("./skill"), TargetKind::Local));
    }

    #[test]
    fn skill_record_keeps_complete_markdown() {
        let markdown = "---\nname: review\ndescription: Review changes\n---\nDo the review.";
        let record = skill_record(markdown, "local_import", "/tmp/review", "local", "review")
            .expect("record");
        assert_eq!(record["capability_id"], "skill:local:review");
        assert_eq!(record["capsule_text"], markdown);
        assert_eq!(record["enable_supported"], true);
    }
}
