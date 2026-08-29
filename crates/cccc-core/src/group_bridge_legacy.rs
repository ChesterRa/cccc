//! Canonical Group Bridge persistence plus one-way migration from the old store.
//!
//! The purpose-specific YAML files from 0.4.35 remain the durable authority.
//! `settings.yaml:group_bridge` is read only as a one-time migration source and
//! is cleared after a successful canonical write.

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io;

use crate::fs::{with_exclusive_lock, write_secret_yaml, write_yaml};
use crate::{HomeLayout, integration_state};

const LEGACY_STORE_KEY: &str = "group_bridge";
const LOCK_FILE: &str = "group_bridge_state.lock";
const IDENTITY_FILE: &str = "group_bridge_identity.yaml";
const PAIRING_FILE: &str = "group_bridge_pairing.yaml";
const REGISTRATIONS_FILE: &str = "group_bridge_registrations.yaml";
const CREDENTIALS_FILE: &str = "group_bridge_credentials.yaml";
const RECEIPTS_FILE: &str = "group_bridge_receipts.yaml";

/// Load canonical Group Bridge state.
pub fn load(home: &HomeLayout) -> io::Result<Value> {
    with_exclusive_lock(&home.root().join(LOCK_FILE), || {
        migrate_settings_unlocked(home)?;
        load_canonical_unlocked(home)
    })
}

/// Mutate the canonical Group Bridge state under one cross-process file lock.
pub fn update<T>(
    home: &HomeLayout,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> io::Result<T> {
    with_exclusive_lock(&home.root().join(LOCK_FILE), || {
        migrate_settings_unlocked(home)?;
        let mut value = load_canonical_unlocked(home)?;
        normalize(&mut value);
        let result = change(value.as_object_mut().expect("bridge store initialized"))?;
        persist_canonical_unlocked(home, &mut value)?;
        Ok(result)
    })
}

/// Compatibility entrypoint retained for callers compiled against the old module.
/// New code should call [`load`] or [`update`].
pub fn import_if_changed(home: &HomeLayout) -> io::Result<()> {
    with_exclusive_lock(&home.root().join(LOCK_FILE), || {
        migrate_settings_unlocked(home)
    })
}

fn migrate_settings_unlocked(home: &HomeLayout) -> io::Result<()> {
    let legacy = integration_state::global_get(home, LEGACY_STORE_KEY)?;
    let Some(legacy_root) = legacy.as_object() else {
        return Ok(());
    };
    if legacy_root.is_empty() {
        clear_legacy_store(home)?;
        return Ok(());
    }

    let mut canonical = load_canonical_unlocked(home)?;
    normalize(&mut canonical);
    let root = canonical
        .as_object_mut()
        .expect("canonical bridge store initialized");

    if !valid_identity(root.get("identity").unwrap_or(&Value::Null)) {
        if let Some(identity) = legacy_root
            .get("identity")
            .filter(|value| valid_identity(value))
        {
            root.insert("identity".into(), identity.clone());
        }
    }

    let denied_registrations = terminal_registration_ids(root);
    let denied_routes = terminal_routes(root);
    for (section, id_field) in [
        ("invites", "invite_id"),
        ("requests", "request_id"),
        ("trusts", "trust_id"),
        ("registrations", "registration_id"),
        ("outbounds", "outbound_id"),
        ("deliveries", "idempotency_key"),
    ] {
        merge_legacy_records(
            root,
            section,
            id_field,
            legacy_root.get(section),
            &denied_registrations,
            &denied_routes,
        );
    }

    // Canonical state wins every conflict.  Persist it first; only then retire
    // the old authority so a partial migration can be retried safely.
    persist_canonical_unlocked(home, &mut canonical)?;
    clear_legacy_store(home)
}

fn clear_legacy_store(home: &HomeLayout) -> io::Result<()> {
    integration_state::global_update(home, LEGACY_STORE_KEY, |value| {
        *value = Value::Null;
        Ok(())
    })
}

fn load_canonical_unlocked(home: &HomeLayout) -> io::Result<Value> {
    let identity = read_yaml_or_empty(home, IDENTITY_FILE)?;
    let pairing = read_yaml_or_empty(home, PAIRING_FILE)?;
    let registrations = read_yaml_or_empty(home, REGISTRATIONS_FILE)?;
    let credentials = read_yaml_or_empty(home, CREDENTIALS_FILE)?;
    let receipts = read_yaml_or_empty(home, RECEIPTS_FILE)?;

    let mut state = json!({
        "identity": if valid_identity(&identity) { identity } else { json!({}) },
        "invites": map_values(pairing.get("invites")),
        "requests": map_values(pairing.get("requests")),
        "trusts": map_values(pairing.get("trusts")),
        "registrations": map_values(registrations.get("registrations")),
        "outbounds": map_values(pairing.get("outbounds")),
        "deliveries": map_values(receipts.get("receipts")),
    });
    attach_credentials(
        state.as_object_mut().expect("bridge store initialized"),
        credentials.get("credentials"),
    );
    Ok(state)
}

fn persist_canonical_unlocked(home: &HomeLayout, state: &mut Value) -> io::Result<()> {
    normalize(state);
    let root = state
        .as_object_mut()
        .expect("canonical bridge store initialized");
    ensure_active_trust_registrations(root);
    let existing_credentials = read_yaml_or_empty(home, CREDENTIALS_FILE)?;
    let mut credentials = existing_credentials
        .get("credentials")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let active_inbound = persist_inbound_credentials(root, &mut credentials);
    let active_outbound = persist_outbound_credentials(root, &mut credentials);
    credentials.retain(|reference, record| match nonempty(record.get("kind")) {
        Some("remote_send") => active_inbound.contains(reference),
        Some("bearer") => active_outbound.contains(reference),
        _ => true,
    });
    clear_unresolved_credential_refs(root, &credentials);
    scrub_inline_secrets(root);

    if let Some(identity) = root.get("identity").filter(|value| valid_identity(value)) {
        write_yaml(&home.root().join(IDENTITY_FILE), identity)?;
    }
    write_yaml(
        &home.root().join(PAIRING_FILE),
        &json!({
            "invites": records_map(root.get("invites"), "invite_id"),
            "requests": records_map(root.get("requests"), "request_id"),
            "trusts": records_map(root.get("trusts"), "trust_id"),
            "outbounds": records_map(root.get("outbounds"), "outbound_id"),
        }),
    )?;
    write_yaml(
        &home.root().join(REGISTRATIONS_FILE),
        &json!({"registrations":records_map(root.get("registrations"), "registration_id")}),
    )?;
    write_yaml(
        &home.root().join(RECEIPTS_FILE),
        &json!({"receipts":receipt_map(root.get("deliveries"))}),
    )?;
    write_secret_yaml(
        &home.root().join(CREDENTIALS_FILE),
        &json!({"credentials":credentials}),
    )
}

fn persist_inbound_credentials(
    root: &mut Map<String, Value>,
    credentials: &mut Map<String, Value>,
) -> HashSet<String> {
    let mut active_references = HashSet::new();
    let active_trusts = root
        .get("trusts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|trust| trust["status"] == "active")
        .collect::<Vec<_>>();
    let active_requests = root
        .get("trusts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|trust| trust["status"] == "active")
        .filter_map(|trust| nonempty(trust.get("request_id")).map(str::to_owned))
        .collect::<HashSet<_>>();

    let registrations = root
        .get("registrations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let requests = root
        .entry("requests")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("bridge requests initialized");
    for request in requests {
        let request_id = nonempty(request.get("request_id")).unwrap_or("").to_owned();
        if request_id.is_empty() || !active_requests.contains(&request_id) {
            request
                .as_object_mut()
                .map(|item| item.remove("remote_send_credential_ref"));
            continue;
        }
        let registration_id = nonempty(request.get("registration_id")).unwrap_or("");
        let registration = registrations
            .iter()
            .find(|item| item["registration_id"] == registration_id);
        let existing_ref = nonempty(request.get("remote_send_credential_ref")).unwrap_or("");
        let token = registration
            .and_then(|item| nonempty(item.get("credential")))
            .or_else(|| existing_ref_token(credentials, existing_ref, "remote_send"));
        let Some(token) = token else {
            request
                .as_object_mut()
                .map(|item| item.remove("remote_send_credential_ref"));
            continue;
        };
        let group_id = nonempty(request.get("group_id")).unwrap_or("");
        let remote_group_id = nonempty(request.get("remote_group_id")).unwrap_or("");
        let remote_peer_id = nonempty(request.get("remote_peer_id")).unwrap_or("");
        if group_id.is_empty() || remote_group_id.is_empty() || remote_peer_id.is_empty() {
            continue;
        }
        let reference = if existing_ref.starts_with("fsec_remote_send_") {
            existing_ref.to_owned()
        } else {
            let material =
                format!("remote_send|{group_id}|{remote_group_id}|{remote_peer_id}|{request_id}");
            format!("fsec_remote_send_{}", short_digest(&material))
        };
        let created_at = credentials
            .get(&reference)
            .and_then(|item| item.get("created_at"))
            .cloned()
            .unwrap_or_else(|| request.get("created_at").cloned().unwrap_or(Value::Null));
        let updated_at = request.get("updated_at").cloned().unwrap_or(Value::Null);
        credentials.insert(
            reference.clone(),
            json!({
                "credential_ref":reference,
                "kind":"remote_send",
                "token":token,
                "registration_id":registration_id,
                "group_id":group_id,
                "remote_group_id":remote_group_id,
                "remote_peer_id":remote_peer_id,
                "request_id":request_id,
                "created_at":created_at,
                "updated_at":updated_at,
            }),
        );
        request["remote_send_credential_ref"] = json!(reference.clone());
        active_references.insert(reference);
    }

    // Older Rust releases stored the inbound bearer directly on a
    // registration and did not always retain a pairing request id. Preserve
    // that credential only while the matching trust remains active, and move
    // it into the canonical secret store during migration.
    for registration in registrations {
        let registration_id = nonempty(registration.get("registration_id")).unwrap_or("");
        let token = nonempty(registration.get("credential"));
        let Some(trust) = active_trusts.iter().find(|trust| {
            !registration_id.is_empty() && trust["registration_id"] == registration_id
        }) else {
            continue;
        };
        let Some(token) = token else { continue };
        let group_id = nonempty(trust.get("group_id"))
            .or_else(|| nonempty(registration.get("group_id")))
            .unwrap_or("");
        let remote_group_id = nonempty(trust.get("remote_group_id"))
            .or_else(|| nonempty(registration.get("remote_group_id")))
            .unwrap_or("");
        let remote_peer_id = nonempty(trust.get("remote_peer_id"))
            .or_else(|| nonempty(registration.get("remote_peer_id")))
            .unwrap_or("");
        if group_id.is_empty() || remote_group_id.is_empty() || remote_peer_id.is_empty() {
            continue;
        }
        let request_id = nonempty(trust.get("request_id")).unwrap_or("");
        let material = format!(
            "remote_send|{group_id}|{remote_group_id}|{remote_peer_id}|{}",
            if request_id.is_empty() {
                registration_id
            } else {
                request_id
            }
        );
        let reference = format!("fsec_remote_send_{}", short_digest(&material));
        let created_at = credentials
            .get(&reference)
            .and_then(|item| item.get("created_at"))
            .cloned()
            .unwrap_or_else(|| {
                registration
                    .get("created_at")
                    .cloned()
                    .unwrap_or(Value::Null)
            });
        credentials.insert(
            reference.clone(),
            json!({
                "credential_ref":reference,
                "kind":"remote_send",
                "token":token,
                "registration_id":registration_id,
                "group_id":group_id,
                "remote_group_id":remote_group_id,
                "remote_peer_id":remote_peer_id,
                "request_id":request_id,
                "created_at":created_at,
                "updated_at":registration.get("updated_at").cloned().unwrap_or(Value::Null),
            }),
        );
        active_references.insert(reference);
    }
    active_references
}

fn persist_outbound_credentials(
    root: &mut Map<String, Value>,
    credentials: &mut Map<String, Value>,
) -> HashSet<String> {
    let existing = credentials.clone();
    let registrations = root
        .get("registrations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let denied_registration_ids = terminal_registration_ids(root);
    let denied_route_ids = terminal_routes(root);
    let mut active_references = registrations
        .iter()
        .filter(|item| {
            item["status"] == "active"
                && nonempty(item.get("registration_id"))
                    .is_none_or(|id| !denied_registration_ids.contains(id))
                && item
                    .as_object()
                    .and_then(route_identity)
                    .as_ref()
                    .is_none_or(|route| !denied_route_ids.contains(route))
        })
        .filter_map(|item| nonempty(item.get("credential_ref")))
        .filter(|reference| {
            existing
                .get(*reference)
                .is_some_and(|record| record["kind"] == "bearer")
        })
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let outbound_tokens = root
        .get("outbounds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let token = nonempty(item.get("credential")).or_else(|| {
                item.get("remote_request")
                    .and_then(|value| nonempty(value.get("remote_send_token")))
            })?;
            Some((
                nonempty(item.get("local_group_id"))
                    .unwrap_or("")
                    .to_owned(),
                nonempty(item.get("issuer_group_id"))
                    .unwrap_or("")
                    .to_owned(),
                token.to_owned(),
            ))
        })
        .collect::<Vec<_>>();
    let mut registration_refs = Vec::new();
    let trusts = root
        .entry("trusts")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("bridge trusts initialized");
    for trust in trusts
        .iter_mut()
        .filter(|trust| trust["status"] == "active")
    {
        let local_group_id = nonempty(trust.get("group_id")).unwrap_or("");
        let remote_group_id = nonempty(trust.get("remote_group_id")).unwrap_or("");
        let endpoint = nonempty(trust.get("remote_endpoint")).unwrap_or("");
        let registration_id = nonempty(trust.get("registration_id")).unwrap_or("");
        let existing_ref = registrations
            .iter()
            .find(|registration| registration["registration_id"] == registration_id)
            .and_then(|registration| nonempty(registration.get("credential_ref")))
            .unwrap_or("");
        let token = nonempty(trust.get("credential"))
            .map(str::to_owned)
            .or_else(|| {
                outbound_tokens
                    .iter()
                    .find(|(local, remote, _)| local == local_group_id && remote == remote_group_id)
                    .map(|(_, _, token)| token.clone())
            })
            .or_else(|| existing_ref_token(&existing, existing_ref, "bearer").map(str::to_owned))
            .or_else(|| existing_bearer_token(&existing, local_group_id, remote_group_id));
        let Some(token) = token else { continue };
        if local_group_id.is_empty() || remote_group_id.is_empty() {
            continue;
        }
        let reference = if existing_ref.starts_with("fsec_pairing_") {
            existing_ref.to_owned()
        } else {
            let token_digest = format!("{:x}", Sha256::digest(token.as_bytes()));
            let material = format!("{local_group_id}|{remote_group_id}|{endpoint}|{token_digest}");
            format!("fsec_pairing_{}", short_digest(&material))
        };
        let created_at = existing
            .get(&reference)
            .and_then(|item| item.get("created_at"))
            .cloned()
            .unwrap_or_else(|| trust.get("created_at").cloned().unwrap_or(Value::Null));
        let updated_at = trust.get("updated_at").cloned().unwrap_or(Value::Null);
        credentials.insert(
            reference.clone(),
            json!({
                "credential_ref":reference,
                "kind":"bearer",
                "token":token,
                "local_group_id":local_group_id,
                "remote_group_id":remote_group_id,
                "remote_endpoint":endpoint,
                "created_at":created_at,
                "updated_at":updated_at,
            }),
        );
        active_references.insert(reference.clone());
        if !registration_id.is_empty() {
            registration_refs.push((registration_id.to_owned(), reference));
        }
    }
    for (registration_id, reference) in registration_refs {
        if let Some(registration) = root
            .get_mut("registrations")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .find(|item| item["registration_id"] == registration_id)
        {
            registration["credential_ref"] = json!(reference);
        }
    }
    active_references
}

fn scrub_inline_secrets(root: &mut Map<String, Value>) {
    for section in ["registrations", "trusts", "outbounds"] {
        for item in root
            .get_mut(section)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if let Some(record) = item.as_object_mut() {
                record.remove("credential");
                if let Some(remote_request) = record
                    .get_mut("remote_request")
                    .and_then(Value::as_object_mut)
                {
                    remote_request.remove("remote_send_token");
                    remote_request.remove("credential");
                }
            }
        }
    }
}

fn ensure_active_trust_registrations(root: &mut Map<String, Value>) {
    let existing_ids = root
        .get("registrations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| nonempty(item.get("registration_id")).map(str::to_owned))
        .collect::<HashSet<_>>();
    let missing = root
        .get("trusts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|trust| trust["status"] == "active")
        .filter_map(|trust| {
            let registration_id = nonempty(trust.get("registration_id"))?;
            if existing_ids.contains(registration_id) {
                return None;
            }
            let group_id = nonempty(trust.get("group_id"))?;
            let remote_group_id = nonempty(trust.get("remote_group_id"))?;
            let remote_peer_id = nonempty(trust.get("remote_peer_id")).unwrap_or("");
            let endpoint = nonempty(trust.get("remote_endpoint"))
                .map(str::to_owned)
                .unwrap_or_else(|| format!("group-bridge-session://{remote_peer_id}"));
            Some(json!({
                "registration_id":registration_id,
                "group_id":group_id,
                "url":endpoint,
                "transport":nonempty(trust.get("transport")).unwrap_or("group_bridge_session"),
                "remote_group_id":remote_group_id,
                "remote_peer_id":remote_peer_id,
                "multiaddrs":trust.get("multiaddrs").cloned().unwrap_or_else(|| json!([])),
                "status":"active",
                "created_at":trust.get("created_at").cloned().unwrap_or(Value::Null),
                "updated_at":trust.get("updated_at").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect::<Vec<_>>();
    records_mut(root, "registrations").extend(missing);
}

fn clear_unresolved_credential_refs(
    root: &mut Map<String, Value>,
    credentials: &Map<String, Value>,
) {
    for registration in records_mut(root, "registrations") {
        let reference = nonempty(registration.get("credential_ref"))
            .unwrap_or("")
            .to_owned();
        if !reference.is_empty() && !credentials.contains_key(&reference) {
            registration
                .as_object_mut()
                .map(|record| record.remove("credential_ref"));
        }
    }
}

fn attach_credentials(state: &mut Map<String, Value>, raw: Option<&Value>) {
    let credentials = raw
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(reference, item)| item.as_object().map(|record| (reference.as_str(), record)))
        .collect::<Vec<_>>();
    for (reference, credential) in &credentials {
        let Some(token) = nonempty(credential.get("token")) else {
            continue;
        };
        match nonempty(credential.get("kind")) {
            Some("remote_send") => attach_inbound_credential(state, credential, token),
            Some("bearer") => attach_outbound_credential(state, reference, credential, token),
            _ => {}
        }
    }
    attach_outbound_tokens(state);
}

fn attach_inbound_credential(
    state: &mut Map<String, Value>,
    credential: &Map<String, Value>,
    token: &str,
) {
    let request_id = nonempty(credential.get("request_id")).unwrap_or("");
    let registration_id = nonempty(credential.get("registration_id"))
        .map(str::to_owned)
        .or_else(|| {
            state
                .get("requests")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|request| request["request_id"] == request_id)
                .and_then(|request| request["registration_id"].as_str())
                .map(str::to_owned)
        })
        .unwrap_or_default();
    if let Some(registration) = records_mut(state, "registrations")
        .iter_mut()
        .find(|item| item["registration_id"] == registration_id)
    {
        registration["credential"] = json!(token);
    }
}

fn attach_outbound_credential(
    state: &mut Map<String, Value>,
    reference: &str,
    credential: &Map<String, Value>,
    token: &str,
) {
    let registration_ids = state
        .get("registrations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|registration| nonempty(registration.get("credential_ref")) == Some(reference))
        .filter_map(|registration| nonempty(registration.get("registration_id")).map(str::to_owned))
        .collect::<HashSet<_>>();
    if !registration_ids.is_empty() {
        for trust in records_mut(state, "trusts").iter_mut().filter(|item| {
            nonempty(item.get("registration_id")).is_some_and(|id| registration_ids.contains(id))
        }) {
            trust["credential"] = json!(token);
        }
        return;
    }
    let local_group = nonempty(credential.get("local_group_id")).unwrap_or("");
    let remote_group = nonempty(credential.get("remote_group_id")).unwrap_or("");
    let endpoint = nonempty(credential.get("remote_endpoint")).unwrap_or("");
    if let Some(trust) = records_mut(state, "trusts").iter_mut().find(|item| {
        item["group_id"] == local_group
            && item["remote_group_id"] == remote_group
            && (endpoint.is_empty() || item["remote_endpoint"] == endpoint)
    }) {
        trust["credential"] = json!(token);
    }
}

fn attach_outbound_tokens(state: &mut Map<String, Value>) {
    let tokens = state
        .get("outbounds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let token = item["remote_request"]["remote_send_token"].as_str()?;
            Some((
                item["local_group_id"].as_str()?.to_owned(),
                item["issuer_group_id"].as_str()?.to_owned(),
                token.to_owned(),
            ))
        })
        .collect::<Vec<_>>();
    for (local_group, remote_group, token) in tokens {
        if let Some(trust) = records_mut(state, "trusts")
            .iter_mut()
            .find(|item| item["group_id"] == local_group && item["remote_group_id"] == remote_group)
        {
            trust["credential"] = json!(token);
        }
    }
}

fn merge_legacy_records(
    state: &mut Map<String, Value>,
    section: &str,
    id_field: &str,
    legacy: Option<&Value>,
    denied_registrations: &HashSet<String>,
    denied_routes: &HashSet<(String, String, String)>,
) {
    let Some(legacy) = legacy.and_then(Value::as_array) else {
        return;
    };
    for item in legacy {
        let Some(item) = item.as_object() else {
            continue;
        };
        let id = nonempty(item.get(id_field)).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        if section == "registrations" {
            let route = route_identity(item);
            if denied_registrations.contains(id)
                || route
                    .as_ref()
                    .is_some_and(|route| denied_routes.contains(route))
            {
                continue;
            }
        }
        let records = records_mut(state, section);
        if records.iter().any(|record| record[id_field] == id) {
            continue;
        }
        if section == "trusts" && terminal_trust_conflicts(records, item) {
            continue;
        }
        if section == "deliveries"
            && records.iter().any(|record| {
                record.get("registration_id") == item.get("registration_id")
                    && record.get("idempotency_key") == item.get("idempotency_key")
            })
        {
            continue;
        }
        records.push(Value::Object(item.clone()));
    }
}

fn terminal_trust_conflicts(records: &[Value], candidate: &Map<String, Value>) -> bool {
    records.iter().any(|record| {
        is_terminal(record.get("status"))
            && ["request_id", "registration_id"]
                .into_iter()
                .any(|field| same_nonempty(record.get(field), candidate.get(field)))
            || is_terminal(record.get("status"))
                && ["group_id", "remote_group_id", "remote_peer_id"]
                    .into_iter()
                    .all(|field| same_nonempty(record.get(field), candidate.get(field)))
    })
}

fn terminal_registration_ids(root: &Map<String, Value>) -> HashSet<String> {
    root.get("trusts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|trust| is_terminal(trust.get("status")))
        .filter_map(|trust| nonempty(trust.get("registration_id")).map(str::to_owned))
        .collect()
}

fn terminal_routes(root: &Map<String, Value>) -> HashSet<(String, String, String)> {
    root.get("trusts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|trust| is_terminal(trust.get("status")))
        .filter_map(|trust| trust.as_object().and_then(route_identity))
        .collect()
}

fn route_identity(record: &Map<String, Value>) -> Option<(String, String, String)> {
    Some((
        nonempty(record.get("group_id"))?.to_owned(),
        nonempty(record.get("remote_group_id"))?.to_owned(),
        nonempty(record.get("remote_peer_id"))?.to_owned(),
    ))
}

fn is_terminal(value: Option<&Value>) -> bool {
    matches!(
        nonempty(value),
        Some("revoked") | Some("rejected") | Some("expired") | Some("disabled")
    )
}

fn same_nonempty(left: Option<&Value>, right: Option<&Value>) -> bool {
    matches!((nonempty(left), nonempty(right)), (Some(left), Some(right)) if left == right)
}

fn read_yaml_or_empty(home: &HomeLayout, name: &str) -> io::Result<Value> {
    let path = home.root().join(name);
    match std::fs::read(&path) {
        Ok(raw) => serde_yaml::from_slice(&raw).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(error),
    }
}

fn map_values(value: Option<&Value>) -> Value {
    Value::Array(
        value
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(_, item)| item.as_object().map(|_| item.clone()))
            .collect(),
    )
}

fn records_map(value: Option<&Value>, id_field: &str) -> Map<String, Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| nonempty(item.get(id_field)).map(|id| (id.to_owned(), item.clone())))
        .collect()
}

fn receipt_map(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let registration_id = nonempty(item.get("registration_id"))?;
            let idempotency_key = nonempty(item.get("idempotency_key"))?;
            Some((
                format!("{registration_id}::{idempotency_key}"),
                item.clone(),
            ))
        })
        .collect()
}

fn existing_ref_token<'a>(
    credentials: &'a Map<String, Value>,
    reference: &str,
    kind: &str,
) -> Option<&'a str> {
    let record = credentials.get(reference)?;
    (record["kind"] == kind)
        .then(|| nonempty(record.get("token")))
        .flatten()
}

fn existing_bearer_token(
    credentials: &Map<String, Value>,
    local_group_id: &str,
    remote_group_id: &str,
) -> Option<String> {
    credentials.values().find_map(|record| {
        (record["kind"] == "bearer"
            && record["local_group_id"] == local_group_id
            && record["remote_group_id"] == remote_group_id)
            .then(|| nonempty(record.get("token")).map(str::to_owned))
            .flatten()
    })
}

fn short_digest(material: &str) -> String {
    format!("{:x}", Sha256::digest(material.as_bytes()))[..24].to_owned()
}

fn valid_identity(value: &Value) -> bool {
    value["node_id"]
        .as_str()
        .is_some_and(|item| !item.trim().is_empty())
        && value["peer_id"]
            .as_str()
            .is_some_and(|item| !item.trim().is_empty())
}

fn records_mut<'a>(state: &'a mut Map<String, Value>, section: &str) -> &'a mut Vec<Value> {
    state
        .entry(section)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("bridge section initialized")
}

fn nonempty(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn normalize(value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
    let state = value.as_object_mut().expect("bridge store initialized");
    for key in [
        "invites",
        "requests",
        "trusts",
        "registrations",
        "outbounds",
        "deliveries",
    ] {
        if !state.get(key).is_some_and(Value::is_array) {
            state.insert(key.into(), json!([]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, HomeLayout) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        (temp, home)
    }

    #[test]
    fn canonical_revocation_wins_over_legacy_active_state() {
        let (_temp, home) = home();
        write_yaml(
            &home.root().join(PAIRING_FILE),
            &json!({"trusts":{"ptrust_1":{
                "trust_id":"ptrust_1","registration_id":"reg_1",
                "group_id":"g_local","remote_group_id":"g_remote",
                "remote_peer_id":"peer_remote","status":"revoked"
            }}}),
        )
        .expect("canonical trust");
        integration_state::global_update(&home, LEGACY_STORE_KEY, |value| {
            *value = json!({
                "trusts":[{"trust_id":"ptrust_1","registration_id":"reg_1",
                    "group_id":"g_local","remote_group_id":"g_remote",
                    "remote_peer_id":"peer_remote","status":"active"}],
                "registrations":[
                    {"registration_id":"reg_1","group_id":"g_local","remote_group_id":"g_remote",
                        "remote_peer_id":"peer_remote","status":"active"},
                    {"registration_id":"reg_alias","group_id":"g_local","remote_group_id":"g_remote",
                        "remote_peer_id":"peer_remote","status":"active"}
                ]
            });
            Ok(())
        })
        .expect("legacy state");

        let state = load(&home).expect("canonical load");
        assert_eq!(state["trusts"][0]["status"], "revoked");
        assert!(state["registrations"].as_array().is_some_and(Vec::is_empty));
        assert!(
            integration_state::global_get(&home, LEGACY_STORE_KEY)
                .expect("legacy retired")
                .is_null()
        );
    }

    #[test]
    fn updates_python_compatible_receipts_and_credentials() {
        let (_temp, home) = home();
        update(&home, |state| {
            state.insert("requests".into(), json!([{
                "request_id":"preq_1","registration_id":"reg_1","group_id":"g_local",
                "remote_group_id":"g_remote","remote_peer_id":"peer_remote","status":"approved"
            }]));
            state.insert("trusts".into(), json!([{
                "trust_id":"ptrust_1","request_id":"preq_1","registration_id":"reg_1",
                "group_id":"g_local","remote_group_id":"g_remote","remote_peer_id":"peer_remote",
                "status":"active"
            }]));
            state.insert("registrations".into(), json!([{
                "registration_id":"reg_1","group_id":"g_local","credential":"secret","status":"active"
            }]));
            state.insert("deliveries".into(), json!([{
                "registration_id":"reg_1","idempotency_key":"once","status":"sent"
            }]));
            Ok(())
        })
        .expect("update");

        let pairing = read_yaml_or_empty(&home, PAIRING_FILE).expect("pairing");
        let registrations = read_yaml_or_empty(&home, REGISTRATIONS_FILE).expect("registrations");
        let credentials = read_yaml_or_empty(&home, CREDENTIALS_FILE).expect("credentials");
        let receipts = read_yaml_or_empty(&home, RECEIPTS_FILE).expect("receipts");
        assert!(
            pairing["requests"]["preq_1"]["remote_send_credential_ref"]
                .as_str()
                .is_some_and(|value| value.starts_with("fsec_remote_send_"))
        );
        assert!(registrations["registrations"]["reg_1"]["credential"].is_null());
        assert_eq!(
            credentials["credentials"].as_object().map(Map::len),
            Some(1)
        );
        assert_eq!(receipts["receipts"]["reg_1::once"]["status"], "sent");
        assert_eq!(
            load(&home).expect("reload")["registrations"][0]["credential"],
            "secret"
        );
    }

    #[test]
    fn legacy_registration_credential_survives_migration_but_not_revocation() {
        let (_temp, home) = home();
        integration_state::global_update(&home, LEGACY_STORE_KEY, |value| {
            *value = json!({
                "registrations":[{
                    "registration_id":"reg_legacy","group_id":"g_local",
                    "remote_group_id":"g_remote","remote_peer_id":"peer_remote",
                    "credential":"legacy-secret","status":"active"
                }],
                "trusts":[{
                    "trust_id":"trust_legacy","registration_id":"reg_legacy",
                    "group_id":"g_local","remote_group_id":"g_remote",
                    "remote_peer_id":"peer_remote","status":"active"
                }]
            });
            Ok(())
        })
        .expect("legacy state");

        assert_eq!(
            load(&home).expect("migrated")["registrations"][0]["credential"],
            "legacy-secret"
        );
        update(&home, |state| {
            state["trusts"][0]["status"] = json!("revoked");
            Ok(())
        })
        .expect("revoke");
        assert!(load(&home).expect("revoked")["registrations"][0]["credential"].is_null());
        let credentials = read_yaml_or_empty(&home, CREDENTIALS_FILE).expect("credentials");
        assert!(
            credentials["credentials"]
                .as_object()
                .is_some_and(Map::is_empty)
        );
    }

    #[test]
    fn legacy_outbound_trust_becomes_python_routable_and_revocation_removes_bearer() {
        let (_temp, home) = home();
        integration_state::global_update(&home, LEGACY_STORE_KEY, |value| {
            *value = json!({
                "outbounds":[{
                    "outbound_id":"pout_legacy","local_group_id":"g_local",
                    "issuer_group_id":"g_remote","issuer_peer_id":"peer_remote",
                    "issuer_endpoint":"https://remote.example","credential":"outbound-secret",
                    "status":"approved"
                }],
                "trusts":[{
                    "trust_id":"trust_outbound","registration_id":"reg_remote",
                    "group_id":"g_local","remote_group_id":"g_remote",
                    "remote_peer_id":"peer_remote","remote_endpoint":"https://remote.example",
                    "transport":"group_bridge_session","credential":"outbound-secret",
                    "status":"active"
                }]
            });
            Ok(())
        })
        .expect("legacy state");

        let migrated = load(&home).expect("migrated");
        assert_eq!(migrated["trusts"][0]["credential"], "outbound-secret");
        let registrations = read_yaml_or_empty(&home, REGISTRATIONS_FILE).expect("registrations");
        let registration = &registrations["registrations"]["reg_remote"];
        assert_eq!(registration["url"], "https://remote.example");
        let reference = registration["credential_ref"]
            .as_str()
            .expect("credential ref")
            .to_owned();
        assert!(reference.starts_with("fsec_pairing_"));
        let credentials = read_yaml_or_empty(&home, CREDENTIALS_FILE).expect("credentials");
        assert_eq!(credentials["credentials"][&reference]["kind"], "bearer");
        assert_eq!(
            credentials["credentials"][&reference]["token"],
            "outbound-secret"
        );

        update(&home, |state| {
            state["trusts"][0]["status"] = json!("revoked");
            Ok(())
        })
        .expect("revoke");
        let credentials = read_yaml_or_empty(&home, CREDENTIALS_FILE).expect("credentials");
        assert!(
            credentials["credentials"]
                .as_object()
                .is_some_and(Map::is_empty)
        );
        let registrations = read_yaml_or_empty(&home, REGISTRATIONS_FILE).expect("registrations");
        assert!(registrations["registrations"]["reg_remote"]["credential_ref"].is_null());
    }
}
