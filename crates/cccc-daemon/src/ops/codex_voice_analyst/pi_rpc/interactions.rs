use serde_json::{Value, json};
use std::io;

pub(super) fn dialog(record: &Value) -> bool {
    record["type"] == "extension_ui_request"
        && matches!(
            record["method"].as_str(),
            Some("select" | "confirm" | "input" | "editor")
        )
}

pub(super) fn metadata(record: &Value, token: &str) -> io::Result<Value> {
    if !record["id"].is_string() || !record["title"].is_string() {
        return Err(invalid());
    }
    let method = record["method"].as_str().ok_or_else(invalid)?;
    if method == "select"
        && !record["options"]
            .as_array()
            .is_some_and(|options| !options.is_empty() && options.iter().all(Value::is_string))
    {
        return Err(invalid());
    }
    Ok(
        json!({"request_id":token,"kind":format!("pi_{method}"),"title":record["title"],
        "message":record["message"],"options":record["options"],"placeholder":record["placeholder"],"prefill":record["prefill"]}),
    )
}

pub(super) fn response(record: &Value, reply: &Value) -> io::Result<Value> {
    if !reply.is_object() || serde_json::to_vec(reply).map_err(io::Error::other)?.len() > 64 * 1024
    {
        return Err(invalid());
    }
    let mut response = json!({"type":"extension_ui_response","id":record["id"]});
    if reply["cancelled"] == true {
        response["cancelled"] = json!(true);
        return Ok(response);
    }
    match record["method"].as_str() {
        Some("confirm") => {
            response["confirmed"] = json!(reply["confirmed"].as_bool().ok_or_else(invalid)?)
        }
        Some("select") => {
            let value = reply["value"].as_str().ok_or_else(invalid)?;
            if !record["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option.as_str() == Some(value)))
            {
                return Err(invalid());
            }
            response["value"] = json!(value);
        }
        Some("input" | "editor") => {
            response["value"] = json!(reply["value"].as_str().ok_or_else(invalid)?)
        }
        _ => return Err(invalid()),
    }
    Ok(response)
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Answer does not match the pending Pi interaction",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_supported_fields_and_explicit_valid_choices_reach_pi() {
        let request = json!({"type":"extension_ui_request","id":"provider-reused","method":"select","title":"Pick","options":["a","b"]});
        assert!(response(&request, &json!({"confirmed":true})).is_err());
        assert!(response(&request, &json!({"value":"foreign"})).is_err());
        assert_eq!(
            response(&request, &json!({"value":"b","id":"foreign","extra":true})).expect("answer"),
            json!({"type":"extension_ui_response","id":"provider-reused","value":"b"})
        );
        let confirm = json!({"method":"confirm","id":"p"});
        assert!(response(&confirm, &json!({"value":"yes"})).is_err());
        assert_eq!(
            response(&confirm, &json!({"confirmed":false})).expect("explicit false")["confirmed"],
            false
        );
    }
}
