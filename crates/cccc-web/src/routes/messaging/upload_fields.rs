use serde_json::{Map, Value, json};

use crate::api::ApiError;

pub(crate) fn insert(
    args: &mut Map<String, Value>,
    name: String,
    value: String,
) -> Result<(), ApiError> {
    match name.as_str() {
        "to_json" => insert_recipients(args, &value),
        "refs_json" => insert_refs(args, &value),
        _ => {
            args.insert(name, Value::String(value));
            Ok(())
        }
    }
}

fn insert_recipients(args: &mut Map<String, Value>, value: &str) -> Result<(), ApiError> {
    let recipients = serde_json::from_str::<Value>(value)
        .map_err(|error| ApiError::bad_code("invalid_recipient", error.to_string(), json!({})))?;
    let recipients = recipients.as_array().ok_or_else(|| {
        ApiError::bad_code(
            "invalid_recipient",
            "to_json must be a JSON array",
            json!({}),
        )
    })?;
    if recipients.iter().any(|item| !item.is_string()) {
        return Err(ApiError::bad_code(
            "invalid_recipient",
            "to_json entries must be strings",
            json!({}),
        ));
    }
    args.insert("to".into(), Value::Array(recipients.clone()));
    Ok(())
}

fn insert_refs(args: &mut Map<String, Value>, value: &str) -> Result<(), ApiError> {
    let refs = serde_json::from_str::<Value>(value)
        .map_err(|error| ApiError::bad_code("invalid_refs", error.to_string(), json!({})))?;
    let refs = refs.as_array().ok_or_else(|| {
        ApiError::bad_code("invalid_refs", "refs_json must be a JSON array", json!({}))
    })?;
    args.insert(
        "refs".into(),
        Value::Array(
            refs.iter()
                .filter(|item| item.is_object())
                .cloned()
                .collect(),
        ),
    );
    Ok(())
}
