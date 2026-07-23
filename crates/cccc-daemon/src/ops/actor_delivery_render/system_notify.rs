use cccc_contracts::Event;
use serde_json::Value;

pub(super) fn body(event: &Event) -> String {
    let context = event.data.get("context").and_then(Value::as_object);
    match context
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "voice_secretary_input" => voice_input(context),
        "voice_secretary_action_request" => voice_action(event, context),
        _ => fallback(event),
    }
}

fn voice_input(context: Option<&serde_json::Map<String, Value>>) -> String {
    let envelope = context.and_then(|value| value.get("input_envelope"));
    let Some(envelope) = envelope.filter(|value| value.is_object()) else {
        let reason = context
            .and_then(|value| value.get("reason"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut lines = vec![
            "Secretary input ready.",
            "Legacy pointer notification: call MCP tool cccc_voice_secretary_document(action=\"read_new_input\") before doing other work.",
            "Pointer only; fetch the input text through read_new_input.",
        ];
        let reason_line = (!reason.trim().is_empty()).then(|| format!("reason={}", reason.trim()));
        if let Some(ref reason_line) = reason_line {
            lines.push(reason_line);
        }
        return lines.join("\n");
    };
    format!(
        "Voice Secretary input is ready. Work directly from this daemon-delivered input_envelope:\n{}",
        serde_json::to_string_pretty(envelope).unwrap_or_else(|_| envelope.to_string())
    )
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
    let request_text = request
        .and_then(|value| value.get("request_text").or_else(|| value.get("text")))
        .and_then(Value::as_str)
        .or_else(|| event.data.get("text").and_then(Value::as_str))
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
    blocks.push(
        "Action: handle the request from your inbox; acknowledge or reply according to the requested work."
            .into(),
    );
    blocks.join("\n\n")
}

fn fallback(event: &Event) -> String {
    ["title", "message", "text"]
        .into_iter()
        .filter_map(|key| event.data.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
