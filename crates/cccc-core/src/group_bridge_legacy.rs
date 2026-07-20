use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{HomeLayout, integration_state};

const STORE_KEY: &str = "group_bridge";

const LEGACY_FILES: [&str; 4] = [
    "group_bridge_identity.yaml",
    "group_bridge_pairing.yaml",
    "group_bridge_registrations.yaml",
    "group_bridge_credentials.yaml",
];

pub fn import_if_changed(home: &HomeLayout) -> std::io::Result<()> {
    let revision = revision(home);
    let current = integration_state::global_get(home, STORE_KEY)?;
    if current["legacy_import"]["revision"].as_str() == Some(&revision) {
        return Ok(());
    }
    integration_state::global_update(home, STORE_KEY, |value| {
        normalize(value);
        merge(home, value, &revision);
        Ok(())
    })
}

fn revision(home: &HomeLayout) -> String {
    let mut digest = Sha256::new();
    for name in LEGACY_FILES {
        let Ok(raw) = std::fs::read(home.root().join(name)) else {
            continue;
        };
        digest.update(name.as_bytes());
        digest.update((raw.len() as u64).to_le_bytes());
        digest.update(&raw);
    }
    format!("{:x}", digest.finalize())
}

fn merge(home: &HomeLayout, state: &mut Value, revision: &str) {
    let state = state.as_object_mut().expect("bridge store initialized");
    let identity = read_yaml(home, "group_bridge_identity.yaml");
    if valid_identity(&identity) {
        state.insert("identity".into(), identity);
    }

    let pairing = read_yaml(home, "group_bridge_pairing.yaml");
    for (section, id_field) in [
        ("invites", "invite_id"),
        ("requests", "request_id"),
        ("trusts", "trust_id"),
        ("outbounds", "outbound_id"),
    ] {
        merge_records(state, section, id_field, pairing.get(section));
    }
    let registrations = read_yaml(home, "group_bridge_registrations.yaml");
    merge_records(
        state,
        "registrations",
        "registration_id",
        registrations.get("registrations"),
    );

    let credentials = read_yaml(home, "group_bridge_credentials.yaml");
    attach_legacy_credentials(state, credentials.get("credentials"));
    state.insert(
        "legacy_import".into(),
        json!({"version":1,"revision":revision}),
    );
}

fn read_yaml(home: &HomeLayout, name: &str) -> Value {
    std::fs::read(home.root().join(name))
        .ok()
        .and_then(|raw| serde_yaml::from_slice::<Value>(&raw).ok())
        .unwrap_or_else(|| json!({}))
}

fn valid_identity(value: &Value) -> bool {
    value["node_id"]
        .as_str()
        .is_some_and(|item| !item.trim().is_empty())
        && value["peer_id"]
            .as_str()
            .is_some_and(|item| !item.trim().is_empty())
}

fn merge_records(
    state: &mut Map<String, Value>,
    section: &str,
    id_field: &str,
    legacy: Option<&Value>,
) {
    let Some(legacy) = legacy.and_then(Value::as_object) else {
        return;
    };
    let records = state
        .entry(section)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("bridge section initialized");
    for (legacy_id, item) in legacy {
        let Some(mut item) = item.as_object().cloned() else {
            continue;
        };
        let id = item
            .get(id_field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(legacy_id)
            .to_owned();
        if records.iter().any(|record| record[id_field] == id) {
            continue;
        }
        item.insert(id_field.into(), json!(id));
        records.push(Value::Object(item));
    }
}

fn attach_legacy_credentials(state: &mut Map<String, Value>, legacy: Option<&Value>) {
    let credentials = legacy
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(_, item)| item.as_object())
        .collect::<Vec<_>>();

    for credential in &credentials {
        let Some(token) = nonempty(credential.get("token")) else {
            continue;
        };
        match nonempty(credential.get("kind")) {
            Some("remote_send") => attach_inbound_credential(state, credential, token),
            Some("bearer") => attach_outbound_credential(state, credential, token),
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
    let registration_id = state
        .get("requests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|request| request["request_id"] == request_id)
        .and_then(|request| request["registration_id"].as_str())
        .unwrap_or("")
        .to_owned();
    if registration_id.is_empty() {
        return;
    }
    if let Some(registration) = records_mut(state, "registrations")
        .iter_mut()
        .find(|item| item["registration_id"] == registration_id)
    {
        registration["credential"] = json!(token);
    }
}

fn attach_outbound_credential(
    state: &mut Map<String, Value>,
    credential: &Map<String, Value>,
    token: &str,
) {
    let local_group = nonempty(credential.get("local_group_id")).unwrap_or("");
    let remote_group = nonempty(credential.get("remote_group_id")).unwrap_or("");
    if let Some(trust) = records_mut(state, "trusts")
        .iter_mut()
        .find(|item| item["group_id"] == local_group && item["remote_group_id"] == remote_group)
    {
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
        state.entry(key).or_insert_with(|| json!([]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration_state;

    #[test]
    fn imports_legacy_identity_records_and_credentials_idempotently() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("group_bridge_identity.yaml"),
            "node_id: node_legacy\npeer_id: 12D3KooLegacy\n",
        )
        .expect("identity");
        std::fs::write(
            home.root().join("group_bridge_pairing.yaml"),
            concat!(
                "invites: {}\nrequests:\n  preq_1:\n    request_id: preq_1\n    registration_id: reg_1\n",
                "trusts:\n  ptrust_1:\n    trust_id: ptrust_1\n    group_id: g_local\n    remote_group_id: g_remote\n",
                "outbounds:\n  pout_1:\n    outbound_id: pout_1\n    local_group_id: g_local\n    issuer_group_id: g_remote\n",
                "    remote_request:\n      request_id: preq_remote\n      remote_send_token: outbound-token\n",
            ),
        )
        .expect("pairing");
        std::fs::write(
            home.root().join("group_bridge_registrations.yaml"),
            "registrations:\n  reg_1:\n    registration_id: reg_1\n    group_id: g_local\n",
        )
        .expect("registrations");
        std::fs::write(
            home.root().join("group_bridge_credentials.yaml"),
            concat!(
                "credentials:\n  inbound:\n    kind: remote_send\n    request_id: preq_1\n    token: inbound-token\n",
                "  outbound:\n    kind: bearer\n    local_group_id: g_local\n    remote_group_id: g_remote\n    token: outbound-token\n",
            ),
        )
        .expect("credentials");
        integration_state::global_update(&home, "group_bridge", |value| {
            *value = json!({"identity":{"node_id":"node_new","peer_id":"peer_new"}});
            Ok(())
        })
        .expect("state");

        import_if_changed(&home).expect("import");
        let first = integration_state::global_get(&home, STORE_KEY).expect("load");
        import_if_changed(&home).expect("reimport");
        let second = integration_state::global_get(&home, STORE_KEY).expect("reload");
        assert_eq!(first["identity"]["peer_id"], "12D3KooLegacy");
        assert_eq!(first["registrations"][0]["credential"], "inbound-token");
        assert_eq!(first["trusts"][0]["credential"], "outbound-token");
        assert_eq!(first["outbounds"].as_array().map(Vec::len), Some(1));
        assert_eq!(second["outbounds"].as_array().map(Vec::len), Some(1));
        assert!(first["legacy_import"]["revision"].is_string());
    }
}
