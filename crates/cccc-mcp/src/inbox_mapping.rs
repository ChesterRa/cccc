use serde_json::{Map, Value};

const ACTION_ERROR: &str = "cccc_inbox_mark_read action must be 'read' or 'read_all'";

pub(super) fn daemon_call(
    mut args: Map<String, Value>,
) -> Result<(String, Map<String, Value>), String> {
    let action = match args.remove("action") {
        None | Some(Value::Null) => "read".into(),
        Some(Value::String(value)) if value.trim().is_empty() => "read".into(),
        Some(Value::String(value)) => value.trim().to_ascii_lowercase(),
        Some(_) => return Err(ACTION_ERROR.into()),
    };
    let op = match action.as_str() {
        "read" => {
            args.remove("kind_filter");
            "inbox_mark_read"
        }
        "read_all" => {
            args.remove("event_id");
            "inbox_mark_all_read"
        }
        _ => return Err(ACTION_ERROR.into()),
    };
    Ok((op.into(), args))
}
