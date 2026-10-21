//! Grok does not supply a per-Bot MCP identity. An opaque binding credential
//! proves possession of the selected Actor's authority, not the caller's Bot.
use super::*;

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
        if let Some(bound) = binding_for_actor(c, group, actor, generation)
            .filter(|b| b["url"] == url && b["transport"] == "grok_webhook")
        {
            return Ok(bound);
        }
        let bound = json!({"group_id":group,"actor_id":actor,"generation":generation,
            "revision":Uuid::new_v4().to_string(),"transport":"grok_webhook","url":url,"state":"bound","updated_at":cccc_contracts::utc_now()});
        if let Some(all) = c["grok_webhooks"].as_object_mut() {
            all.remove(&key);
        }
        c["bindings"][key] = bound.clone();
        Ok(bound)
    })
}
