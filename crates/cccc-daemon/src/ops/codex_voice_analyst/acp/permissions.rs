use super::PermissionPolicy;
use serde_json::{Value, json};

pub(super) fn pending_request(message: &Value, turn_id: &str) -> (String, Value) {
    let mut request = message.clone();
    request["_cccc_turn_id"] = json!(turn_id);
    // Provider RPC ids may repeat. The caller must identify this exact
    // interaction, while the original id stays private for the wire response.
    let token = format!("{turn_id}:{}", uuid::Uuid::new_v4().simple());
    (token, request)
}

pub(super) fn permission_response(message: &Value, policy: PermissionPolicy) -> (Value, bool) {
    if policy == PermissionPolicy::AllowOnce
        && let Some(option_id) = permission_option(
            message,
            &[
                "once",
                "allow-once",
                "allow_once",
                "approve-once",
                "approve_once",
            ],
        )
    {
        return (
            json!({"outcome":{"outcome":"selected","optionId":option_id}}),
            true,
        );
    }
    let option = permission_option(
        message,
        &["reject-once", "reject_once", "deny-once", "deny_once"],
    );
    (
        option.map_or_else(
            || json!({"outcome":{"outcome":"cancelled"}}),
            |option_id| json!({"outcome":{"outcome":"selected","optionId":option_id}}),
        ),
        false,
    )
}

fn permission_option(message: &Value, accepted: &[&str]) -> Option<String> {
    message
        .pointer("/params/options")
        .and_then(Value::as_array)
        .and_then(|options| {
            options.iter().find_map(|option| {
                let id = option.get("optionId").and_then(Value::as_str)?;
                if let Some(kind) = option.get("kind").and_then(Value::as_str) {
                    let expected = if accepted.contains(&"allow_once") {
                        "allow_once"
                    } else {
                        "reject_once"
                    };
                    return (kind == expected).then(|| id.to_owned());
                }
                accepted
                    .contains(&id.to_ascii_lowercase().as_str())
                    .then(|| id.to_owned())
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permission_request() -> Value {
        json!({
            "params": {
                "options": [
                    {"optionId":"reject_once"},
                    {"optionId":"once"},
                    {"optionId":"always"}
                ]
            }
        })
    }

    #[test]
    fn allow_once_selects_only_the_narrow_one_time_option() {
        let (response, allowed) =
            permission_response(&permission_request(), PermissionPolicy::AllowOnce);
        assert!(allowed);
        assert_eq!(response["outcome"]["outcome"], "selected");
        assert_eq!(response["outcome"]["optionId"], "once");
    }

    #[test]
    fn reject_policy_prefers_a_one_time_rejection() {
        let (response, allowed) =
            permission_response(&permission_request(), PermissionPolicy::Reject);
        assert!(!allowed);
        assert_eq!(response["outcome"]["outcome"], "selected");
        assert_eq!(response["outcome"]["optionId"], "reject_once");
    }
}
