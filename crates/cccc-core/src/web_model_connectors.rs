//! One authenticated Web Model entrance, with explicitly paired conversation routes.
//! Old Actor-bound credentials are deliberately not promoted to instance authority.
use std::io;
use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{HomeLayout, fs};

const VERSION: u64 = 2;
const PAIRING_TTL_MS: i64 = 10 * 60 * 1000;

fn store_path(home: &HomeLayout) -> PathBuf {
    home.root().join("web_model_connectors.yaml")
}
fn lock_path(home: &HomeLayout) -> PathBuf {
    store_path(home).with_extension("yaml.lock")
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn route_key(group: &str, actor: &str) -> String {
    json!([group, actor]).to_string()
}
fn error(message: &str) -> io::Error {
    io::Error::other(message)
}
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn read(home: &HomeLayout) -> io::Result<Value> {
    let path = store_path(home);
    if !path.exists() {
        return Ok(json!({"version":VERSION,"connector":null}));
    }
    let raw = fs::read_yaml::<Value>(&path)?;
    match raw["version"].as_u64() {
        Some(VERSION) => Ok(raw),
        None | Some(1) => {
            Ok(json!({"version":VERSION,"connector":null,"requires_reconfiguration":true}))
        }
        _ => Err(error("unsupported web-model connector store version")),
    }
}

fn update<T>(home: &HomeLayout, change: impl FnOnce(&mut Value) -> io::Result<T>) -> io::Result<T> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let mut root = read(home)?;
        let result = change(&mut root)?;
        fs::write_secret_yaml(&store_path(home), &root)?;
        Ok(result)
    })
}

pub fn load(home: &HomeLayout) -> io::Result<Vec<Value>> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let root = read(home)?;
        Ok(root
            .get("connector")
            .filter(|v| v.is_object())
            .cloned()
            .into_iter()
            .collect())
    })
}

pub fn requires_reconfiguration(home: &HomeLayout) -> io::Result<bool> {
    let root = read(home)?;
    Ok(root["requires_reconfiguration"] == true
        || (root["connector"].is_null()
            && crate::settings::load(home)?
                .extra
                .contains_key("web_model_connectors")))
}

fn current<'a>(root: &'a mut Value, id: &str) -> io::Result<&'a mut Value> {
    let connector = root
        .get_mut("connector")
        .filter(|c| c.is_object() && c["connector_id"] == id && c["revoked"] != true)
        .ok_or_else(|| error("connector_unavailable"))?;
    Ok(connector)
}

/// Configure or rotate the current entrance. Return the new credential once;
/// its hash is the only credential representation persisted in the store.
pub fn configure(home: &HomeLayout) -> io::Result<Value> {
    update(home, |root| {
        let old = &root["connector"];
        let now = cccc_contracts::utc_now();
        let mut connector = if old.is_object() && old["revoked"] != true {
            old.clone()
        } else {
            json!({"connector_id":format!("wmc_{}",Uuid::new_v4().simple()),"kind":"web_model_connector","routing_mode":"session","provider":"chatgpt_web","routing_salt":Uuid::new_v4().to_string(),"created_at":now,"bindings":{},"pairings":{}})
        };
        let secret = format!(
            "wmcs_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        connector["secret_hash"] = json!(hash(&secret));
        connector["secret_preview"] =
            json!(format!("{}…{}", &secret[..6], &secret[secret.len() - 4..]));
        connector["updated_at"] = json!(now);
        connector["revoked"] = json!(false);
        *root = json!({"version":VERSION,"connector":connector});
        Ok(json!({"connector":root["connector"],"secret":secret}))
    })
}

pub fn revoke(home: &HomeLayout, id: &str) -> io::Result<bool> {
    update(home, |root| {
        if root["connector"]["connector_id"] != id {
            return Ok(false);
        }
        root["connector"]["revoked"] = json!(true);
        root["connector"]["updated_at"] = json!(cccc_contracts::utc_now());
        root["connector"]["pairings"] = json!({});
        Ok(true)
    })
}

pub fn secret_matches(item: &Value, supplied: &str) -> bool {
    !supplied.is_empty() && item["secret_hash"].as_str() == Some(hash(supplied).as_str())
}

/// Credential-scoped correlation, not authentication. Never use model arguments
/// or HTTP transport session IDs as a substitute for per-call host metadata.
pub fn session_key(connector: &Value, metadata: &Value) -> io::Result<String> {
    let metadata = metadata
        .as_object()
        .ok_or_else(|| error("session_missing"))?;
    let mut parts = vec![
        json!("cccc-web-model-session-v1"),
        connector["connector_id"].clone(),
        connector["routing_salt"].clone(),
    ];
    for name in ["openai/session", "openai/subject", "openai/organization"] {
        match metadata.get(name) {
            Some(value) => {
                let text = value
                    .as_str()
                    .filter(|v| {
                        !v.trim().is_empty() && v.len() <= 1024 && !v.chars().any(char::is_control)
                    })
                    .ok_or_else(|| error("session_metadata_invalid"))?;
                parts.push(json!(text));
            }
            None if name == "openai/session" => return Err(error("session_missing")),
            None => parts.push(Value::Null),
        }
    }
    Ok(hash(&Value::Array(parts).to_string()))
}

pub fn binding_for_actor(
    connector: &Value,
    group: &str,
    actor: &str,
    generation: &str,
) -> Option<Value> {
    if connector["revoked"] == true {
        return None;
    }
    connector["bindings"]
        .get(route_key(group, actor))
        .filter(|b| b["generation"] == generation && b["state"] == "bound")
        .cloned()
}

pub fn binding_for_session(connector: &Value, key: &str) -> Option<Value> {
    if connector["revoked"] == true {
        return None;
    }
    connector["bindings"]
        .as_object()?
        .values()
        .find(|b| b["session_key"] == key && b["state"] == "bound")
        .cloned()
}

/// Revalidate a captured route before a nested call; re-pairing or replacing an
/// Actor must not lend the new identity to an already running code cell.
pub fn validate_binding(home: &HomeLayout, expected: &Value) -> io::Result<()> {
    let connector = load(home)?
        .into_iter()
        .find(|c| c["connector_id"] == expected["connector_id"])
        .ok_or_else(|| error("connector_unavailable"))?;
    let group_id = expected["group_id"].as_str().unwrap_or_default();
    let actor_id = expected["actor_id"].as_str().unwrap_or_default();
    let generation = expected["generation"].as_str().unwrap_or_default();
    let group = crate::GroupStore::new(home.clone())?.load(group_id)?;
    let actor = group
        .actors
        .iter()
        .find(|a| a.id == actor_id)
        .ok_or_else(|| error("paired_actor_unavailable"))?;
    if generation.is_empty()
        || crate::actors::generation_identity(actor) != generation
        || !actor.enabled
        || actor.runtime != cccc_contracts::ActorRuntime::WebModel
        || actor.runner != cccc_contracts::RunnerKind::Headless
    {
        return Err(error("paired_actor_unavailable"));
    }
    let current = binding_for_actor(&connector, group_id, actor_id, generation)
        .ok_or_else(|| error("conversation_not_paired"))?;
    if current["revision"] != expected["revision"] {
        return Err(error("conversation_pairing_changed"));
    }
    Ok(())
}

pub fn begin_pairing(
    home: &HomeLayout,
    id: &str,
    group: &str,
    actor: &str,
    generation: &str,
    automatic: bool,
) -> io::Result<Value> {
    if generation.is_empty() {
        return Err(error("actor_generation_required"));
    }
    update(home, |root| {
        let c = current(root, id)?;
        let code = format!(
            "wm_pair_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let operation = Uuid::new_v4().to_string();
        let now = now_ms();
        let record = json!({"pairing_id":operation,"group_id":group,"actor_id":actor,"generation":generation,"automatic":automatic,"code_hash":hash(&code),"expires_at_ms":now+PAIRING_TTL_MS,"state":"waiting","session_key":null});
        // One record per Actor. Expired/failed attempts are retained until an
        // explicit retry or lifecycle removal, so a supervisor cannot resend.
        c["pairings"][route_key(group, actor)] = record;
        Ok(
            json!({"pairing_id":operation,"code":code,"expires_at_ms":now+PAIRING_TTL_MS,"state":"waiting"}),
        )
    })
}

pub fn accept_pairing(home: &HomeLayout, id: &str, code: &str, session: &str) -> io::Result<Value> {
    if code.is_empty() || session.is_empty() {
        return Err(error("pairing_required"));
    }
    update(home, |root| {
        let c = current(root, id)?;
        let digest = hash(code);
        let pair = c["pairings"]
            .as_object_mut()
            .and_then(|ps| ps.values_mut().find(|p| p["code_hash"] == digest))
            .ok_or_else(|| error("pairing_invalid"))?;
        if pair["expires_at_ms"].as_i64().is_none_or(|t| t <= now_ms()) {
            return Err(error("pairing_expired"));
        }
        if matches!(pair["state"].as_str(), Some("failed" | "cancelled")) {
            return Err(error("pairing_failed"));
        }
        if pair["session_key"]
            .as_str()
            .is_some_and(|existing| existing != session)
        {
            return Err(error("pairing_already_used"));
        }
        pair["session_key"] = json!(session);
        // Returned only through this conversation's MCP call. The browser port
        // checks its assistant-side echo before granting the route authority.
        if pair["receipt"].is_null() {
            pair["receipt"] = json!(format!("CCCC_PAIR_RECEIPT_{}", Uuid::new_v4().simple()));
        }
        if pair["state"] != "bound" {
            pair["state"] = json!("awaiting_confirmation");
        }
        Ok(pair.clone())
    })
}

pub fn pairing_for_actor(connector: &Value, group: &str, actor: &str) -> Option<Value> {
    connector["pairings"]
        .get(route_key(group, actor))
        .filter(|p| p.is_object())
        .cloned()
}

/// A stop/pause invalidates a pending automatic handshake before the lifecycle
/// transition. Retain the attempt so a quick restart cannot silently resend it.
pub fn interrupt_automatic_pairings(
    home: &HomeLayout,
    group: &str,
    actor: Option<&str>,
) -> io::Result<()> {
    if !store_path(home).exists() {
        return Ok(());
    }
    fs::with_exclusive_lock(&lock_path(home), || {
        let mut root = read(home)?;
        let mut changed = false;
        if let Some(pairs) = root["connector"]["pairings"].as_object_mut() {
            for pair in pairs.values_mut().filter(|p| {
                p["group_id"] == group
                    && actor.is_none_or(|a| p["actor_id"] == a)
                    && p["automatic"] == true
                    && matches!(
                        p["state"].as_str(),
                        Some("waiting" | "awaiting_confirmation")
                    )
            }) {
                pair["state"] = json!("failed");
                pair["error_code"] = json!("pairing_interrupted");
                pair["receipt"] = Value::Null;
                pair["session_key"] = Value::Null;
                changed = true;
            }
        }
        if changed {
            fs::write_secret_yaml(&store_path(home), &root)?;
        }
        Ok(())
    })
}

pub fn cancel_pairing(
    home: &HomeLayout,
    id: &str,
    group: &str,
    actor: &str,
    pairing_id: &str,
) -> io::Result<()> {
    update(home, |root| {
        let pairings = current(root, id)?["pairings"]
            .as_object_mut()
            .ok_or_else(|| error("invalid pairing store"))?;
        let key = route_key(group, actor);
        if pairings
            .get(&key)
            .is_some_and(|p| p["pairing_id"] != pairing_id)
        {
            return Err(error("pairing_changed"));
        }
        if let Some(pair) = pairings.get_mut(&key).filter(|p| p["state"] != "bound") {
            pair["state"] = json!("cancelled");
            pair["error_code"] = json!("pairing_cancelled");
            pair["receipt"] = Value::Null;
            pair["session_key"] = Value::Null;
        }
        Ok(())
    })
}

/// End only the named attempt; a late worker must not change a replacement or
/// an already committed binding. Preserve the failure for the setup UI.
pub fn fail_pairing(
    home: &HomeLayout,
    id: &str,
    group: &str,
    actor: &str,
    pairing_id: &str,
    reason: &str,
) -> io::Result<()> {
    update(home, |root| {
        let pair = &mut current(root, id)?["pairings"][route_key(group, actor)];
        if pair["pairing_id"] != pairing_id
            || matches!(pair["state"].as_str(), Some("bound" | "cancelled"))
        {
            return Err(error("pairing_changed"));
        }
        pair["state"] = json!("failed");
        pair["error_code"] = json!(reason);
        pair["session_key"] = Value::Null;
        pair["receipt"] = Value::Null;
        Ok(())
    })
}

/// Persist the binding after the private browser port verifies the conversation. The
/// browser delivery target is this record, so activation is a single store write.
pub fn confirm_pairing(
    home: &HomeLayout,
    id: &str,
    group: &str,
    actor: &str,
    generation: &str,
    pairing_id: &str,
    url: &str,
) -> io::Result<Value> {
    let url = conversation_url(url)?;
    update(home, |root| {
        let c = current(root, id)?;
        let key = route_key(group, actor);
        let pair = pairing_for_actor(c, group, actor).ok_or_else(|| error("pairing_expired"))?;
        if pair["expires_at_ms"].as_i64().is_none_or(|t| t <= now_ms()) {
            return Err(error("pairing_expired"));
        }
        if pair["pairing_id"] != pairing_id || pair["generation"] != generation {
            return Err(error("pairing_changed"));
        }
        if matches!(pair["state"].as_str(), Some("failed" | "cancelled")) {
            return Err(error("pairing_failed"));
        }
        let session = pair["session_key"]
            .as_str()
            .ok_or_else(|| error("pairing_not_received"))?;
        let bindings = c["bindings"]
            .as_object()
            .ok_or_else(|| error("invalid binding store"))?;
        if bindings
            .iter()
            .any(|(other, b)| other != &key && (b["session_key"] == session || b["url"] == url))
        {
            return Err(error("binding_conflict"));
        }
        if let Some(bound) = bindings.get(&key).filter(|b| b["pairing_id"] == pairing_id) {
            if bound["url"] != url {
                return Err(error("pairing_target_changed"));
            }
            return Ok(bound.clone());
        }
        let bound = json!({"group_id":group,"actor_id":actor,"generation":generation,"pairing_id":pairing_id,"revision":Uuid::new_v4().to_string(),"session_key":session,"url":url,"state":"bound","updated_at":cccc_contracts::utc_now()});
        c["bindings"][&key] = bound.clone();
        c["pairings"][&key]["state"] = json!("bound");
        Ok(bound)
    })
}

pub fn conversation_url(value: &str) -> io::Result<String> {
    let mut url = url::Url::parse(value.trim()).map_err(|_| error("invalid_conversation_url"))?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !matches!(host, "chatgpt.com" | "chat.openai.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(error("invalid_conversation_url"));
    }
    let parts = url
        .path_segments()
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    let id = parts
        .windows(2)
        .find(|p| p[0] == "c")
        .map(|p| p[1])
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .ok_or_else(|| error("stable_conversation_url_required"))?;
    // Preserve custom GPT routing prefixes, but strip query/fragment state.
    let end = url
        .path()
        .find(&format!("/c/{id}"))
        .expect("validated conversation path")
        + 3
        + id.len();
    let path = url.path()[..end].to_owned();
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn retire(home: &HomeLayout, group: &str, actor: Option<&str>) -> io::Result<Vec<Value>> {
    update(home, |root| {
        let c = &mut root["connector"];
        if !c.is_object() {
            return Ok(Vec::new());
        }
        let mut retired = Vec::new();
        for collection in ["bindings", "pairings"] {
            let keys = c[collection]
                .as_object()
                .into_iter()
                .flat_map(|m| m.iter())
                .filter(|(_, b)| b["group_id"] == group && actor.is_none_or(|a| b["actor_id"] == a))
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for key in keys {
                let value = c[collection]
                    .as_object_mut()
                    .expect("map")
                    .remove(&key)
                    .expect("entry");
                retired.push(json!({"kind":"web_model_route_snapshot","connector_id":c["connector_id"],"collection":collection,"key":key,"value":value}));
            }
        }
        Ok(retired)
    })
}
pub fn retire_actor(home: &HomeLayout, group: &str, actor: &str) -> io::Result<Vec<Value>> {
    retire(home, group, Some(actor))
}
pub fn retire_group(home: &HomeLayout, group: &str) -> io::Result<Vec<Value>> {
    retire(home, group, None)
}

pub fn restore(home: &HomeLayout, entries: &[Value]) -> io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    update(home, |root| {
        let c = &mut root["connector"];
        for entry in entries {
            if entry["kind"] != "web_model_route_snapshot"
                || entry["connector_id"] != c["connector_id"]
                || c["revoked"] == true
            {
                return Err(error("connector changed during route rollback"));
            }
            let collection = entry["collection"]
                .as_str()
                .filter(|v| matches!(*v, "bindings" | "pairings"))
                .ok_or_else(|| error("invalid route snapshot"))?;
            let key = entry["key"]
                .as_str()
                .ok_or_else(|| error("invalid route snapshot"))?;
            if let Some(existing) = c[collection].get(key) {
                if existing != &entry["value"] {
                    return Err(error("route changed during rollback"));
                }
            }
            c[collection][key] = entry["value"].clone();
        }
        Ok(())
    })
}

pub fn update_connector(
    home: &HomeLayout,
    id: &str,
    change: impl FnOnce(&mut Value),
) -> io::Result<bool> {
    update(home, |root| {
        if root["connector"]["connector_id"] != id {
            return Ok(false);
        }
        change(&mut root["connector"]);
        Ok(true)
    })
}

#[cfg(test)]
mod tests;
