use cccc_contracts::Event;
use serde_json::{Map, Value, json};

use crate::automation::STANDUP_SNIPPET;
use crate::{GroupDoc, actors, inbox};

pub fn notify_events(
    group: &GroupDoc,
    rule_id: &str,
    rule: &Map<String, Value>,
    action: Option<&Map<String, Value>>,
    scheduled_at: &str,
) -> Vec<Event> {
    let template = snippet(group, action).or_else(|| {
        action
            .and_then(|action| action.get("message"))
            .and_then(Value::as_str)
            .filter(|message| !message.trim().is_empty())
            .map(str::to_owned)
    });
    let Some(template) = template else {
        return Vec::new();
    };
    let interval_seconds = rule
        .get("trigger")
        .and_then(|trigger| trigger.get("every_seconds"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let actor_names = actors::visible(group)
        .filter(|actor| actor.enabled && actor.id != "user")
        .map(|actor| {
            if actor.title.trim().is_empty() {
                actor.id.as_str()
            } else {
                actor.title.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let message = render(
        &template,
        &[
            (
                "interval_minutes",
                if interval_seconds >= 60 {
                    (interval_seconds / 60).to_string()
                } else {
                    "0".into()
                },
            ),
            ("group_title", group.title.clone()),
            ("actor_names", actor_names),
            ("scheduled_at", scheduled_at.to_owned()),
        ],
    )
    .trim()
    .to_owned();
    if message.is_empty() {
        return Vec::new();
    }
    let to = rule
        .get("to")
        .cloned()
        .unwrap_or_else(|| json!(["@foreman"]));
    let mut routing = Event::new("system.notify", &group.group_id);
    routing.by = "system".into();
    routing.data = json!({"to":to}).as_object().cloned().unwrap_or_default();
    actors::visible(group)
        .filter(|actor| actor.enabled && inbox::is_for_actor(group, &routing, &actor.id))
        .map(|actor| {
            let mut event = Event::new("system.notify", &group.group_id);
            event.by = "system".into();
            event.data = json!({
                "kind": "automation",
                "title": action.and_then(|action| action.get("title")).and_then(Value::as_str)
                    .filter(|title| !title.trim().is_empty()).unwrap_or("Reminder"),
                "message": message,
                "target_actor_id": actor.id.as_str(),
                "to": [actor.id.as_str()],
                "im_visibility": "public",
                "priority": action.and_then(|action| action.get("priority")).cloned().unwrap_or_else(|| json!("normal")),
                "context": {"rule_id": rule_id},
            })
            .as_object()
            .cloned()
            .unwrap_or_default();
            event
        })
        .collect()
}

fn snippet(group: &GroupDoc, action: Option<&Map<String, Value>>) -> Option<String> {
    let reference = action
        .and_then(|action| action.get("snippet_ref"))
        .and_then(Value::as_str)?;
    let custom = group
        .automation
        .get("snippets")
        .and_then(|value| value.get(reference));
    let overrides = group
        .automation
        .get("snippet_overrides")
        .and_then(|value| value.get(reference));
    if reference == "standup" {
        return overrides
            .or(custom)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| Some(STANDUP_SNIPPET.into()));
    }
    custom
        .or(overrides)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn render(template: &str, context: &[(&str, String)]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        output.push_str(&remaining[..start]);
        let after_open = &remaining[start + 2..];
        let Some(end) = after_open.find("}}") else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let raw_key = &after_open[..end];
        let key = raw_key.trim();
        if !key.is_empty()
            && key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            if let Some((_, value)) = context.iter().find(|(name, _)| *name == key) {
                output.push_str(value);
            }
        } else {
            output.push_str("{{");
            output.push_str(raw_key);
            output.push_str("}}");
        }
        remaining = &after_open[end + 2..];
    }
    output.push_str(remaining);
    output
}
