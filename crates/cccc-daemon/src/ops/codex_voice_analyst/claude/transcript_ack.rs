use serde_json::Value;

/// Claude may acknowledge its meta-only resume prompt before any real user turn.
/// Recognize the exact provider-generated acknowledgement, never normal model output
/// or synthetic API failures, which must still pass through transcript validation.
pub(super) fn is_resume_ack(record: &Value) -> bool {
    if record.get("type").and_then(Value::as_str) != Some("assistant")
        || record.pointer("/message/model").and_then(Value::as_str) != Some("<synthetic>")
        || record.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true)
        || record.get("error").is_some_and(|value| !value.is_null())
    {
        return false;
    }
    let Some(content) = record.pointer("/message/content").and_then(Value::as_array) else {
        return false;
    };
    content.len() == 1
        && content[0].get("type").and_then(Value::as_str) == Some("text")
        && content[0].get("text").and_then(Value::as_str) == Some("No response requested.")
}
