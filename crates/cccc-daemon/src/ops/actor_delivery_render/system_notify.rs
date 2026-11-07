use cccc_contracts::Event;
use serde_json::Value;

pub(super) fn body(event: &Event) -> String {
    let context = event.data.get("context").and_then(Value::as_object);
    match context
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "voice_secretary_action_request" => voice_action(event, context),
        _ => fallback(event),
    }
}

fn voice_action(event: &Event, context: Option<&serde_json::Map<String, Value>>) -> String {
    let request = context
        .and_then(|value| value.get("request"))
        .filter(|value| value.is_object());
    let request_id = request
        .and_then(|value| value.get("request_id"))
        .and_then(Value::as_str)
        .or_else(|| {
            context
                .and_then(|value| value.get("request_id"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default();
    let document_path = request
        .and_then(|value| value.get("document_path"))
        .and_then(Value::as_str)
        .or_else(|| {
            context
                .and_then(|value| value.get("document_path"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default();
    let request_text = [
        request
            .and_then(|value| value.get("request_text"))
            .and_then(Value::as_str),
        request
            .and_then(|value| value.get("text"))
            .and_then(Value::as_str),
        event.data.get("text").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .unwrap_or_default();
    let mut metadata = vec!["kind=voice_secretary_action_request".to_owned()];
    if !request_id.trim().is_empty() {
        metadata.push(format!("request_id={}", request_id.trim()));
    }
    if !document_path.trim().is_empty() {
        metadata.push(format!("document_path={}", document_path.trim()));
    }
    let mut blocks = vec![
        "Voice Secretary handed you an action request.".to_owned(),
        metadata.join("; "),
    ];
    if !request_text.trim().is_empty() {
        blocks.push(format!("Request:\n{}", request_text.trim()));
    }
    if let Some(request) = request {
        blocks.push(format!(
            "Request envelope:\n{}",
            serde_json::to_string_pretty(request).unwrap_or_else(|_| request.to_string())
        ));
    }
    blocks.push("Action: handle the request above and report the requested result.".into());
    blocks.join("\n\n")
}

fn fallback(event: &Event) -> String {
    let text = ["title", "message", "text"]
        .into_iter()
        .filter_map(|key| event.data.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    match due_replies(event) {
        ids if ids.is_empty() => text,
        // A notice can wait in the queue, and reading a message does not show its reply state.
        ids => format!(
            "{text}\nDue when this was sent, so some may be settled by now: {}",
            ids.join(", ")
        ),
    }
}

fn due_replies(event: &Event) -> Vec<&str> {
    if event.data.get("kind").and_then(Value::as_str) != Some("reply_notice") {
        return Vec::new();
    }
    event
        .data
        .get("context")
        .and_then(|context| context.get("source_event_ids"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn notice(kind: &str) -> Event {
        let mut event = Event::new("system.notify", "g_test");
        event.data = json!({
            "kind":kind,
            "title":"Reply requested",
            "message":"2 message(s) still need a concrete reply.",
            "context":{"source_event_ids":["event-a","event-b"]},
        })
        .as_object()
        .cloned()
        .expect("object");
        event
    }

    #[test]
    fn a_reply_notice_names_the_messages_it_counts() {
        assert!(body(&notice("reply_notice")).ends_with(
            "2 message(s) still need a concrete reply.\n\
             Due when this was sent, so some may be settled by now: event-a, event-b"
        ));
    }

    #[test]
    fn other_notices_keep_their_text() {
        assert!(!body(&notice("mail_notice")).contains("event-a"));
    }

    #[test]
    fn a_voice_request_is_handled_from_the_notice_itself() {
        let mut event = notice("info");
        event.data["context"] = json!({
            "kind":"voice_secretary_action_request",
            "request":{"request_id":"r1","request_text":"Summarize"},
        });
        let text = body(&event);
        assert!(text.contains("handle the request above"));
        assert!(!text.contains("inbox"));
    }
}
