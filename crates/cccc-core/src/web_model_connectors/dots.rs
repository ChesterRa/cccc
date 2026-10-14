//! One explicitly paired Dot route. Subscription configuration never grants
//! task access; execution credentials are fenced to their accepted turn.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub const DOTS_URL: &str = "https://chatgpt.com/dots/home";

pub(super) fn state(connector: &Value) -> Option<(&Value, Value)> {
    if connector["provider"] != "chatgpt_web" || connector["revoked"] == true {
        return None;
    }
    let dots = connector.get("dots").filter(|dots| dots.is_object())?;
    let binding = binding_for_actor(
        connector,
        dots["group_id"].as_str()?,
        dots["actor_id"].as_str()?,
        dots["generation"].as_str()?,
    )?;
    if binding["transport"] != "mcp_events"
        || binding["revision"] != dots["revision"]
        || dots["epoch"].as_str().is_none_or(str::is_empty)
    {
        return None;
    }
    Some((dots, binding))
}

pub fn bind_dots(
    home: &HomeLayout,
    id: &str,
    group: &str,
    actor: &str,
    generation: &str,
) -> io::Result<Value> {
    if generation.is_empty() || group.is_empty() || actor.is_empty() {
        return Err(error("actor_generation_required"));
    }
    update(home, |root| {
        let connector = current(root, id)?;
        require_chatgpt(connector)?;
        if let Some((_, binding)) = state(connector) {
            if binding["group_id"] == group
                && binding["actor_id"] == actor
                && binding["generation"] == generation
            {
                return Ok(binding);
            }
            return Err(error("dots_binding_conflict"));
        }
        let key = route_key(group, actor);
        if !connector["dots"].is_null()
            || connector["bindings"].as_object().is_none_or(|bindings| {
                bindings.contains_key(&key)
                    || bindings
                        .values()
                        .any(|binding| binding["transport"] == "mcp_events")
            })
        {
            return Err(error("dots_binding_conflict"));
        }
        if connector["routing_key"].as_str().is_none_or(str::is_empty) {
            connector["routing_key"] = json!(format!(
                "{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            ));
        }
        let binding = json!({"group_id":group,"actor_id":actor,"generation":generation,
            "revision":Uuid::new_v4().to_string(),"transport":"mcp_events",
            "url":DOTS_URL,"state":"bound","updated_at":cccc_contracts::utc_now()});
        connector["bindings"][&key] = binding.clone();
        connector["dots"] = json!({"group_id":group,"actor_id":actor,"generation":generation,
            "revision":binding["revision"],"epoch":Uuid::new_v4().to_string(),
            "subscription":null,"wake":null,"execution":null,"last_execution":null});
        Ok(binding)
    })
}

/// A setup-only, expiring code authorizes pairing with the selected Dot Actor.
/// Retrieving instructions for an already paired Actor never replaces its session.
pub fn begin_dots_pairing(home: &HomeLayout, id: &str) -> io::Result<Option<Value>> {
    dots_update(home, id, |connector| {
        let (_, binding) = state(connector).ok_or_else(|| error("dots_binding_unavailable"))?;
        if binding["session_key"]
            .as_str()
            .is_some_and(|key| !key.is_empty())
        {
            return Ok(None);
        }
        let (mut pair, result) = pairing_candidate(
            binding["group_id"].as_str().expect("group"),
            binding["actor_id"].as_str().expect("actor"),
            binding["generation"].as_str().expect("generation"),
            false,
        );
        pair["transport"] = json!("mcp_events");
        pair["revision"] = binding["revision"].clone();
        let key = route_key(
            binding["group_id"].as_str().expect("group"),
            binding["actor_id"].as_str().expect("actor"),
        );
        connector["pairings"][key] = pair;
        Ok(Some(result))
    })
}

/// The daemon validates the selected Actor under its lifecycle permit first.
/// Unlike browser pairing, the user explicitly pastes this code into their Dot;
/// no browser receipt or URL can establish the identity of a native event task.
pub fn confirm_dots_pairing(home: &HomeLayout, id: &str, pairing_id: &str) -> io::Result<Value> {
    dots_update(home, id, |connector| {
        let (_, binding) = state(connector).ok_or_else(|| error("dots_binding_unavailable"))?;
        let group = binding["group_id"].as_str().expect("group");
        let actor = binding["actor_id"].as_str().expect("actor");
        let key = route_key(group, actor);
        let pair =
            pairing_for_actor(connector, group, actor).ok_or_else(|| error("pairing_invalid"))?;
        if pair["pairing_id"] != pairing_id
            || pair["transport"] != "mcp_events"
            || pair["generation"] != binding["generation"]
            || pair["revision"] != binding["revision"]
        {
            return Err(error("pairing_changed"));
        }
        if pair["expires_at_ms"]
            .as_i64()
            .is_none_or(|expiry| expiry <= now_ms())
        {
            return Err(error("pairing_expired"));
        }
        if !matches!(
            pair["state"].as_str(),
            Some("awaiting_confirmation" | "bound")
        ) {
            return Err(error("pairing_not_received"));
        }
        let session = pair["session_key"]
            .as_str()
            .filter(|key| !key.is_empty())
            .ok_or_else(|| error("pairing_not_received"))?;
        if binding["session_key"]
            .as_str()
            .is_some_and(|existing| existing != session)
            || connector["bindings"].as_object().is_none_or(|bindings| {
                bindings
                    .iter()
                    .any(|(other, b)| other != &key && b["session_key"] == session)
            })
        {
            return Err(error("binding_conflict"));
        }
        if binding["session_key"] == session {
            return Ok(binding);
        }
        connector["bindings"][&key]["session_key"] = json!(session);
        connector["bindings"][&key]["pairing_id"] = json!(pairing_id);
        connector["bindings"][&key]["updated_at"] = json!(cccc_contracts::utc_now());
        connector["pairings"][&key]["state"] = json!("bound");
        Ok(connector["bindings"][&key].clone())
    })
}

/// Mutate private Dots state using the existing connector lock and secret file.
/// The closure must not return private callback or work state to a public port.
pub fn dots_update<T>(
    home: &HomeLayout,
    id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> io::Result<T> {
    update(home, |root| {
        let connector = current(root, id)?;
        if state(connector).is_none() {
            return Err(error("dots_binding_unavailable"));
        }
        let result = change(connector)?;
        if state(connector).is_none() {
            return Err(error("dots_binding_changed"));
        }
        Ok(result)
    })
}

fn mac(connector: &Value, binding: &Value, event_id: Option<&str>) -> io::Result<Hmac<Sha256>> {
    let (dots, current) = state(connector).ok_or_else(|| error("dots_binding_unavailable"))?;
    if [
        "group_id",
        "actor_id",
        "generation",
        "revision",
        "transport",
    ]
    .iter()
    .any(|field| current[*field] != binding[*field])
    {
        return Err(error("dots_binding_changed"));
    }
    match event_id {
        None => binding_mac(connector, binding, "cccc-dots-subscription-v1", &[]),
        Some(event_id) if !event_id.is_empty() => binding_mac(
            connector,
            binding,
            "cccc-dots-event-v1",
            &[dots["epoch"].clone(), json!(event_id)],
        ),
        Some(_) => Err(error("dots_event_required")),
    }
}

pub fn dots_subscription_token(connector: &Value, binding: &Value) -> io::Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(mac(connector, binding, None)?.finalize().into_bytes()))
}

pub fn dots_wake_token(connector: &Value, binding: &Value, event_id: &str) -> io::Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(
        mac(connector, binding, Some(event_id))?
            .finalize()
            .into_bytes(),
    ))
}

fn token_matches(connector: &Value, binding: &Value, token: &str, event_id: Option<&str>) -> bool {
    if token.len() != 43 {
        return false;
    }
    URL_SAFE_NO_PAD.decode(token).is_ok_and(|bytes| {
        mac(connector, binding, event_id).is_ok_and(|mac| mac.verify_slice(&bytes).is_ok())
    })
}

pub fn dots_subscription_binding(connector: &Value, token: &str) -> Option<Value> {
    let (_, mut binding) = state(connector)?;
    if !token_matches(connector, &binding, token, None) {
        return None;
    }
    binding["connector_id"] = connector["connector_id"].clone();
    Some(binding)
}

fn execution_matches(execution: &Value, dots: &Value, event_id: &str, state: &str) -> bool {
    execution["event_id"] == event_id
        && execution["epoch"] == dots["epoch"]
        && execution["state"] == state
        && execution["turn_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
}

pub fn dots_binding_for_token(connector: &Value, token: &str, tool_name: &str) -> Option<Value> {
    let (dots, binding) = state(connector)?;
    for (stage, record) in [
        ("execution", &dots["execution"]),
        ("completion", &dots["last_execution"]),
    ] {
        let Some(event_id) = record["event_id"].as_str().filter(|id| !id.is_empty()) else {
            continue;
        };
        if !token_matches(connector, &binding, token, Some(event_id)) {
            continue;
        }
        let allowed = match stage {
            "execution" => {
                tool_name != "cccc_runtime_wait_next_turn"
                    && execution_matches(record, dots, event_id, "active")
            }
            _ => {
                tool_name == "cccc_runtime_complete_turn"
                    && execution_matches(record, dots, event_id, "completed")
            }
        };
        if !allowed {
            continue;
        }
        let mut captured = binding.clone();
        captured["connector_id"] = connector["connector_id"].clone();
        captured["dots_epoch"] = dots["epoch"].clone();
        captured["dots_wake_id"] = json!(event_id);
        captured["dots_stage"] = json!(stage);
        captured["dots_turn_id"] = record["turn_id"].clone();
        return Some(captured);
    }
    None
}

/// Only a previously approved host session may request the current task.
/// The returned snapshot is claim-only and cannot authorize business tools.
pub fn dots_claim_binding(connector: &Value, session: &str) -> Option<Value> {
    let (dots, mut binding) = state(connector)?;
    if session.is_empty() || binding["session_key"] != session {
        return None;
    }
    binding["connector_id"] = connector["connector_id"].clone();
    binding["dots_epoch"] = dots["epoch"].clone();
    binding["dots_wake_id"] = dots["wake"]["event_id"].clone();
    binding["dots_stage"] = json!("claim");
    Some(binding)
}

pub(super) fn validate_context(
    connector: &Value,
    expected: &Value,
    group: &crate::GroupDoc,
) -> io::Result<()> {
    let (dots, binding) = state(connector).ok_or_else(|| error("dots_binding_unavailable"))?;
    if expected["dots_epoch"] != dots["epoch"] {
        return Err(error("dots_task_expired"));
    }
    if !group.running
        || matches!(
            group.state,
            cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
        )
    {
        return Err(error("dots_actor_stopped"));
    }
    if expected["dots_stage"] == "claim" {
        return if binding["session_key"]
            .as_str()
            .is_some_and(|key| !key.is_empty())
            && expected["session_key"] == binding["session_key"]
            && expected.get("dots_wake_id") == Some(&dots["wake"]["event_id"])
        {
            Ok(())
        } else {
            Err(error("dots_task_expired"))
        };
    }
    let event_id = expected["dots_wake_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| error("dots_task_required"))?;
    let valid = match expected["dots_stage"].as_str() {
        Some("execution") => {
            let execution = &dots["execution"];
            execution_matches(execution, dots, event_id, "active")
                && expected["dots_turn_id"] == execution["turn_id"]
                && group
                    .extra
                    .get("runtime_states")
                    .and_then(|states| {
                        states.get(expected["actor_id"].as_str().unwrap_or_default())
                    })
                    .is_some_and(|state| state["active_turn_id"] == execution["turn_id"])
        }
        Some("completion") => {
            execution_matches(&dots["last_execution"], dots, event_id, "completed")
                && expected["dots_turn_id"] == dots["last_execution"]["turn_id"]
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(error("dots_task_expired"))
    }
}

/// Retain uncertain work facts, but retire every old cloud execution credential.
pub fn dots_invalidate(home: &HomeLayout, group: &str, actor: &str) -> io::Result<()> {
    if !store_path(home).exists() {
        return Ok(());
    }
    fs::with_exclusive_lock(&lock_path(home), || {
        let mut root = read(home)?;
        let mut changed = false;
        for connector in root["connectors"]
            .as_object_mut()
            .expect("connectors")
            .values_mut()
        {
            if connector["dots"]["group_id"] != group || connector["dots"]["actor_id"] != actor {
                continue;
            }
            let dots = &mut connector["dots"];
            dots["epoch"] = json!(Uuid::new_v4().to_string());
            for field in ["wake", "execution"] {
                if dots[field].is_object() {
                    dots[field]["state"] = json!("interrupted");
                }
            }
            changed = true;
        }
        if changed {
            fs::write_secret_yaml(&store_path(home), &root)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests;
