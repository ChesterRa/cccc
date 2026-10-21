//! Private Bot Routine configuration. A wake can claim once; only its returned
//! execution capability can run business tools. Nothing here belongs in a ledger.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub fn endpoint(value: &str) -> io::Result<String> {
    let url = url::Url::parse(value.trim()).map_err(|_| error("invalid_grok_webhook_url"))?;
    // The native Bot Routine surface, not the separate signed Automations API.
    let id = url
        .path()
        .strip_prefix("/automations/webhook/")
        .unwrap_or("");
    if url.scheme() != "https"
        || url.host_str() != Some("api2.cursor.sh")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(error("invalid_grok_webhook_url"));
    }
    Ok(url.into())
}

pub fn state<'a>(c: &'a Value, group: &str, actor: &str) -> &'a Value {
    &c["grok_webhooks"][route_key(group, actor)]
}

pub fn change<T>(
    home: &HomeLayout,
    id: &str,
    group: &str,
    actor: &str,
    change: impl FnOnce(&mut Value, &mut Value) -> io::Result<T>,
) -> io::Result<T> {
    update(home, |root| {
        let c = current(root, id)?;
        if c["provider"] != "grok_web" {
            return Err(error("invalid_web_model_provider"));
        }
        let key = route_key(group, actor);
        let mut s = c["grok_webhooks"][&key].clone();
        let result = change(c, &mut s)?;
        if !c["grok_webhooks"].is_object() {
            c["grok_webhooks"] = json!({});
        }
        if s.is_null() {
            c["grok_webhooks"]
                .as_object_mut()
                .expect("map")
                .remove(&key);
        } else {
            c["grok_webhooks"][key] = s;
        }
        Ok(result)
    })
}

pub fn configure(
    home: &HomeLayout,
    id: &str,
    group: &str,
    actor: &str,
    generation: &str,
    url: &str,
    key: &str,
) -> io::Result<()> {
    let url = endpoint(url)?;
    if key.trim().is_empty() || key.len() > 2048 || key.chars().any(char::is_control) {
        return Err(error("invalid_grok_webhook_key"));
    }
    change(home, id, group, actor, |c, s| {
        let route = route_key(group, actor);
        let b = &mut c["bindings"][&route];
        if b["state"] != "bound" || b["generation"] != generation {
            return Err(error("grok_binding_required"));
        }
        // One Routine cannot wake two Actors with different execution authority.
        if c["grok_webhooks"]
            .as_object()
            .is_some_and(|all| all.iter().any(|(k, s)| k != &route && s["url"] == url))
        {
            return Err(error("grok_webhook_conflict"));
        }
        *s = json!({"group_id":group,"actor_id":actor,"url":url,"key":key,
            "epoch":Uuid::new_v4().to_string(),"wake":null,"execution":null,"last_execution":null});
        c["bindings"][&route]["transport"] = json!("grok_webhook");
        // Retire every credential from the previous configuration.
        c["bindings"][&route]["revision"] = json!(Uuid::new_v4().to_string());
        Ok(())
    })
}

pub fn token(
    c: &Value,
    b: &Value,
    s: &Value,
    stage: &str,
    event: &str,
    turn: &str,
) -> io::Result<String> {
    let mac = binding_mac(
        c,
        b,
        "cccc-grok-webhook-v1",
        &[s["epoch"].clone(), json!(stage), json!(event), json!(turn)],
    )?;
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

pub fn resolve(c: &Value, supplied: &str, tool: &str) -> Option<Value> {
    if c["provider"] != "grok_web" || supplied.len() != 43 {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(supplied).ok()?;
    for b in c["bindings"]
        .as_object()?
        .values()
        .filter(|b| b["transport"] == "grok_webhook")
    {
        let s = state(c, b["group_id"].as_str()?, b["actor_id"].as_str()?);
        for (stage, e) in [
            ("claim", &s["wake"]),
            ("execution", &s["execution"]),
            ("completion", &s["last_execution"]),
        ] {
            let event = e["event_id"].as_str().unwrap_or("");
            let turn = if stage == "claim" {
                ""
            } else {
                e["turn_id"].as_str().unwrap_or("")
            };
            let domain = if stage == "completion" {
                "execution"
            } else {
                stage
            };
            if event.is_empty() || (stage == "completion" && tool != "cccc_runtime_complete_turn") {
                continue;
            }
            let mac = binding_mac(
                c,
                b,
                "cccc-grok-webhook-v1",
                &[s["epoch"].clone(), json!(domain), json!(event), json!(turn)],
            )
            .ok()?;
            if mac.verify_slice(&bytes).is_ok() {
                let mut result = b.clone();
                result["grok_stage"] = json!(stage);
                result["grok_epoch"] = s["epoch"].clone();
                result["grok_wake_id"] = json!(event);
                result["grok_turn_id"] = json!(turn);
                return Some(result);
            }
        }
    }
    None
}

pub fn validate(c: &Value, b: &Value, group: &crate::GroupDoc) -> io::Result<()> {
    let s = state(
        c,
        b["group_id"].as_str().unwrap_or(""),
        b["actor_id"].as_str().unwrap_or(""),
    );
    if s["epoch"] != b["grok_epoch"]
        || !s["epoch"].is_string()
        || !group.running
        || matches!(
            group.state,
            cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
        )
    {
        return Err(error("grok_task_expired"));
    }
    let stage = b["grok_stage"].as_str().unwrap_or("");
    let e = match stage {
        "claim" => &s["wake"],
        "execution" => &s["execution"],
        "completion" => &s["last_execution"],
        _ => return Err(error("grok_task_required")),
    };
    let valid = e["event_id"] == b["grok_wake_id"]
        && e["event_id"].is_string()
        && (stage == "claim"
            || (e["turn_id"] == b["grok_turn_id"]
                && if stage == "execution" {
                    e["state"] == "active"
                        && group
                            .extra
                            .get("runtime_states")
                            .and_then(|r| r.get(b["actor_id"].as_str().unwrap_or("")))
                            .is_some_and(|r| r["active_turn_id"] == e["turn_id"])
                } else {
                    e["state"] == "completed"
                }));
    if valid {
        Ok(())
    } else {
        Err(error("grok_task_expired"))
    }
}

/// Stopping access revokes old cloud capabilities without replaying accepted work.
pub fn invalidate(home: &HomeLayout, group: &str, actor: &str) -> io::Result<()> {
    if !store_path(home).exists() {
        return Ok(());
    }
    fs::with_exclusive_lock(&lock_path(home), || {
        let mut root = read(home)?;
        let mut changed = false;
        for c in root["connectors"]
            .as_object_mut()
            .expect("map")
            .values_mut()
        {
            let Some(s) = c
                .get_mut("grok_webhooks")
                .and_then(Value::as_object_mut)
                .and_then(|all| all.get_mut(&route_key(group, actor)))
            else {
                continue;
            };
            if s.is_object() {
                s["epoch"] = json!(Uuid::new_v4().to_string());
                for field in ["wake", "execution"] {
                    if s[field].is_object() {
                        s[field]["state"] = json!("interrupted");
                    }
                }
                changed = true;
            }
        }
        if changed {
            fs::write_secret_yaml(&store_path(home), &root)?;
        }
        Ok(())
    })
}
