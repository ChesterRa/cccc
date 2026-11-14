//! Member credentials and authenticated discovery never enter shared artifacts.
use cccc_contracts::Actor;
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::dispatch::OpError;

#[cfg(test)]
mod tests;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Scope {
    home: PathBuf,
    group: String,
    group_generation: String,
    actor: String,
    actor_generation: String,
    capability: String,
}
struct Entry {
    tag: Vec<u8>,
    tools: Vec<Value>,
    failure: Option<&'static str>,
    updated: Instant,
}
struct Cache {
    key: [u8; 32],
    entries: BTreeMap<Scope, Entry>,
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut key = [0; 32];
        key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Mutex::new(Cache {
            key,
            entries: BTreeMap::new(),
        })
    })
}

pub(super) fn bearer(record: &Value) -> bool {
    record.pointer("/support/auth").and_then(Value::as_str) == Some("bearer")
}

pub(super) fn env_name(record: &Value) -> Option<&str> {
    cccc_core::capability_recommendations::bearer_env(record)
}

/// Resolve the installed public recipe, not a silently updated catalog candidate.
pub(super) fn execution_record(record: &Value, runtime: &Value) -> Value {
    let id = record["capability_id"].as_str().unwrap_or("");
    let artifact = runtime["capability_artifacts"][id]
        .as_str()
        .and_then(|id| runtime["artifacts"].get(id));
    let mut result = record.clone();
    if let Some(artifact) = artifact {
        let invoker = &artifact["invoker"];
        if let Some(url) = invoker["url"].as_str() {
            result["install_mode"] = json!("remote_only");
            result["install_spec"] = json!({"url":url});
            result["support"]["form"] = json!("remote_http");
            result["support"]["auth"] = json!(if invoker["credential"].is_object() {
                "bearer"
            } else {
                "none"
            });
            result["support"]["credential"] = invoker["credential"].clone();
            result["execution_fingerprint"] = artifact["recipe_fingerprint"].clone();
        } else if invoker["command"].is_array() {
            result["support"]["form"] = json!("stdio");
            result["support"]["auth"] = json!("none");
        }
    }
    result
}

pub(super) fn installed_record(home: &HomeLayout, record: &Value) -> Result<Value, OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    let runtime = if path.exists() {
        cccc_core::fs::read_json(&path).map_err(OpError::io)?
    } else {
        json!({})
    };
    Ok(execution_record(record, &runtime))
}

struct Identity {
    scope: Scope,
    tag: Vec<u8>,
    token: String,
    url: String,
    env: String,
}

fn identity(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: Option<&Actor>,
    record: &Value,
) -> Result<Identity, OpError> {
    let actor = actor
        .filter(|actor| {
            !matches!(actor.id.as_str(), "user" | "system")
                && actor.external_session_id.is_empty()
                && !actor.runtime.is_web_model()
        })
        .ok_or_else(|| error("credential_route_not_supported", None))?;
    if record["install_mode"] != "remote_only" || !bearer(record) {
        return Err(error("credential_route_not_supported", None));
    }
    let env = env_name(record).ok_or_else(|| error("credential_route_not_supported", None))?;
    let token = crate::ops::actor_secrets::effective_values(home, &group.group_id, actor)
        .map_err(|_| error("missing_member_credential", Some(env)))?
        .remove(env)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error("missing_member_credential", Some(env)))?;
    if reqwest_mcp::header::HeaderValue::from_str(&token).is_err() {
        return Err(error("credential_rejected", Some(env)));
    }
    let url = record
        .pointer("/install_spec/url")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
        return Err(error("credential_route_not_supported", Some(env)));
    }
    let scope = Scope {
        home: home.root().to_owned(),
        group: group.group_id.clone(),
        group_generation: group.generation.clone(),
        actor: actor.id.clone(),
        actor_generation: format!("{}:{}", actor.generation, actor.created_at),
        capability: record["capability_id"].as_str().unwrap_or("").into(),
    };
    let inputs = json!([
        url,
        env,
        token,
        actor.profile_id,
        actor.profile_scope,
        actor.profile_owner,
        record.get("execution_fingerprint"),
        super::recipe::fingerprint(record).ok()
    ]);
    let cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut mac = Hmac::<Sha256>::new_from_slice(&cache.key).expect("fixed HMAC key");
    mac.update(&serde_json::to_vec(&inputs).expect("credential context JSON"));
    let tag = mac.finalize().into_bytes().to_vec();
    Ok(Identity {
        scope,
        tag,
        token,
        url: url.into(),
        env: env.into(),
    })
}

fn load_identity(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    record: &Value,
) -> Result<Identity, OpError> {
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(group_id)
        .map_err(OpError::io)?;
    identity(
        home,
        &group,
        group.actors.iter().find(|actor| actor.id == actor_id),
        record,
    )
}

fn remember(identity: &Identity, tools: Vec<Value>, failure: Option<&'static str>) {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Failed metadata is never projected. Retain the previous same-context names
    // only to route an explicitly retried direct call through fresh discovery.
    let tools = if failure.is_some() && tools.is_empty() {
        cache
            .entries
            .get(&identity.scope)
            .filter(|entry| entry.tag == identity.tag)
            .map(|entry| entry.tools.clone())
            .unwrap_or_default()
    } else {
        tools
    };
    // Discovery is replaceable, process-local metadata, with bounded membership.
    if cache.entries.len() >= 128
        && !cache.entries.contains_key(&identity.scope)
        && let Some(oldest) = cache
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.updated)
            .map(|(key, _)| key.clone())
    {
        cache.entries.remove(&oldest);
    }
    cache.entries.insert(
        identity.scope.clone(),
        Entry {
            tag: identity.tag.clone(),
            tools,
            failure,
            updated: Instant::now(),
        },
    );
}

fn discover(identity: &Identity, id: &str) -> Result<Vec<Value>, OpError> {
    match super::mcp_http::list_tools_with_token(
        &identity.url,
        Some(&identity.token),
        Duration::from_secs(45),
    ) {
        Ok(mut tools) => {
            for tool in &mut tools {
                redact(tool, &identity.token);
            }
            let tools = super::package_install::normalize_tools(id, Some(&Value::Array(tools)));
            remember(identity, tools.clone(), None);
            Ok(tools)
        }
        Err(failure) => Err(failed(identity, failure, true)),
    }
}

fn failed(
    identity: &Identity,
    failure: super::mcp_http::Failure,
    discovery_failed: bool,
) -> OpError {
    let code = match failure.status {
        Some(401) => "credential_rejected",
        Some(403) => "credential_permission_denied",
        _ if failure.remote => "capability_tool_failed",
        _ => "capability_transport_error",
    };
    if discovery_failed || matches!(failure.status, Some(401 | 403)) {
        remember(identity, Vec::new(), Some(code));
    }
    if failure
        .status
        .is_some_and(|status| matches!(status, 401 | 403))
    {
        error(code, Some(&identity.env))
    } else {
        let mut result = OpError::new(code, failure.message);
        result.details.insert("reason_codes".into(), json!([code]));
        result
    }
}

pub(super) fn prepare(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    record: &Value,
) -> Result<Vec<Value>, OpError> {
    let identity = load_identity(home, group, actor, record)?;
    discover(&identity, record["capability_id"].as_str().unwrap_or(""))
}

pub(super) fn cached_tools(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    record: &Value,
) -> Vec<Value> {
    let Ok(identity) = load_identity(home, group, actor, record) else {
        return Vec::new();
    };
    tools_for(&identity)
}

fn tools_for(identity: &Identity) -> Vec<Value> {
    let cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entries
        .get(&identity.scope)
        .filter(|entry| entry.tag == identity.tag && entry.failure.is_none())
        .map(|entry| entry.tools.clone())
        .unwrap_or_default()
}

pub(super) fn pending(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: Option<&Actor>,
    record: &Value,
) -> bool {
    let Ok(identity) = identity(home, group, actor, record) else {
        return true;
    };
    let cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    !cache
        .entries
        .get(&identity.scope)
        .is_some_and(|entry| entry.tag == identity.tag && entry.failure.is_none())
}

pub(super) fn availability(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: Option<&Actor>,
    record: &Value,
) -> (&'static str, Vec<&'static str>) {
    match identity(home, group, actor, record) {
        Err(error) => {
            if error.code == "credential_route_not_supported" {
                ("unsupported", vec!["credential_route_not_supported"])
            } else {
                (
                    "needs_setup",
                    vec![if error.code == "credential_rejected" {
                        "credential_rejected"
                    } else {
                        "missing_member_credential"
                    }],
                )
            }
        }
        Ok(identity) => {
            let cache = cache()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match cache
                .entries
                .get(&identity.scope)
                .filter(|entry| entry.tag == identity.tag)
                .and_then(|entry| entry.failure)
            {
                Some("credential_rejected") => ("needs_setup", vec!["credential_rejected"]),
                Some("credential_permission_denied") => {
                    ("needs_setup", vec!["credential_permission_denied"])
                }
                _ => ("compatible", vec![]),
            }
        }
    }
}

pub(super) fn known_capability(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    enabled: &BTreeSet<String>,
    tool: &str,
) -> Option<String> {
    let group = GroupStore::new(home.clone()).ok()?.load(group_id).ok()?;
    let actor = group.actors.iter().find(|actor| actor.id == actor_id)?;
    let cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entries
        .iter()
        .find(|(scope, entry)| {
            scope.home.as_path() == home.root()
                && scope.group == group_id
                && scope.group_generation == group.generation
                && scope.actor == actor_id
                && scope.actor_generation == format!("{}:{}", actor.generation, actor.created_at)
                && enabled.contains(&scope.capability)
                && entry.tools.iter().any(|item| item["name"] == tool)
        })
        .map(|(scope, _)| scope.capability.clone())
}

pub(super) fn call(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    record: &Value,
    name: &str,
    arguments: Value,
) -> Result<Value, OpError> {
    let identity = load_identity(home, group, actor, record)?;
    let id = record["capability_id"].as_str().unwrap_or("");
    let mut tools = tools_for(&identity);
    if !tools
        .iter()
        .any(|tool| tool["name"] == name || tool["real_tool_name"] == name)
    {
        tools = discover(&identity, id)?;
    }
    let tool = tools
        .iter()
        .find(|tool| tool["name"] == name || tool["real_tool_name"] == name)
        .ok_or_else(|| {
            OpError::new(
                "capability_tool_not_found",
                "tool is not available for this member's credential",
            )
        })?;
    let real = tool["real_tool_name"].as_str().unwrap_or("");
    let arguments = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| OpError::new("invalid_args", "tool arguments must be an object"))?;
    let mut result = super::mcp_http::call_tool(
        &identity.url,
        Some(&identity.token),
        real,
        arguments,
        Duration::from_secs(120),
    )
    .map_err(|failure| failed(&identity, failure, false))?;
    redact(&mut result, &identity.token);
    Ok(result)
}

fn error(code: &str, env: Option<&str>) -> OpError {
    let message = match code {
        "missing_member_credential" => {
            "configure the referenced environment key in this member's settings"
        }
        "credential_rejected" => "the member credential is missing, invalid or expired",
        "credential_permission_denied" => "the member credential lacks permission for this service",
        _ => "this member has no supported credential route",
    };
    let mut error = OpError::new(code, message);
    error.details.insert("reason_codes".into(), json!([code]));
    if let Some(env) = env {
        error.details.insert("credential_env".into(), json!(env));
    }
    error
}

fn redact(value: &mut Value, token: &str) {
    match value {
        Value::String(value) => *value = value.replace(token, "<redacted>"),
        Value::Array(items) => {
            for item in items {
                redact(item, token);
            }
        }
        Value::Object(items) => {
            let old = std::mem::take(items);
            for (key, mut item) in old {
                redact(&mut item, token);
                items.insert(key.replace(token, "<redacted>"), item);
            }
        }
        _ => (),
    }
}
