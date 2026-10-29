use serde_json::{Map, Value, json};

pub(crate) fn alias(args: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = args.remove(from) {
        args.entry(to).or_insert(value);
    }
}

/// MCP uses `mode`; `message_mode` belongs to the daemon IPC contract.
pub(crate) fn message_mode(args: &mut Map<String, Value>, default: &str) -> Result<String, String> {
    if args.contains_key("message_mode") {
        return Err("invalid_args: MCP message tools use mode, not message_mode; remove message_mode and pass mode explicitly".into());
    }
    match args.remove("mode") {
        None => Ok(default.into()),
        Some(Value::String(mode)) if matches!(mode.as_str(), "send" | "request_reply" | "mail") => {
            Ok(mode)
        }
        _ => Err("invalid_args: mode must be send, request_reply or mail".into()),
    }
}

pub(crate) fn normalize_recipients(args: &mut Map<String, Value>) {
    if let Some(Value::String(value)) = args.get("to").cloned() {
        args.insert("to".into(), json!([value]));
    }
}

pub(crate) fn normalize_message_author(args: &mut Map<String, Value>) {
    if args
        .get("by")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return;
    }
    if let Some(actor_id) = args
        .get("actor_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
    {
        args.insert("by".into(), Value::String(actor_id));
    }
}
