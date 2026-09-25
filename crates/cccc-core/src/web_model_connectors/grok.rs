//! Grok does not supply a per-Bot MCP identity. An opaque binding credential
//! proves possession of the selected Actor's authority, not the caller's Bot.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};

pub fn grok_bot_url(value: &str) -> io::Result<String> {
    let url = url::Url::parse(value.trim()).map_err(|_| error("invalid_bot_url"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("grok.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(error("invalid_bot_url"));
    }
    let id = url
        .path()
        .strip_prefix("/bot/")
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| error("stable_bot_url_required"))?;
    Ok(format!("https://grok.com/bot/{id}"))
}

pub fn bind_grok(
    home: &HomeLayout,
    id: &str,
    group: &str,
    actor: &str,
    generation: &str,
    url: &str,
) -> io::Result<Value> {
    if generation.is_empty() {
        return Err(error("actor_generation_required"));
    }
    let url = grok_bot_url(url)?;
    update(home, |root| {
        let c = current(root, id)?;
        if c["provider"] != "grok_web" {
            return Err(error("invalid_web_model_provider"));
        }
        let key = route_key(group, actor);
        let bindings = c["bindings"]
            .as_object()
            .ok_or_else(|| error("invalid binding store"))?;
        if bindings
            .iter()
            .any(|(other, b)| other != &key && b["url"] == url)
        {
            return Err(error("binding_conflict"));
        }
        if let Some(bound) =
            binding_for_actor(c, group, actor, generation).filter(|b| b["url"] == url)
        {
            return Ok(bound);
        }
        let bound = json!({"group_id":group,"actor_id":actor,"generation":generation,
            "revision":Uuid::new_v4().to_string(),"url":url,"state":"bound","updated_at":cccc_contracts::utc_now()});
        c["bindings"][key] = bound.clone();
        Ok(bound)
    })
}

fn token_mac(connector: &Value, binding: &Value) -> io::Result<Hmac<Sha256>> {
    if connector["provider"] != "grok_web"
        || connector["revoked"] == true
        || binding["state"] != "bound"
    {
        return Err(error("binding_unavailable"));
    }
    let key = connector["routing_key"]
        .as_str()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| error("connector_unavailable"))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|_| error("connector_unavailable"))?;
    mac.update(
        json!([
            "cccc-grok-binding-v1",
            connector["connector_id"],
            binding["group_id"],
            binding["actor_id"],
            binding["generation"],
            binding["revision"]
        ])
        .to_string()
        .as_bytes(),
    );
    Ok(mac)
}

/// Reconstruct only for browser delivery. Never persist this in turns or ledgers.
pub fn grok_token(connector: &Value, binding: &Value) -> io::Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(token_mac(connector, binding)?.finalize().into_bytes()))
}

pub fn binding_for_token(connector: &Value, token: &str) -> Option<Value> {
    if token.len() != 43 {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(token).ok()?;
    connector["bindings"]
        .as_object()?
        .values()
        .find(|b| token_mac(connector, b).is_ok_and(|mac| mac.verify_slice(&bytes).is_ok()))
        .cloned()
}
