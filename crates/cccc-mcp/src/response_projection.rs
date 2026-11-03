use serde_json::{Map, Value};

/// The caller's exact values, before dispatch. Missing or non-string fields
/// are not equivalent to the daemon's default or normalized values.
#[derive(Default)]
pub(crate) struct EchoedFields {
    text: Option<String>,
    insight: Option<String>,
}

impl EchoedFields {
    pub(crate) fn capture(arguments: &Map<String, Value>) -> Self {
        Self {
            text: arguments
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_owned),
            insight: arguments
                .get("insight")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    }
}

/// Project only known message-event locations. All other result data, including
/// partial failures, generated questions and attachments, remains unchanged.
pub(crate) fn apply(name: &str, payload: &mut Map<String, Value>, input: &EchoedFields) {
    const MARKER: &str = "mcp_omitted_fields";
    if payload.contains_key(MARKER) {
        return;
    }
    let paths: &[&str] = match name {
        "cccc_message_send" | "cccc_message_reply" => {
            &["event", "source_event", "src_event", "dst_event"]
        }
        "cccc_tracked_send" => &["event"],
        "cccc_file_send" => &[
            "result.event",
            "result.source_event",
            "result.src_event",
            "result.dst_event",
        ],
        _ => return,
    };
    let mut omitted = Map::new();
    for path in paths {
        let event = if let Some((parent, key)) = path.split_once('.') {
            payload
                .get_mut(parent)
                .and_then(Value::as_object_mut)
                .and_then(|value| value.get_mut(key))
        } else {
            payload.get_mut(*path)
        };
        let Some(event) = event.and_then(Value::as_object_mut) else {
            continue;
        };
        if event.get("kind").and_then(Value::as_str) != Some("chat.message") {
            continue;
        }
        let Some(data) = event.get_mut("data").and_then(Value::as_object_mut) else {
            continue;
        };
        let mut fields = Vec::new();
        for (field, submitted) in [("text", &input.text), ("insight", &input.insight)] {
            if submitted
                .as_deref()
                .is_some_and(|value| data.get(field).and_then(Value::as_str) == Some(value))
            {
                data.remove(field);
                fields.push(Value::String(field.into()));
            }
        }
        if !fields.is_empty() {
            omitted.insert((*path).into(), Value::Array(fields));
        }
    }
    if !omitted.is_empty() {
        payload.insert(MARKER.into(), Value::Object(omitted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(value: Value) -> Map<String, Value> {
        value.as_object().expect("object").clone()
    }

    fn event(text: &str, insight: &str) -> Value {
        json!({"id":"e1","kind":"chat.message","data":{
            "text":text,"insight":insight,"quote_text":"keep", "attachments":[{"path":"a"}]
        }})
    }

    #[test]
    fn projects_known_local_and_cross_group_events_and_records_each_path() {
        let input = EchoedFields::capture(&args(json!({"text":"hello","insight":"why"})));
        let mut payload = args(
            json!({"event":event("hello","why"),"source_event":event("hello","why"),
            "src_event":event("hello","why"),"dst_event":event("hello","why"),
            "partial_failure":true,"duplicate":true,"error":{"details":{"text":"hello"}}}),
        );
        apply("cccc_message_send", &mut payload, &input);
        for key in ["event", "source_event", "src_event", "dst_event"] {
            assert!(payload[key]["data"].get("text").is_none());
            assert!(payload[key]["data"].get("insight").is_none());
            assert_eq!(payload[key]["data"]["quote_text"], "keep");
            assert_eq!(
                payload["mcp_omitted_fields"][key],
                json!(["text", "insight"])
            );
        }
        assert_eq!(payload["error"]["details"]["text"], "hello");
        assert_eq!(payload["partial_failure"], true);
        assert_eq!(payload["duplicate"], true);
    }

    #[test]
    fn preserves_generated_or_different_values_and_absent_arguments() {
        let input = EchoedFields::capture(&args(json!({"text":"original"})));
        let mut payload = args(
            json!({"event":event("expanded question","original"), "question":{"prompt":"original"}}),
        );
        let before = payload.clone();
        apply("cccc_message_send", &mut payload, &input);
        assert_eq!(payload, before);
        let mut payload = args(json!({"event":event("original","unknown" )}));
        apply("cccc_message_send", &mut payload, &input);
        assert_eq!(payload["event"]["data"]["insight"], "unknown");
        assert_eq!(payload["mcp_omitted_fields"]["event"], json!(["text"]));

        let input = EchoedFields::capture(&args(json!({"text":"hello\n","insight":" reason "})));
        let mut payload = args(json!({"event":event("hello","reason")}));
        let before = payload.clone();
        apply("cccc_message_reply", &mut payload, &input);
        assert_eq!(payload, before, "normalization is not an exact match");
    }

    #[test]
    fn reply_and_tracked_receipts_use_the_same_exact_match_rule() {
        let input = EchoedFields::capture(&args(json!({"text":"答复","insight":"理由"})));
        for name in ["cccc_message_reply", "cccc_tracked_send"] {
            let mut payload = args(json!({"event":event("答复","理由"),"task_id":"T1",
                "message_sent":true,"partial_failure":false}));
            apply(name, &mut payload, &input);
            assert!(payload["event"]["data"].get("text").is_none());
            assert_eq!(
                payload["mcp_omitted_fields"]["event"],
                json!(["text", "insight"])
            );
            assert_eq!(payload["task_id"], "T1");
            assert_eq!(payload["message_sent"], true);
            assert_eq!(payload["partial_failure"], false);
        }
    }

    #[test]
    fn file_receipts_only_project_nested_known_events() {
        let input = EchoedFields::capture(&args(json!({"text":"note","insight":"reason"})));
        let mut payload = args(json!({"sent":true,"attachment":{"text":"note"},
            "event":event("note","reason"),"result":{"source_event":event("note","reason"),"delivery_id":"d1"}}));
        apply("cccc_file_send", &mut payload, &input);
        assert_eq!(payload["event"]["data"]["text"], "note");
        assert_eq!(payload["attachment"]["text"], "note");
        assert!(
            payload["result"]["source_event"]["data"]
                .get("text")
                .is_none()
        );
        assert_eq!(
            payload["mcp_omitted_fields"]["result.source_event"],
            json!(["text", "insight"])
        );
        assert_eq!(payload["result"]["delivery_id"], "d1");
    }

    #[test]
    fn does_not_modify_unknown_shapes_or_overwrite_existing_marker() {
        let input = EchoedFields::capture(&args(json!({"text":"hello"})));
        for (name, value) in [
            (
                "cccc_message_send",
                json!({"event":{"kind":"system.notify","data":{"text":"hello"}}}),
            ),
            ("other", json!({"event":event("hello", "")})),
            (
                "cccc_message_send",
                json!({"event":event("hello", ""),"mcp_omitted_fields":{"external":true}}),
            ),
        ] {
            let mut payload = args(value);
            let before = payload.clone();
            apply(name, &mut payload, &input);
            assert_eq!(payload, before);
        }
    }
}
