use serde_json::{Map, Value, json};

pub(crate) fn alias(args: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = args.remove(from) {
        args.entry(to).or_insert(value);
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
