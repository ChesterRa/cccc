//! Host conversation metadata is correlation within an authenticated connector,
//! never authentication or a model-supplied Actor selector.
use crate::AppState;
use cccc_core::web_model_connectors as store;
use serde_json::{Value, json};

pub(super) fn extend_catalog(response: &mut Value) {
    if let Some(tools) = response["result"]["tools"].as_array_mut() {
        tools.extend([
            json!({
                "name":"cccc_connector_status",
                "description":"Use to check this conversation's connection to a CCCC Actor. Reads pairing status and the linked Group/Actor without changing the connection, reading workspace files or running tasks. Available through the CCCC connector before pairing; it is not a tool inside CCCC's cccc_code_exec.",
                "inputSchema":{"type":"object","properties":{},"additionalProperties":false},
                "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
            }),
            json!({
                "name":"cccc_pair",
                "description":"Use when the user connects this conversation to an Actor selected in CCCC. Send the temporary pairing code from this conversation's CCCC setup message back to the same CCCC connector. Records a pending link and returns a receipt; it does not read workspace files or execute tasks. Reply with the receipt exactly as plain text: CCCC verifies it in the selected Actor's window before activating the link. Once verified, subsequent CCCC tool calls use that Actor's configured permissions. Available before pairing through the connector, not inside CCCC's cccc_code_exec. Follow ChatGPT's normal approval flow; report blocked or failed calls.",
                "inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"Temporary CCCC pairing code generated for the selected Actor and supplied in this conversation's setup message. Expires after 10 minutes; send it only to the same CCCC instance's cccc_pair tool."}},"required":["code"],"additionalProperties":false},
                "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
            }),
        ]);
    }
}

pub(super) fn result(request: &Value, payload: Value, error: bool) -> Value {
    json!({"jsonrpc":"2.0","id":request.get("id").cloned().unwrap_or(Value::Null),"result":{"content":[{"type":"text","text":payload.to_string()}],"structuredContent":payload,"isError":error}})
}

pub(super) async fn handle(state: &AppState, connector: &Value, request: &Value) -> Option<Value> {
    if request["method"] != "tools/call" {
        return None;
    }
    let name = request["params"]["name"].as_str()?;
    if !matches!(name, "cccc_connector_status" | "cccc_pair") {
        return None;
    }
    let arguments = request["params"]
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let valid = arguments.as_object().is_some_and(|a| {
        if name == "cccc_pair" {
            a.len() == 1
                && a.get("code")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty() && s.len() <= 256)
        } else {
            a.is_empty()
        }
    });
    if !valid {
        return Some(
            json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32602,"message":"Invalid connector tool arguments"}}),
        );
    }
    let session = match store::session_key(connector, &request["params"]["_meta"]) {
        Ok(key) => key,
        Err(error) => {
            return Some(result(
                request,
                json!({"state":error.to_string(),"instructions":"The ChatGPT host must supply openai/session metadata. CCCC cannot select an Actor without it."}),
                true,
            ));
        }
    };
    if name == "cccc_connector_status" {
        let binding = store::binding_for_session(connector, &session);
        let available = binding.as_ref().is_some_and(|b| {
            let mut b = b.clone();
            b["connector_id"] = connector["connector_id"].clone();
            store::validate_binding(&state.home, &b).is_ok()
        });
        return Some(result(
            request,
            json!({"routing_mode":"session","session_fingerprint":session,
            "state":if binding.is_none(){"unpaired"}else if available{"ready"}else{"actor_unavailable"},
            "group_id":binding.as_ref().map(|b|&b["group_id"]),"actor_id":binding.as_ref().map(|b|&b["actor_id"])}),
            false,
        ));
    }
    let args = json!({"by":"user","connector_id":connector["connector_id"],"code":arguments["code"],"session_key":session}).as_object().expect("literal JSON object").clone();
    Some(
        match super::web_model_delivery_completion::call(state, "web_model_pairing_accept", args)
            .await
        {
            Ok(mut pair) => {
                pair["instructions"] = json!(
                    "CCCC received the pairing code. Reply with the receipt field exactly as plain text so CCCC can verify it in the selected Actor's window and activate the link. This receipt alone does not grant workspace access. After verification, subsequent CCCC tool calls use that Actor's configured permissions. Wait for the next task message; no task execution is part of this setup. Follow ChatGPT's normal approval flow."
                );
                result(request, pair, false)
            }
            Err(error) => result(request, json!({"error":error.to_string()}), true),
        },
    )
}
