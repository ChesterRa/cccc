//! Thin HTTP port for native Bot Routine wakes; daemon owns all task state.
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
use cccc_core::web_model_connectors::grok_webhook as private;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
#[cfg(test)]
mod tests;

#[derive(Deserialize)]
struct ActorQuery {
    group_id: String,
    actor_id: String,
}
pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/web-model/grok-webhook", get(status).post(change))
}
async fn status(State(s): State<AppState>, Query(q): Query<ActorQuery>) -> ApiResult {
    Ok(success(snapshot(&s, &q.group_id, &q.actor_id).await?))
}
pub(super) async fn snapshot(s: &AppState, g: &str, a: &str) -> Result<Value, ApiError> {
    call(
        s,
        "grok_webhook_status",
        json!({"group_id":g,"actor_id":a})
            .as_object()
            .expect("object")
            .clone(),
    )
    .await
}
async fn change(State(s): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let g = super::web_model_connectors::required(&body, "group_id")?;
    let a = super::web_model_connectors::required(&body, "actor_id")?;
    let action = super::web_model_connectors::required(&body, "action")?;
    if action == "instructions" {
        return Ok(success(json!({"instructions":instructions()})));
    }
    let mut args = json!({"by":"user","group_id":g,"actor_id":a})
        .as_object()
        .expect("object")
        .clone();
    if body.get("transport").is_some() {
        return Err(ApiError::bad("Grok uses native Webhook delivery only"));
    }
    let op = match action.as_str() {
        "bind" => {
            args.insert("url".into(), body["url"].clone());
            let connector = cccc_core::web_model_connectors::load(&s.home)
                .map_err(super::web_model_connector_store::io_error)?
                .into_iter()
                .find(|c| c["provider"] == "grok_web" && c["revoked"] != true)
                .ok_or_else(|| ApiError::bad("Configure the Grok connector first"))?;
            args.insert("connector_id".into(), connector["connector_id"].clone());
            "web_model_grok_bind"
        }
        "disconnect" => {
            let connector = cccc_core::web_model_connectors::load(&s.home)
                .map_err(super::web_model_connector_store::io_error)?
                .into_iter()
                .find(|c| c["provider"] == "grok_web" && c["revoked"] != true)
                .ok_or_else(|| ApiError::bad("Grok connector is unavailable"))?;
            args.insert("connector_id".into(), connector["connector_id"].clone());
            "web_model_binding_remove"
        }
        "configure" => {
            for field in ["url", "key"] {
                if let Some(value) = body.get(field) {
                    args.insert(field.into(), value.clone());
                }
            }
            "grok_webhook_configure"
        }
        "retry" => "grok_webhook_retry",
        "resolve" => {
            args.insert(
                "turn_id".into(),
                json!(super::web_model_connectors::required(&body, "turn_id")?),
            );
            args.insert("event_ids".into(), body["event_ids"].clone());
            "grok_work_resolve"
        }
        _ => return Err(ApiError::bad("Unknown Grok Webhook action")),
    };
    call(&s, op, args).await?;
    Ok(success(snapshot(&s, &g, &a).await?))
}
fn instructions() -> String {
    format!(
        r#"In this selected Grok Bot, create or update one native Webhook Routine named CCCC. Use the existing connected CCCC MCP connector. By pasting these instructions I approve creating/updating this Routine, claiming its event-triggered CCCC turns, replying through CCCC and completing them. Respect Grok's normal permission flow and report blocked calls. Do not create a schedule or another Bot. Save exactly the handler below, with no literal credentials. Verify the saved Routine instructions; a chat confirmation alone is not proof. Do not run the handler during setup. If native tools or CCCC permissions are unavailable, report incomplete setup.

BEGIN CCCC GROK HANDLER
When a cccc.grok.ready Webhook event arrives, use only the actor_token in that event to call cccc_runtime_wait_next_turn once. Omit Group/Actor selectors and transport. This event credential authorizes a claim only.
If the result is work_available:
{handoff}
Follow its turn.system_prompt and instructions. Use only the NEW execution actor_token returned by this claim for all CCCC business tools, including cccc_code_exec. Process the messages in the supplied order, reply through CCCC, then call cccc_runtime_complete_turn with the exact turn_id, event_ids and execution actor_token. A rejected or failed completion is not success: report it.
If idle, stopped or turn_in_progress, exit. On a rejected call, report it and stop. Do not poll, retry business inputs, recover another task, or use credentials from another event or run.
{credential_usage}
END CCCC GROK HANDLER

After saving, open this Routine in the Grok desktop app. Copy its POST to URL and Key into this Actor's CCCC native Webhook settings, not into this chat. If Details/Routines navigation fails, open the named chip on the Created routine message. Keep the Routine until the user explicitly disables it. Stopping access in CCCC revokes execution authority but does not stop cloud work already running in Grok."#,
        handoff = session::NATIVE_EXECUTION_HANDOFF,
        credential_usage = session::CREDENTIAL_ARGUMENT_USAGE
    )
}
pub(super) async fn claim(s: &AppState, r: &Value, b: &Value) -> Value {
    let mut args = r["params"]["arguments"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    args.remove("actor_token");
    args.insert("group_id".into(), b["group_id"].clone());
    args.insert("actor_id".into(), b["actor_id"].clone());
    args.insert("by".into(), b["actor_id"].clone());
    args.insert("_cccc_web_binding".into(), b.clone());
    match call(s, "grok_work_claim", args).await {
        Ok(result) => session::result(r, result, false),
        Err(_) => session::result(
            r,
            json!({"error":"grok_claim_failed","instructions":"The current event cannot claim work. Do not replay business inputs or recover another task."}),
            true,
        ),
    }
}
pub(crate) fn spawn(s: AppState) {
    if s.web_mode.is_read_only() {
        return;
    }
    tokio::spawn(async move {
        let Ok(client) = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
        else {
            return;
        };
        let mut shutdown = s.shutdown.subscribe();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! { _=shutdown.recv()=>break, _=tick.tick()=>{} }
            let mut batch = Vec::new();
            for _ in 0..4 {
                let Ok(next) = call(
                    &s,
                    "grok_webhook_next_delivery",
                    json!({"by":"user"}).as_object().expect("object").clone(),
                )
                .await
                else {
                    break;
                };
                if !next["delivery"].is_object() {
                    break;
                }
                batch.push(next["delivery"].clone());
            }
            // One slow endpoint must not hold every other Actor's wake behind it.
            use futures_util::{StreamExt, stream};
            let deliveries = stream::iter(batch).for_each_concurrent(4, |d| {
                let s = &s; let client = &client;
                async move {
                    let http = send(client, &d).await;
                    let args = json!({"by":"user","connector_id":d["connector_id"],"group_id":d["group_id"],"actor_id":d["actor_id"],
                        "event_id":d["event_id"],"epoch":d["epoch"],"attempt":d["attempt"],"http_status":http});
                    let _ = call(s,"grok_webhook_delivery_record",args.as_object().expect("object").clone()).await;
                }
            });
            tokio::select! { _ = shutdown.recv() => break, _ = deliveries => {} }
        }
    });
}
async fn send(client: &reqwest::Client, d: &Value) -> Option<u16> {
    let url = private::endpoint(d["url"].as_str()?).ok()?;
    post(client, &url, d["key"].as_str()?, &d["body"]).await
}
async fn post(client: &reqwest::Client, url: &str, key: &str, body: &Value) -> Option<u16> {
    // Never log request URLs, keys, payloads or provider response bodies.
    client
        .post(url)
        .bearer_auth(key)
        .json(body)
        .send()
        .await
        .ok()
        .map(|r| r.status().as_u16())
}
