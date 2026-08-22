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
    let kind = string_at(envelope, &["kind"]).unwrap_or_default();
    let request_id = string_at(envelope, &["request_id"])
        .or_else(|| string_at(envelope, &["metadata", "request_id"]))
        .unwrap_or_default();
    let document_path = string_at(envelope, &["document_path"]).unwrap_or_default();
    let target_kind = string_at(envelope, &["metadata", "target_kind"])
        .or_else(|| string_at(envelope, &["trigger", "target_kind"]))
        .unwrap_or_else(|| {
            if kind == "prompt_refine" {
                "composer"
            } else if !document_path.is_empty() {
                "document"
            } else {
                "secretary"
            }
        });
    let mode = match target_kind {
        "composer" => "prompt",
        "document" => "document",
        _ => "ask",
    };
    let text = string_at(envelope, &["text"]).unwrap_or_default();
    let work_text = if is_structured_work_text(text) {
        text.to_owned()
    } else if target_kind == "document" && kind != "voice_instruction" {
        format!("Inputs:\n{text}")
    } else {
        format!("Task:\n{text}")
    };
    let required_output = match (target_kind, request_id.is_empty()) {
        ("secretary", false) => format!(
            "Call MCP tool cccc_voice_secretary_request(action=\"report\", request_id=\"{request_id}\", status=\"done\"|\"needs_user\"|\"failed\", reply_text=\"...\").\nConsole text alone is not delivered to the user."
        ),
        ("document", false) => format!(
            "Edit the repository markdown at {document_path}, then call MCP tool cccc_voice_secretary_request(action=\"report\", request_id=\"{request_id}\", status=\"done\", reply_text=\"...\").\nConsole text alone is not delivered to the user."
        ),
        ("composer", false) => format!(
            "Call MCP tool cccc_voice_secretary_composer(action=\"submit_prompt_draft\", request_id=\"{request_id}\", draft_text=\"...\").\nDo not execute the prompt. Console text alone is not delivered to the composer."
        ),
        ("document", true) => format!("Edit the repository markdown at {document_path}."),
        _ => "Complete the work through the target-specific MCP output channel.".into(),
    };
    let mut metadata = vec![format!("Mode: {mode}"), format!("Target: {target_kind}")];
    if !request_id.is_empty() {
        metadata.push(format!("Request id: {request_id}"));
    }
    if !document_path.is_empty() {
        metadata.push(format!("Document: {document_path}"));
    }
    [
        "Voice Secretary input is ready. Follow Required output before ending this turn."
            .to_owned(),
        format!("Work order:\n{}", metadata.join("\n")),
        work_text,
        format!(
            "Canonical daemon-delivered input_envelope:\n{}",
            serde_json::to_string_pretty(envelope).unwrap_or_else(|_| envelope.to_string())
        ),
        format!("Required output:\n{required_output}"),
    ]
    .join("\n\n")
}

fn string_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_structured_work_text(value: &str) -> bool {
    ["Task:\n", "Inputs:\n", "Context (not task):\n", "Target:\n"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
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
    blocks
        .push("Action: handle the request from your inbox and report the requested result.".into());
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
