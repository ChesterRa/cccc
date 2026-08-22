use cccc_contracts::{DaemonRequest, Event};
use cccc_core::group::AUTOMATION_TIMING_KEYS;
use cccc_core::ledger;
use cccc_core::permissions;
use cccc_core::settings;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

const SECTION_SETTING_KEYS: &[(&str, &str, &str)] = &[
    ("default_send_to", "messaging", "default_send_to"),
    ("min_interval_seconds", "delivery", "min_interval_seconds"),
    (
        "mail_notice_after_seconds",
        "delivery",
        "mail_notice_after_seconds",
    ),
    (
        "reply_notice_after_seconds",
        "delivery",
        "reply_notice_after_seconds",
    ),
    (
        "terminal_transcript_visibility",
        "terminal_transcript",
        "visibility",
    ),
    (
        "terminal_transcript_notify_tail",
        "terminal_transcript",
        "notify_tail",
    ),
    (
        "terminal_transcript_notify_lines",
        "terminal_transcript",
        "notify_lines",
    ),
    ("panorama_enabled", "features", "panorama_enabled"),
];

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_settings_update" => group_settings(home, request),
        "observability_get" => global_get(home, "observability"),
        "observability_update" => global_update(home, request, "observability"),
        "branding_get" => global_get(home, "branding"),
        "branding_update" => global_update(home, request, "branding"),
        _ => return None,
    })
}

fn group_settings(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load_group(home, request)?;
    authorize(&group, request)?;
    let mut patch = patch(request)?;
    patch.remove("by");
    let settings_value = store(home)?
        .mutate(&group.group_id, |doc| {
            let mut flat = match doc.extra.remove("settings") {
                Some(Value::Object(settings)) => settings,
                Some(_) => return Err(std::io::Error::other("invalid group settings")),
                None => Map::new(),
            };

            promote_legacy_group_settings(doc, &mut flat);

            let mut automation_patch = Map::new();
            let mut flat_patch = Map::new();
            for (key, value) in &patch {
                if AUTOMATION_TIMING_KEYS.contains(&key.as_str()) {
                    automation_patch.insert(key.clone(), value.clone());
                } else if let Some((section, canonical)) = section_target(key) {
                    set_section_value(doc, section, canonical, value.clone());
                } else {
                    flat_patch.insert(key.clone(), value.clone());
                }
            }
            settings::merge(&mut doc.automation, &automation_patch);
            settings::merge(&mut flat, &flat_patch);
            if !flat.is_empty() {
                doc.extra
                    .insert("settings".into(), Value::Object(flat.clone()));
            }

            Ok(Value::Object(effective_group_settings(doc, flat)))
        })
        .map_err(OpError::io)?;
    let event = append(
        home,
        &group.group_id,
        "group.settings_update",
        request,
        json!({"patch": patch}),
    )?;
    object(json!({
        "group_id": group.group_id,
        "settings": settings_value,
        "event": event,
    }))
}

fn section_target(key: &str) -> Option<(&'static str, &'static str)> {
    SECTION_SETTING_KEYS
        .iter()
        .find(|(legacy, _, _)| *legacy == key)
        .map(|(_, section, canonical)| (*section, *canonical))
}

fn section_mut<'a>(doc: &'a mut GroupDoc, section: &str) -> &'a mut Map<String, Value> {
    let value = doc
        .extra
        .entry(section)
        .or_insert_with(|| Value::Object(Map::new()));
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("group settings section is an object")
}

fn set_section_value(doc: &mut GroupDoc, section: &str, key: &str, value: Value) {
    let target = section_mut(doc, section);
    if value.is_null() {
        target.remove(key);
    } else {
        target.insert(key.into(), value);
    }
}

fn promote_legacy_group_settings(doc: &mut GroupDoc, flat: &mut Map<String, Value>) {
    for key in AUTOMATION_TIMING_KEYS {
        if let Some(legacy) = flat.remove(*key) {
            doc.automation.entry(*key).or_insert(legacy);
        }
    }
    for (legacy_key, section, canonical_key) in SECTION_SETTING_KEYS {
        if let Some(legacy) = flat.remove(*legacy_key) {
            section_mut(doc, section)
                .entry(*canonical_key)
                .or_insert(legacy);
        }
    }
}

fn effective_group_settings(doc: &GroupDoc, mut flat: Map<String, Value>) -> Map<String, Value> {
    for key in AUTOMATION_TIMING_KEYS {
        if let Some(value) = doc.automation.get(*key) {
            flat.insert((*key).into(), value.clone());
        }
    }
    for (legacy_key, section, canonical_key) in SECTION_SETTING_KEYS {
        if let Some(value) = doc
            .extra
            .get(*section)
            .and_then(Value::as_object)
            .and_then(|values| values.get(*canonical_key))
        {
            flat.insert((*legacy_key).into(), value.clone());
        }
    }
    flat
}

fn global_get(home: &HomeLayout, key: &str) -> OpResult {
    let global = settings::load(home).map_err(OpError::io)?;
    let value = if key == "branding" {
        json!(global.branding)
    } else {
        json!(global.observability)
    };
    object(json!({key: value}))
}

fn global_update(home: &HomeLayout, request: &DaemonRequest, key: &str) -> OpResult {
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if !by.is_empty() && by != "user" {
        return Err(OpError::new(
            "permission_denied",
            "only user can update global settings",
        ));
    }
    let patch = patch(request)?;
    let result = settings::update(home, |global| {
        let target = if key == "branding" {
            &mut global.branding
        } else {
            &mut global.observability
        };
        settings::merge(target, &patch);
        if key == "branding" {
            cccc_core::branding::touch(target);
        }
        Ok(target.clone())
    })
    .map_err(OpError::io)?;
    object(json!({key: result}))
}

fn patch(request: &DaemonRequest) -> Result<Map<String, Value>, OpError> {
    request
        .args
        .get("patch")
        .and_then(Value::as_object)
        .cloned()
        .or_else(|| {
            request
                .args
                .get("settings")
                .and_then(Value::as_object)
                .cloned()
        })
        .ok_or_else(|| OpError::new("invalid_args", "patch must be an object"))
}

fn load_group(home: &HomeLayout, request: &DaemonRequest) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(&required_arg(request, "group_id")?)
        .map_err(OpError::not_found)
}
fn authorize(group: &GroupDoc, request: &DaemonRequest) -> Result<(), OpError> {
    permissions::require_group(
        group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
    )
    .map_err(OpError::invalid)
}
fn append(
    home: &HomeLayout,
    group_id: &str,
    kind: &str,
    request: &DaemonRequest,
    data: Value,
) -> Result<Event, OpError> {
    let mut event = Event::new(kind, group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)?;
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;

    #[test]
    fn group_settings_promote_legacy_automation_timing_to_canonical_storage() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("settings", "").expect("group");
        store
            .mutate(&group.group_id, |group| {
                group
                    .automation
                    .insert("actor_idle_timeout_seconds".into(), json!(123));
                group.extra.insert(
                    "settings".into(),
                    json!({
                        "actor_idle_timeout_seconds":999,
                        "help_nudge_interval_seconds":777,
                        "default_send_to":"broadcast",
                        "native_extension":{"keep":true}
                    }),
                );
                group
                    .extra
                    .insert("messaging".into(), json!({"default_send_to":"foreman"}));
                Ok(())
            })
            .expect("seed legacy settings");
        let request = DaemonRequest {
            v: 1,
            op: "group_settings_update".into(),
            args: json!({
                "group_id":group.group_id,
                "by":"user",
                "patch":{
                    "keepalive_delay_seconds":456,
                    "min_interval_seconds":42,
                    "terminal_transcript_visibility":"all",
                    "panorama_enabled":true
                }
            })
            .as_object()
            .cloned()
            .expect("request object"),
        };

        let response = group_settings(&home, &request).expect("settings update");
        let stored = store.load(&group.group_id).expect("stored group");
        let flat = stored.extra["settings"]
            .as_object()
            .expect("flat settings object");

        assert_eq!(stored.automation["actor_idle_timeout_seconds"], json!(123));
        assert_eq!(stored.automation["help_nudge_interval_seconds"], json!(777));
        assert_eq!(stored.automation["keepalive_delay_seconds"], json!(456));
        assert_eq!(
            Value::Object(flat.clone()),
            json!({"native_extension":{"keep":true}})
        );
        for key in AUTOMATION_TIMING_KEYS {
            assert!(!flat.contains_key(*key));
        }
        assert_eq!(
            stored.extra["messaging"]["default_send_to"],
            json!("foreman")
        );
        assert_eq!(stored.extra["delivery"]["min_interval_seconds"], json!(42));
        assert_eq!(
            stored.extra["terminal_transcript"]["visibility"],
            json!("all")
        );
        assert_eq!(stored.extra["features"]["panorama_enabled"], json!(true));
        assert_eq!(
            response["settings"]["actor_idle_timeout_seconds"],
            json!(123)
        );
        assert_eq!(response["settings"]["keepalive_delay_seconds"], json!(456));
        assert_eq!(response["settings"]["default_send_to"], json!("foreman"));
        assert_eq!(response["settings"]["min_interval_seconds"], json!(42));
        assert_eq!(response["group_id"], json!(group.group_id));
        assert_eq!(response["event"]["kind"], json!("group.settings_update"));
    }
}
