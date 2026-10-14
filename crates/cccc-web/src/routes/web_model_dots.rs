//! Dots uses the shared ChatGPT MCP entrance, without browser message delivery.
mod callback;
#[cfg(test)]
mod http_tests;
pub(super) mod protocol;

use super::{web_model_connector_session as session, web_model_delivery_completion::call};
use crate::{
    AppState,
    api::{ApiError, ApiResult, success},
};
use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use cccc_core::web_model_connectors as connectors;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

const EVENT_NAME: &str = "cccc.dots.ready";

#[derive(Deserialize)]
struct ActorQuery {
    group_id: String,
    actor_id: String,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/web-model/dots", get(status).post(change))
}

async fn status(State(state): State<AppState>, Query(query): Query<ActorQuery>) -> ApiResult {
    let args = json!({"group_id":query.group_id,"actor_id":query.actor_id});
    Ok(success(
        call(
            &state,
            "dots_events_status",
            args.as_object().expect("object").clone(),
        )
        .await?,
    ))
}

async fn change(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group = super::web_model_connectors::required(&body, "group_id")?;
    let actor = super::web_model_connectors::required(&body, "actor_id")?;
    let action = super::web_model_connectors::required(&body, "action")?;
    let mut args = json!({"by":"user","group_id":group,"actor_id":actor})
        .as_object()
        .expect("object")
        .clone();
    let op = match action.as_str() {
        "connect" => "dots_events_connect",
        "disconnect" => "dots_events_disconnect",
        "resolve" => {
            args.insert("action".into(), json!("resolve"));
            args.insert(
                "turn_id".into(),
                json!(super::web_model_connectors::required(&body, "turn_id")?),
            );
            let status = call(&state, "dots_events_status", args.clone()).await?;
            args.insert("event_ids".into(), status["active_event_ids"].clone());
            "dots_work_control"
        }
        _ => return Err(ApiError::bad("Unknown Dots connection action")),
    };
    let mut result = call(&state, op, args).await?;
    if action == "connect" {
        let token = result
            .as_object_mut()
            .and_then(|m| m.remove("subscription_token"))
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| ApiError::bad("Dots subscription credential is unavailable"))?;
        let code = result
            .as_object_mut()
            .and_then(|m| m.remove("pairing_code"));
        result["instructions"] = json!(setup_instruction(
            &token,
            code.as_ref().and_then(Value::as_str)
        ));
    }
    Ok(success(result))
}

fn setup_instruction(token: &str, pairing_code: Option<&str>) -> String {
    let pairing = match pairing_code {
        Some(code) => format!(r#"1. Pair this Dot with the selected CCCC Actor by calling cccc_pair once:
{{"code":"{code}"}}
This setup-only code is valid for 10 minutes; never save it in the task handler. Continue only with state=bound and routing_mode=mcp_events. On rejection, report the error and request fresh instructions for this Actor."#),
        None => "1. This Actor is already paired. Use the same Dot conversation; do not pair or select another Actor.".into(),
    };
    format!(
        r#"Create and save a native event-triggered task in Dots using ChatGPT's built-in task/monitoring capability. If this Actor already has such a task, update that task instead of creating another.
Use the existing CCCC plugin in this Dot's ChatGPT account. Refresh its tools and events if it predates this CCCC update.
By pasting these instructions I approve pairing this Dot, saving or updating this Actor's event task, claiming queued work, replying and completing turns. Ask me only if ChatGPT itself blocks plugin permissions; report rejected calls and incomplete setup.

{pairing}

2. Subscribe through ChatGPT's native event-task capability to {EVENT_NAME} with arguments {{"actor_token":"{token}"}}. Request ttlMs=null, or omit ttlMs if the task interface does not offer it; both are persistent. Keep this credential only in subscription configuration. Do not substitute a timed schedule, polling or an ordinary MCP subscribe tool.
Save exactly this credential-free handler as the task's instructions:

BEGIN CCCC DOTS HANDLER
When {EVENT_NAME} wakes this task, call cccc_runtime_wait_next_turn once:
{{}}
ChatGPT metadata identifies the explicitly paired Dot. Do not supply Actor or Group selectors or retrieve credentials from subscription configuration, event data, chat history or memory.
If work_available, follow that turn's system_prompt and instructions. Use the actor_token returned in that tool result for all CCCC calls in this turn, including cccc_code_exec. Reply through CCCC, then call cccc_runtime_complete_turn with the exact turn_id, event_ids and execution actor_token.
If idle, stopped or turn_in_progress, exit. If a call is rejected, report it and stop; do not poll, recover another turn, reconnect or infer another identity.
Use execution credentials only in this turn's tool arguments. Never put them in messages, files, commands, JavaScript source, saved instructions or replies.
END CCCC DOTS HANDLER

3. Verify the task actually saved this handler with {{}} and no literal credential; a chat confirmation is not proof. If saving or subscription fails, report incomplete setup. Do not echo credentials. After successful setup, execute its saved handler once for queued, unclaimed work. Keep the task until I explicitly disable it."#
    )
}

pub(super) fn extend_catalog(response: &mut Value) {
    let Some(tools) = response["result"]["tools"].as_array_mut() else {
        return;
    };
    for tool in tools.iter_mut() {
        if tool["name"] == "cccc_pair" {
            continue;
        }
        if tool["name"] == "cccc_runtime_wait_next_turn" {
            let original = tool["description"].as_str().unwrap_or_default();
            tool["description"] = json!(format!(
                "{original} For ChatGPT Dots: call once with {{}} from the explicitly paired Dot conversation. Host metadata identifies the fixed Actor; no saved credential or event payload is needed. If unpaired, use Get setup instructions in CCCC to pair this Dot. A successful claim returns the current turn's execution actor_token; pairing alone cannot authorize business tools."
            ));
            continue;
        }
        let description = "For ChatGPT Dots only: use the execution actor_token returned by cccc_runtime_wait_next_turn for the current turn. Pairing and the native subscription credential cannot authorize this tool. Ordinary paired ChatGPT conversations omit this field. Never include credentials in messages, files, commands, JavaScript source, saved instructions or replies.";
        tool["inputSchema"]["properties"]["actor_token"] = json!({"type":"string","minLength":43,"maxLength":43,
            "description":description});
    }
}

fn definition() -> Value {
    json!({"name":EVENT_NAME,"description":"The explicitly connected CCCC Dots Actor has queued work. The event only wakes a task; retrieve and finish the exact work through CCCC tools.",
        "delivery":["webhook"],
        "inputSchema":{"type":"object","properties":{"actor_token":{"type":"string","minLength":43,"maxLength":43,"description":"Subscription credential from this fixed Actor's CCCC setup. Authorizes native event subscription only; cannot claim work or authorize business tools."}},"required":["actor_token"],"additionalProperties":false},
        "payloadSchema":{"type":"object","properties":{"event_id":{"type":"string"}},"required":["event_id"],"additionalProperties":false}})
}

pub(super) async fn handle(state: &AppState, connector: &Value, request: &Value) -> Option<Value> {
    let method = request["method"].as_str()?;
    if method == "server/discover" {
        let mut capabilities = json!({"tools":{}});
        if connector["provider"] == "chatgpt_web" {
            capabilities["events"] = json!({});
        }
        return Some(
            json!({"jsonrpc":"2.0","id":request["id"],"result":{"supportedVersions":[protocol::VERSION],"capabilities":capabilities}}),
        );
    }
    if !method.starts_with("events/") {
        return None;
    }
    if connector["provider"] != "chatgpt_web" {
        return Some(protocol::failure(
            request,
            -32601,
            "Events are available through the ChatGPT connector",
        ));
    }
    if method == "events/list" {
        return Some(
            json!({"jsonrpc":"2.0","id":request["id"],"result":{"events":[definition()]}}),
        );
    }
    let outcome = subscription(state, connector, request).await;
    note_event_call(state, connector, method, outcome.as_ref().err());
    Some(match outcome {
        Ok(result) => json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
        Err(error) => error.response(request),
    })
}

/// Keep the outcome of recent event subscription calls on the connector, since
/// later calls overwrite its last activity and callback errors are not logged.
/// Only fixed reason codes are kept: never a credential or callback address.
fn note_event_call(
    state: &AppState,
    connector: &Value,
    method: &str,
    error: Option<&SubscriptionError>,
) {
    let Some(id) = connector["connector_id"].as_str() else {
        return;
    };
    let entry = json!({"at":cccc_contracts::utc_now(),"method":method,"ok":error.is_none(),
        "reason":error.map(SubscriptionError::reason)});
    // Diagnosis must never fail the call it describes.
    let _ = super::web_model_connector_store::update_connector(state, id, |connector| {
        super::web_model_connector_activity::append_call(connector, entry);
    });
}

#[derive(Debug)]
enum SubscriptionError {
    Invalid(&'static str),
    Callback(&'static str),
}

impl From<&'static str> for SubscriptionError {
    fn from(message: &'static str) -> Self {
        Self::Invalid(message)
    }
}

impl SubscriptionError {
    fn reason(&self) -> &'static str {
        match self {
            Self::Invalid(message) => message,
            Self::Callback(reason) => reason,
        }
    }

    fn response(self, request: &Value) -> Value {
        match self {
            Self::Invalid(message) => protocol::failure(request, -32602, message),
            Self::Callback(reason) => {
                let mut error = protocol::failure(request, -32015, "Callback verification failed");
                error["error"]["data"] = json!({"reason":reason});
                error
            }
        }
    }
}

fn subscription_result(result: Value) -> Result<Value, SubscriptionError> {
    let id = result["subscription_id"]
        .as_str()
        .ok_or("Subscription identity is unavailable")?;
    let expiration = if result["expires_at_ms"].is_null() {
        None
    } else {
        Some(
            chrono::DateTime::from_timestamp_millis(
                result["expires_at_ms"]
                    .as_i64()
                    .ok_or("Subscription expiration is invalid")?,
            )
            .ok_or("Subscription expiration is invalid")?
            .to_rfc3339(),
        )
    };
    Ok(json!({"id":id,"refreshBefore":expiration,"cursor":null,"truncated":false}))
}

async fn subscription(
    state: &AppState,
    connector: &Value,
    request: &Value,
) -> Result<Value, SubscriptionError> {
    let params = &request["params"];
    let arguments = params["arguments"]
        .as_object()
        .ok_or("Subscription arguments are required")?;
    if params["name"] != EVENT_NAME
        || arguments.len() != 1
        || params["delivery"]["mode"] != "webhook"
        || !params["cursor"].is_null()
    {
        return Err(SubscriptionError::Invalid("Unsupported event subscription"));
    }
    let token = arguments
        .get("actor_token")
        .and_then(Value::as_str)
        .ok_or("Dots subscription credential is required")?;
    let binding = connectors::dots_subscription_binding(connector, token)
        .ok_or("Dots subscription credential is invalid or revoked")?;
    let callback_url = params["delivery"]["url"]
        .as_str()
        .ok_or("Callback URL is required")?;
    callback::url(callback_url).map_err(SubscriptionError::Callback)?;
    let connector_id = connector["connector_id"]
        .as_str()
        .ok_or("Connector is unavailable")?;
    let mut args = json!({"by":"user","connector_id":connector_id,"subscription_token":token,
        "group_id":binding["group_id"],"actor_id":binding["actor_id"],"expected_revision":binding["revision"],
        "expected_epoch":connector["dots"]["epoch"],"callback_url":callback_url}).as_object().expect("object").clone();
    if request["method"] == "events/unsubscribe" {
        call(state, "dots_events_unsubscribe", args)
            .await
            .map_err(|_| "Dots unsubscription failed")?;
        return Ok(json!({}));
    }
    let secret = params["delivery"]["secret"]
        .as_str()
        .ok_or("Callback signing secret is required")?;
    callback::key(secret).map_err(|_| "Invalid callback signing secret")?;
    // An omitted TTL is persistent: a Dot task interface may not offer ttlMs,
    // and the event task is meant to stay until it is explicitly disabled.
    let ttl = params.get("ttlMs").cloned().unwrap_or(Value::Null);
    let now = chrono::Utc::now().timestamp_millis();
    if !ttl.is_null()
        && !ttl.as_u64().is_some_and(|v| {
            v > 0
                && v <= i64::MAX as u64
                && now
                    .checked_add(v as i64)
                    .and_then(chrono::DateTime::from_timestamp_millis)
                    .is_some()
        })
    {
        return Err(SubscriptionError::Invalid("ttlMs must be positive or null"));
    }
    args.insert("ttl_ms".into(), ttl);
    use sha2::{Digest, Sha256};
    let id = format!(
        "dots_sub_{:x}",
        Sha256::digest(
            json!([connector_id, callback_url, binding["revision"], EVENT_NAME])
                .to_string()
                .as_bytes()
        )
    );
    let old = &connector["dots"]["subscription"];
    let verified = old["verified_at_ms"]
        .as_i64()
        .filter(|time| *time > 0 && *time <= now && now - *time < 600_000);
    let cached = old["enabled"] == true
        && old["subscription_id"] == id
        && old["callback_url"] == callback_url
        && old["signing_secret"] == secret
        && verified.is_some();
    if !cached {
        let challenge = uuid::Uuid::new_v4().simple().to_string();
        let body = serde_json::to_vec(&json!({"type":"verification","challenge":challenge}))
            .map_err(|_| SubscriptionError::Callback("challenge_failed"))?;
        let verification_id = format!("msg_verification_{}", uuid::Uuid::new_v4().simple());
        let headers = callback::headers(
            &id,
            secret,
            &verification_id,
            chrono::Utc::now().timestamp(),
            &body,
            None,
        )
        .map_err(SubscriptionError::Callback)?;
        let (status, bytes) = tokio::time::timeout(
            Duration::from_secs(15),
            callback::post(callback_url, headers, body, true),
        )
        .await
        .map_err(|_| SubscriptionError::Callback("timeout"))?
        .map_err(SubscriptionError::Callback)?;
        let echoed = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v["challenge"].as_str().map(str::to_owned));
        if !(200..300).contains(&status)
            || !echoed
                .as_deref()
                .is_some_and(|actual| callback::matches_challenge(secret, &challenge, actual))
        {
            return Err(SubscriptionError::Callback("challenge_failed"));
        }
    }
    args.insert(
        "verified_at_ms".into(),
        json!(if cached {
            verified.expect("cached verification")
        } else {
            chrono::Utc::now().timestamp_millis()
        }),
    );
    args.insert("subscription_id".into(), json!(id));
    args.insert("signing_secret".into(), json!(secret));
    subscription_result(
        call(state, "dots_events_subscribe", args)
            .await
            .map_err(|_| "Dots subscription changed or could not be saved")?,
    )
}

pub(super) async fn claim(state: &AppState, request: &Value, binding: &Value) -> Value {
    let mut args = request["params"]["arguments"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    args.remove("actor_token");
    args.insert("group_id".into(), binding["group_id"].clone());
    args.insert("actor_id".into(), binding["actor_id"].clone());
    args.insert("by".into(), binding["actor_id"].clone());
    args.insert("_cccc_web_binding".into(), binding.clone());
    match call(state, "dots_work_claim", args).await {
        Ok(result) => session::result(request, result, false),
        Err(_) => session::result(
            request,
            json!({"error":"dots_claim_failed","instructions":"The current event cannot claim work. Do not recover, repeat business inputs or complete another task."}),
            true,
        ),
    }
}

/// The private Web process owns only the network port. The daemon claims and
/// persists notification identities, retries and execution state.
pub(crate) fn spawn(state: AppState) {
    if state.web_mode.is_read_only() {
        return;
    }
    tokio::spawn(async move {
        let mut ledger = state.ledger_events.subscribe_global();
        let mut shutdown = state.shutdown.subscribe();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.recv() => break,
                _ = tick.tick() => {},
                message = ledger.recv() => if matches!(message, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; },
            }
            let Ok(result) = call(
                &state,
                "dots_events_next_delivery",
                json!({"by":"user"}).as_object().expect("object").clone(),
            )
            .await
            else {
                continue;
            };
            let delivery = &result["delivery"];
            if !delivery.is_object() {
                continue;
            }
            let outcome = tokio::select! {
                _ = shutdown.recv() => break,
                outcome = deliver(delivery) => outcome,
            };
            let mut receipt = json!({"by":"user","connector_id":delivery["connector_id"],"event_id":delivery["event_id"],
                "epoch":delivery["epoch"],"attempt":delivery["attempt"]}).as_object().expect("object").clone();
            match outcome {
                Ok(status) => {
                    receipt.insert("http_status".into(), json!(status));
                }
                Err(code) => {
                    receipt.insert("error_code".into(), json!(code));
                }
            }
            let _ = call(&state, "dots_events_delivery_record", receipt).await;
        }
    });
}

async fn deliver(delivery: &Value) -> Result<u16, &'static str> {
    let body = serde_json::to_vec(&delivery["event"]).map_err(|_| "invalid_event_payload")?;
    if body.len() > 256 * 1024 {
        return Err("event_payload_too_large");
    }
    let header = callback::headers(
        delivery["subscription_id"]
            .as_str()
            .ok_or("subscription_unavailable")?,
        delivery["signing_secret"]
            .as_str()
            .ok_or("subscription_unavailable")?,
        delivery["event_id"].as_str().ok_or("event_unavailable")?,
        chrono::Utc::now().timestamp(),
        &body,
        delivery.get("previous_secret").and_then(Value::as_str),
    )?;
    let (status, _) = callback::post(
        delivery["callback_url"]
            .as_str()
            .ok_or("subscription_unavailable")?,
        header,
        body,
        false,
    )
    .await?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_wire_result_uses_protocol_identity_and_granted_expiration() {
        let permanent = subscription_result(
            json!({"subscription_id":"sub_fixture","expires_at_ms":null,"subscribed":true}),
        )
        .expect("wire result");
        assert_eq!(
            permanent,
            json!({"id":"sub_fixture","refreshBefore":null,"cursor":null,"truncated":false})
        );
        let finite =
            subscription_result(json!({"subscription_id":"sub_fixture","expires_at_ms":0}))
                .expect("wire expiration");
        assert_eq!(finite["refreshBefore"], "1970-01-01T00:00:00+00:00");
        assert!(finite.get("expires_at_ms").is_none());
        let error = SubscriptionError::Callback("challenge_failed").response(&json!({"id":1}));
        assert_eq!(error["error"]["code"], -32015);
        assert_eq!(error["error"]["data"]["reason"], "challenge_failed");
    }

    #[test]
    fn task_setup_and_schema_claim_work_without_saved_credentials_or_event_data() {
        let setup = setup_instruction(
            "synthetic-subscription-credential",
            Some("synthetic-setup-code"),
        );
        assert!(setup.contains("native event-triggered task in Dots"));
        assert!(setup.contains("cccc_pair"));
        assert!(setup.contains("valid for 10 minutes"));
        assert!(setup.contains("actor_token returned in that tool result"));
        assert!(setup.contains("turn_in_progress"));
        assert!(setup.contains("By pasting these instructions I approve pairing this Dot"));
        assert!(setup.contains("Ask me only if ChatGPT itself blocks plugin permissions"));
        assert!(setup.contains("omit ttlMs if the task interface does not offer it"));
        let handler = setup
            .split_once("BEGIN CCCC DOTS HANDLER\n")
            .expect("handler start")
            .1
            .split_once("\nEND CCCC DOTS HANDLER")
            .expect("handler end")
            .0;
        assert!(handler.lines().any(|line| line == "{}"));
        assert!(!handler.contains("synthetic-subscription-credential"));
        assert!(!handler.contains("synthetic-setup-code"));
        assert!(setup.contains("update that task instead of creating another"));
        assert!(setup.contains("execute its saved handler once"));
        let paired_setup = setup_instruction("synthetic-subscription-credential", None);
        assert!(!paired_setup.contains("synthetic-setup-code"));
        let event = definition();
        assert_eq!(event["name"], EVENT_NAME);
        assert_eq!(event["inputSchema"]["required"], json!(["actor_token"]));
        assert!(event["payloadSchema"]["properties"]["actor_token"].is_null());
        assert_eq!(event["payloadSchema"]["required"], json!(["event_id"]));
        let mut response = json!({"result":{"tools":[
            {"name":"cccc_help","inputSchema":{"properties":{}}},
            {"name":"cccc_pair","inputSchema":{"properties":{"code":{}}}},
            {"name":"cccc_runtime_wait_next_turn","inputSchema":{"properties":{}}}
        ]}});
        extend_catalog(&mut response);
        assert!(
            response["result"]["tools"][0]["inputSchema"]["properties"]["actor_token"].is_object()
        );
        for index in [1, 2] {
            assert!(
                response["result"]["tools"][index]["inputSchema"]["properties"]["actor_token"]
                    .is_null()
            );
        }
    }
}
