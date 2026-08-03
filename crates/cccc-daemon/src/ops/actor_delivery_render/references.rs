use cccc_contracts::Event;
use serde_json::Value;

const MAX_REFERENCES: usize = 4;

pub(super) fn lines(event: &Event) -> Vec<String> {
    let refs = event
        .data
        .get("refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("hidden").and_then(Value::as_bool) != Some(true))
        .take(MAX_REFERENCES)
        .filter_map(render)
        .collect::<Vec<_>>();
    if refs.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["[cccc] References:".to_owned()];
    lines.extend(refs);
    lines
}

fn render(item: &Value) -> Option<String> {
    let kind = item.get("kind").and_then(Value::as_str).unwrap_or("ref");
    if kind == "group_bridge_route" {
        return render_group_bridge_route(item);
    }
    if kind == "local_group_route" {
        return render_local_group_route(item);
    }
    let label = ["title", "path", "url", "task_id", "slot_id"]
        .into_iter()
        .find_map(|key| nonempty(item, key))
        .unwrap_or(kind);
    Some(format!("- {kind}: {}", compact(label, 120)))
}

fn render_local_group_route(item: &Value) -> Option<String> {
    let group_id = nonempty(item, "group_id")?;
    let label = nonempty(item, "group_title")
        .or_else(|| nonempty(item, "token"))
        .unwrap_or(group_id);
    Some(format!(
        "- Local group route {} (group_id={}); this is context, not an automatic send. If the user asks you to contact it, decide first, then use cccc_message_send with dst_group_id=\"{}\", to=\"@foreman\" or a target actor, and your own natural message. Do not forward the user's text or a template.",
        compact(label, 72),
        compact(group_id, 48),
        compact(group_id, 48)
    ))
}

fn render_group_bridge_route(item: &Value) -> Option<String> {
    let remote_group_id = nonempty(item, "remote_group_id")?;
    let label = nonempty(item, "remote_group_title")
        .or_else(|| nonempty(item, "token"))
        .unwrap_or(remote_group_id);
    let access = nonempty(item, "access_level");
    let route = access.map_or_else(
        || format!("remote_group_id={}", compact(remote_group_id, 48)),
        |access| {
            format!(
                "{} remote/{}",
                compact(remote_group_id, 48),
                compact(access, 24)
            )
        },
    );
    Some(format!(
        "- Group Bridge route {} ({route}); send with cccc_message_send dst_group_id=\"{}\" and an explicit remote recipient such as to=\"@foreman\".",
        compact(label, 72),
        compact(remote_group_id, 48)
    ))
}

fn nonempty<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn compact(value: &str, limit: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        return normalized;
    }
    format!(
        "{}...",
        normalized
            .chars()
            .take(limit.saturating_sub(3))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_group_bridge_route_as_an_actionable_remote_target() {
        let mut event = Event::new("chat.message", "g_local");
        event.data = json!({
            "refs":[{
                "kind":"group_bridge_route",
                "remote_group_id":"g_remote",
                "remote_group_title":"外部数据采集平台",
                "access_level":"messages",
                "token":"#外部数据采集平台"
            }]
        })
        .as_object()
        .cloned()
        .expect("event data");

        assert_eq!(
            lines(&event),
            vec![
                "[cccc] References:",
                "- Group Bridge route 外部数据采集平台 (g_remote remote/messages); send with cccc_message_send dst_group_id=\"g_remote\" and an explicit remote recipient such as to=\"@foreman\"."
            ]
        );
    }

    #[test]
    fn skips_invalid_group_bridge_route_instead_of_repeating_its_kind() {
        let mut event = Event::new("chat.message", "g_local");
        event.data = json!({"refs":[{"kind":"group_bridge_route"}]})
            .as_object()
            .cloned()
            .expect("event data");

        assert!(lines(&event).is_empty());
    }

    #[test]
    fn renders_local_group_route_as_ai_owned_contact_context() {
        let mut event = Event::new("chat.message", "g_local");
        event.data = json!({
            "refs":[{
                "kind":"local_group_route",
                "group_id":"g_self_agent",
                "group_title":"Self Agent",
                "token":"#Self Agent"
            }]
        })
        .as_object()
        .cloned()
        .expect("event data");

        let rendered = lines(&event).join("\n");
        assert!(rendered.contains("Local group route Self Agent (group_id=g_self_agent)"));
        assert!(rendered.contains("this is context, not an automatic send"));
        assert!(rendered.contains("your own natural message"));
        assert!(rendered.contains("Do not forward the user's text or a template"));
    }
}
