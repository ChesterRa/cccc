//! Provider-scoped Web Model entrances with explicit Actor routes.
//! Old Actor-bound credentials are deliberately not promoted to instance authority.
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{HomeLayout, fs};

const VERSION: u64 = 3;

mod grok;
pub use grok::{bind_grok, grok_bot_url};
mod dots;
pub mod grok_webhook;
pub use dots::{
    DOTS_URL, begin_dots_pairing, bind_dots, confirm_dots_pairing, dots_binding_for_token,
    dots_claim_binding, dots_invalidate, dots_subscription_binding, dots_subscription_token,
    dots_update, dots_wake_token,
};
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

fn binding_mac(
    connector: &Value,
    binding: &Value,
    domain: &str,
    extra: &[Value],
) -> io::Result<Hmac<Sha256>> {
    if connector["revoked"] == true || binding["state"] != "bound" {
        return Err(error("binding_unavailable"));
    }
    let key = connector["routing_key"]
        .as_str()
        .filter(|key| !key.is_empty())
        .ok_or_else(|| error("connector_unavailable"))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|_| error("connector_unavailable"))?;
    let mut identity = vec![
        json!(domain),
        connector["connector_id"].clone(),
        binding["group_id"].clone(),
        binding["actor_id"].clone(),
        binding["generation"].clone(),
        binding["revision"].clone(),
    ];
    identity.extend_from_slice(extra);
    mac.update(Value::Array(identity).to_string().as_bytes());
    Ok(mac)
}
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn read(home: &HomeLayout) -> io::Result<Value> {
    let path = store_path(home);
    if !path.exists() {
        return Ok(json!({"version":VERSION,"connectors":{}}));
    }
    let raw = fs::read_yaml::<Value>(&path).map_err(|source| {
        io::Error::new(source.kind(), "web-model connector store could not be read")
    })?;
    match raw["version"].as_u64() {
        Some(VERSION) if raw["connectors"].is_object() => Ok(raw),
        Some(2) => {
            let mut connectors = json!({});
            if raw["connector"].is_object() {
                connectors["chatgpt_web"] = raw["connector"].clone();
            }
            Ok(json!({"version":VERSION,"connectors":connectors,
                "requires_reconfiguration":raw["requires_reconfiguration"] == true}))
        }
        None | Some(1) => {
            Ok(json!({"version":VERSION,"connectors":{},"requires_reconfiguration":true}))
        }
        _ => Err(error("unsupported web-model connector store version")),
    }
}

fn update<T>(home: &HomeLayout, change: impl FnOnce(&mut Value) -> io::Result<T>) -> io::Result<T> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let mut root = read(home)?;
        let result = change(&mut root)?;
        fs::write_secret_yaml(&store_path(home), &root)?;
        prune_rejections(home, &root);
        Ok(result)
    })
}

pub fn load(home: &HomeLayout) -> io::Result<Vec<Value>> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let root = read(home)?;
        Ok(root["connectors"]
            .as_object()
            .expect("validated store")
            .values()
            .cloned()
            .collect())
    })
}

pub fn requires_reconfiguration(home: &HomeLayout) -> io::Result<bool> {
    let root = read(home)?;
    Ok(root["requires_reconfiguration"] == true
        || (root["connectors"]["chatgpt_web"].is_null()
            && crate::settings::load(home)?
                .extra
                .contains_key("web_model_connectors")))
}

fn current<'a>(root: &'a mut Value, id: &str) -> io::Result<&'a mut Value> {
    let connector = root["connectors"]
        .as_object_mut()
        .expect("validated store")
        .values_mut()
        .find(|c| c["connector_id"] == id && c["revoked"] != true)
        .ok_or_else(|| error("connector_unavailable"))?;
    Ok(connector)
}

/// Configure or rotate the current entrance. Return the new credential once;
/// its hash is the only credential representation persisted in the store.
pub fn configure(home: &HomeLayout) -> io::Result<Value> {
    configure_provider(home, "chatgpt_web")
}

pub fn configure_provider(home: &HomeLayout, provider: &str) -> io::Result<Value> {
    configure_inner(home, provider, true).map(|created| created.expect("configure always writes"))
}

/// Create the provider's connector only when none is active, in the same locked
/// update that checks it. `None` means one exists and is left untouched: a
/// one-click "create" must never rotate a credential already in use.
pub fn create_provider_if_absent(home: &HomeLayout, provider: &str) -> io::Result<Option<Value>> {
    configure_inner(home, provider, false)
}

fn configure_inner(
    home: &HomeLayout,
    provider: &str,
    rotate_existing: bool,
) -> io::Result<Option<Value>> {
    if !matches!(provider, "chatgpt_web" | "grok_web") {
        return Err(error("invalid_web_model_provider"));
    }
    update(home, |root| {
        let old = &root["connectors"][provider];
        if !rotate_existing && old.is_object() && old["revoked"] != true {
            return Ok(None);
        }
        let now = cccc_contracts::utc_now();
        let mut connector = if old.is_object() && old["revoked"] != true {
            old.clone()
        } else {
            json!({"connector_id":format!("wmc_{}",Uuid::new_v4().simple()),"kind":"web_model_connector","routing_mode":if provider == "grok_web" {"credential"} else {"session"},"provider":provider,"routing_salt":Uuid::new_v4().to_string(),"created_at":now,"bindings":{},"pairings":{}})
        };
        let existed = old.is_object() && old["revoked"] != true;
        let generation = if existed {
            credential_generation(old)
                .checked_add(1)
                .ok_or_else(|| error("credential_generation_exhausted"))?
        } else {
            1
        };
        let secret = install_secret(&mut connector, &now);
        connector["credential_generation"] = json!(generation);
        if existed {
            record_rotation(&mut connector, "local:core", &now);
        }
        if provider == "grok_web" && !connector["routing_key"].is_string() {
            connector["routing_key"] = json!(format!(
                "{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            ));
        }
        root["connectors"][provider] = connector.clone();
        if provider == "chatgpt_web" {
            root["requires_reconfiguration"] = json!(false);
        }
        Ok(Some(
            json!({"outcome":if existed {"rotated"} else {"created"},"connector":connector,"secret":secret}),
        ))
    })
}

// Creation and explicit rotation use separate store paths.
const ROTATION_CONFIRMATION_DOMAIN: &str = "cccc-web-model-rotation-confirmation-v1";
const UNRECORDED_GENERATION: u64 = 0;
const EVENTS_LOG_RETAINED: usize = 10;
const REJECTION_WRITE_INTERVAL: Duration = Duration::from_secs(5);

fn credential_generation(c: &Value) -> u64 {
    c["credential_generation"]
        .as_u64()
        .unwrap_or(UNRECORDED_GENERATION)
}
fn active_provider<'a>(root: &'a Value, provider: &str) -> Option<&'a Value> {
    let c = &root["connectors"][provider];
    (c.is_object() && c["revoked"] != true).then_some(c)
}
fn require_provider(provider: &str) -> io::Result<()> {
    if matches!(provider, "chatgpt_web" | "grok_web") {
        Ok(())
    } else {
        Err(error("invalid_web_model_provider"))
    }
}
fn install_secret(connector: &mut Value, now: &str) -> String {
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
    secret
}

fn push_log(c: &mut Value, field: &str, entry: Value) {
    let mut log = c[field].as_array().cloned().unwrap_or_default();
    log.push(entry);
    log.drain(..log.len().saturating_sub(EVENTS_LOG_RETAINED));
    c[field] = Value::Array(log);
}
fn record_rotation(c: &mut Value, by: &str, now: &str) {
    c["secret_rotated_at"] = json!(now);
    c["rotated_by"] = json!(by);
    let preview = rotation_preview(c);
    // Existing diagnostics expect method/ok; a successful rotation remains success.
    push_log(
        c,
        "events_log",
        json!({"at":now,"method":"secret_rotated","ok":true,
        "reason":"secret_rotated","outcome":"ok","rotated_by":by,
        "credential_generation":preview["credential_generation"],
        "recorded_binding_count":preview["recorded_binding_count"]}),
    );
}
/// Non-authorizing fence over connector id, generation and sorted binding keys.
pub fn rotation_preview(connector: &Value) -> Value {
    let mut keys: Vec<&String> = connector["bindings"]
        .as_object()
        .map(|b| b.keys().collect())
        .unwrap_or_default();
    keys.sort();
    let recorded_binding_count = keys.len();
    let generation = credential_generation(connector);
    let confirmation = hash(
        &json!([
            ROTATION_CONFIRMATION_DOMAIN,
            connector["connector_id"],
            generation,
            keys
        ])
        .to_string(),
    );
    json!({
        "connector_id": connector["connector_id"],
        "credential_generation": generation,
        "recorded_binding_count": recorded_binding_count,
        "dots_binding_recorded": connector["dots"]["actor_id"].is_string(),
        "secret_rotated_at": connector["secret_rotated_at"],
        "rotated_by": connector["rotated_by"],
        "confirmation": confirmation
    })
}

/// Explicit rotation preserves routes, pairings and Dots state in one locked update.
pub fn rotate_provider(
    home: &HomeLayout,
    provider: &str,
    confirmation: &str,
    rotated_by: &str,
) -> io::Result<Value> {
    require_provider(provider)?;
    if rotated_by.trim().is_empty() {
        return Err(error("rotated_by_required"));
    }
    update(home, |root| {
        let live = active_provider(root, provider).ok_or_else(|| error("connector_unavailable"))?;
        let preview = rotation_preview(live);
        if preview["confirmation"] != confirmation {
            // `update` only writes on Ok, so a stale fence leaves the file untouched.
            return Err(error("rotation_confirmation_stale"));
        }
        let previous_generation = credential_generation(live);
        let generation = previous_generation
            .checked_add(1)
            .ok_or_else(|| error("credential_generation_exhausted"))?;
        let mut connector = live.clone();
        let now = cccc_contracts::utc_now();
        let secret = install_secret(&mut connector, &now);
        connector["credential_generation"] = json!(generation);
        record_rotation(&mut connector, rotated_by, &now);
        root["connectors"][provider] = connector.clone();
        Ok(
            json!({"outcome":"rotated","connector":connector,"secret":secret,
            "previous_generation":previous_generation}),
        )
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum RejectionRecord {
    Written { count: u64 },
    Deferred,
    UnknownConnector,
}

/// Process-local 5 s coalescing. Pending counts are lost on restart, so stored
/// counts are lower bounds; "at" is write-time, not last-seen time.
#[derive(Default)]
struct RejectionThrottle {
    entries: HashMap<(PathBuf, String), (Instant, u64)>,
}
impl RejectionThrottle {
    fn retain(&mut self, home: &HomeLayout, root: &Value) {
        self.entries.retain(|(path, id), _| {
            path.as_path() != home.root()
                || root["connectors"].as_object().is_some_and(|cs| {
                    cs.values()
                        .any(|c| c["connector_id"] == id.as_str() && c["revoked"] != true)
                })
        });
    }
    fn admit(&mut self, home: &HomeLayout, id: &str, now: Instant) -> Option<u64> {
        let key = (home.root().to_path_buf(), id.to_owned());
        match self.entries.get_mut(&key) {
            Some((last, pending)) => {
                *pending = pending.saturating_add(1);
                if now.saturating_duration_since(*last) < REJECTION_WRITE_INTERVAL {
                    return None;
                }
                *last = now;
                Some(std::mem::take(pending))
            }
            None => {
                self.entries.insert(key, (now, 0));
                Some(1)
            }
        }
    }
}
static REJECTIONS: OnceLock<Mutex<RejectionThrottle>> = OnceLock::new();
fn prune_rejections(home: &HomeLayout, root: &Value) {
    if let Some(m) = REJECTIONS.get() {
        m.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(home, root);
    }
}
/// Wrong-secret diagnostics never contain request material and never evict
/// rotation/call records: rejection_log has its own ten-entry bound.
pub fn record_rejected_secret(
    home: &HomeLayout,
    id: &str,
    transport: &str,
) -> io::Result<RejectionRecord> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let mut root = read(home)?;
        let count = {
            let mut throttle = REJECTIONS
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            throttle.retain(home, &root);
            if !root["connectors"].as_object().is_some_and(|cs| {
                cs.values()
                    .any(|c| c["connector_id"] == id && c["revoked"] != true)
            }) {
                return Ok(RejectionRecord::UnknownConnector);
            }
            match throttle.admit(home, id, Instant::now()) {
                Some(count) => count,
                None => return Ok(RejectionRecord::Deferred),
            }
        };
        let c = current(&mut root, id)?;
        let transport = match transport {
            "get_probe" | "post_header" | "post_path_token" => transport,
            _ => "other",
        };
        let generation = credential_generation(c);
        push_log(
            c,
            "rejection_log",
            json!({"at":cccc_contracts::utc_now(),
            "reason":"invalid_secret","outcome":"denied","transport":transport,
            "count":count,"credential_generation":generation}),
        );
        fs::write_secret_yaml(&store_path(home), &root)?;
        Ok(RejectionRecord::Written { count })
    })
}

pub fn revoke(home: &HomeLayout, id: &str) -> io::Result<bool> {
    update(home, |root| {
        let Some(connector) = root["connectors"]
            .as_object_mut()
            .expect("validated store")
            .values_mut()
            .find(|c| c["connector_id"] == id)
        else {
            return Ok(false);
        };
        connector["revoked"] = json!(true);
        connector["updated_at"] = json!(cccc_contracts::utc_now());
        connector["pairings"] = json!({});
        connector
            .as_object_mut()
            .expect("connector")
            .remove("grok_webhooks");
        connector.as_object_mut().expect("connector").remove("dots");
        Ok(true)
    })
}

pub fn secret_matches(item: &Value, supplied: &str) -> bool {
    !supplied.is_empty() && item["secret_hash"].as_str() == Some(hash(supplied).as_str())
}

/// Credential-scoped correlation, not authentication. Never use model arguments
/// or HTTP transport session IDs as a substitute for per-call host metadata.
pub fn session_key(connector: &Value, metadata: &Value) -> io::Result<String> {
    Ok(hash(
        &Value::Array(session_parts(connector, metadata)?).to_string(),
    ))
}

/// Diagnostic correlation only. Domain separation prevents these hashes from
/// substituting for a session key; each component stays scoped to this connector.
pub fn host_identity_fingerprints(connector: &Value, metadata: &Value) -> io::Result<Value> {
    let parts = session_parts(connector, metadata)?;
    let mut result = serde_json::Map::new();
    for (index, field) in ["session", "subject", "organization"]
        .into_iter()
        .enumerate()
    {
        let value = &parts[index + 3];
        result.insert(
            field.into(),
            if value.is_null() {
                Value::Null
            } else {
                json!(hash(
                    &json!([
                        "cccc-host-identity-diagnostic-v1",
                        parts[1],
                        parts[2],
                        field,
                        value
                    ])
                    .to_string()
                ))
            },
        );
    }
    Ok(Value::Object(result))
}

fn session_parts(connector: &Value, metadata: &Value) -> io::Result<Vec<Value>> {
    require_chatgpt(connector)?;
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
    Ok(parts)
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
    if generation.is_empty() || crate::actors::generation_identity(actor) != generation {
        return Err(error("paired_actor_identity_changed"));
    }
    if actor.runtime.web_model_provider() != connector["provider"].as_str()
        || actor.runner != cccc_contracts::RunnerKind::Headless
    {
        return Err(error("paired_actor_runtime_changed"));
    }
    if !actor.enabled {
        return Err(error("paired_actor_stopped"));
    }
    let current = binding_for_actor(&connector, group_id, actor_id, generation)
        .ok_or_else(|| error("conversation_not_paired"))?;
    if current["revision"] != expected["revision"] {
        return Err(error("conversation_pairing_changed"));
    }
    if actor.runtime == cccc_contracts::ActorRuntime::GrokWebModel {
        grok_webhook::validate(&connector, expected, &group)?;
    }
    if current["transport"] == "mcp_events" {
        if actor.runtime != cccc_contracts::ActorRuntime::ChatgptDots {
            return Err(error("dots_actor_unavailable"));
        }
        dots::validate_context(&connector, expected, &group)?;
    } else if actor.runtime == cccc_contracts::ActorRuntime::ChatgptDots {
        return Err(error("dots_binding_required"));
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
        require_chatgpt(c)?;
        if c["bindings"][route_key(group, actor)]["transport"] == "mcp_events" {
            return Err(error("dots_requires_event_binding"));
        }
        let (record, result) = pairing_candidate(group, actor, generation, automatic);
        // One record per Actor. Expired/failed attempts are retained until an
        // explicit retry or lifecycle removal, so a supervisor cannot resend.
        c["pairings"][route_key(group, actor)] = record;
        Ok(result)
    })
}

fn pairing_candidate(
    group: &str,
    actor: &str,
    generation: &str,
    automatic: bool,
) -> (Value, Value) {
    let code = format!(
        "wm_pair_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let operation = Uuid::new_v4().to_string();
    let expiry = now_ms() + PAIRING_TTL_MS;
    (
        json!({"pairing_id":operation,"group_id":group,"actor_id":actor,"generation":generation,
            "automatic":automatic,"code_hash":hash(&code),"expires_at_ms":expiry,"state":"waiting","session_key":null}),
        json!({"pairing_id":operation,"code":code,"expires_at_ms":expiry,"state":"waiting"}),
    )
}

pub fn accept_pairing(home: &HomeLayout, id: &str, code: &str, session: &str) -> io::Result<Value> {
    if code.is_empty() || session.is_empty() {
        return Err(error("pairing_required"));
    }
    update(home, |root| {
        let c = current(root, id)?;
        require_chatgpt(c)?;
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
        if let Some(pairs) = root["connectors"]["chatgpt_web"]["pairings"].as_object_mut() {
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
        require_chatgpt(c)?;
        let key = route_key(group, actor);
        if c["bindings"][&key]["transport"] == "mcp_events" {
            return Err(error("dots_requires_event_binding"));
        }
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
        let mut retired = Vec::new();
        for c in root["connectors"]
            .as_object_mut()
            .expect("validated store")
            .values_mut()
        {
            for collection in ["bindings", "pairings", "grok_webhooks"] {
                let keys = c[collection]
                    .as_object()
                    .into_iter()
                    .flat_map(|m| m.iter())
                    .filter(|(_, b)| {
                        b["group_id"] == group && actor.is_none_or(|a| b["actor_id"] == a)
                    })
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
            if c["dots"]["group_id"] == group
                && actor.is_none_or(|a| c["dots"]["actor_id"] == a)
                && let Some(value) = c.as_object_mut().expect("connector").remove("dots")
            {
                // Private in-memory saga state, never a public binding response.
                retired.push(json!({"kind":"web_model_dots_snapshot","connector_id":c["connector_id"],"value":value}));
            }
        }
        Ok(retired)
    })
}
/// Held by a Web process for one Actor's browser work, from its claim to its
/// last write, and by a runtime switch. Neither waits, so the work falls wholly
/// before or after the switch, whichever process each runs in.
pub fn browser_work_lock(home: &HomeLayout, group: &str, actor: &str) -> std::path::PathBuf {
    browser_work_locks(home, group).join(format!("{actor}.lock"))
}

fn browser_work_locks(home: &HomeLayout, group: &str) -> std::path::PathBuf {
    home.root().join("state/web_model_browser_work").join(group)
}

/// Best effort, once a Group is gone: nothing depends on these empty files.
pub fn remove_browser_work_locks(home: &HomeLayout, group: &str) {
    let _ = std::fs::remove_dir_all(browser_work_locks(home, group));
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
        for entry in entries {
            let c = current(root, entry["connector_id"].as_str().unwrap_or_default())?;
            if entry["kind"] == "web_model_dots_snapshot" {
                if c["provider"] != "chatgpt_web"
                    || !entry["value"].is_object()
                    || (!c["dots"].is_null() && c["dots"] != entry["value"])
                {
                    return Err(error("Dots route changed during rollback"));
                }
                c["dots"] = entry["value"].clone();
                if dots::state(c).is_none() {
                    return Err(error("Dots binding changed during rollback"));
                }
                continue;
            }
            if entry["kind"] != "web_model_route_snapshot"
                || entry["connector_id"] != c["connector_id"]
                || c["revoked"] == true
            {
                return Err(error("connector changed during route rollback"));
            }
            let collection = entry["collection"]
                .as_str()
                .filter(|v| matches!(*v, "bindings" | "pairings" | "grok_webhooks"))
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
        let Some(c) = root["connectors"]
            .as_object_mut()
            .expect("validated store")
            .values_mut()
            .find(|c| c["connector_id"] == id)
        else {
            return Ok(false);
        };
        change(c);
        Ok(true)
    })
}

fn require_chatgpt(connector: &Value) -> io::Result<()> {
    if connector["provider"] != "chatgpt_web" {
        return Err(error("invalid_web_model_provider"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
