use serde_json::{Value, json};

use super::group_bridge_pairing_http::post_remote;

pub(super) async fn claim_approved(
    endpoint: &str,
    outbound: &Value,
    remote_request: &Value,
    approved: bool,
) -> (Value, String) {
    if !approved {
        return (json!({}), String::new());
    }

    // CCCC 0.4.35 has no claim endpoint and returns the credential in its status response.
    if let Some(token) = remote_request["remote_send_token"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        return (
            json!({"claim":{
                "registration_id":remote_request["registration_id"],
                "credential":token,
                "access_level":remote_request["access_level"].as_str().unwrap_or("messages")
            }}),
            String::new(),
        );
    }

    post_remote(
        endpoint,
        "/api/group-bridge/pairing/requests/remote/claim",
        &json!({
            "request_id":remote_request["request_id"],
            "invite_id":outbound["invite_id"],
            "pairing_code":outbound["pairing_code"]
        }),
    )
    .await
}
