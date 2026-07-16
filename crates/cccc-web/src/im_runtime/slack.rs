use super::{
    accepts_inbound, dispatch_inbound, outbound_text, resolve_credential, spawn_outbound, string,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

const PLATFORM: &str = "slack";
const API: &str = "https://slack.com/api";

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<Vec<JoinHandle<()>>, String> {
    let bot_token = resolve_credential(&string(config, "bot_token_env"))?;
    let app_token = resolve_credential(&string(config, "app_token_env"))?;
    let http = reqwest::Client::new();
    slack_call(&http, &bot_token, "auth.test", json!({}))
        .await
        .map_err(|error| format!("Slack credential verification failed: {error}"))?;
    let initial_endpoint = open_socket_url(&http, &app_token)
        .await
        .map_err(|error| format!("Slack app token verification failed: {error}"))?;

    let inbound_home = home.clone();
    let inbound_group = group_id.to_owned();
    let inbound_http = http.clone();
    let connection = tokio::spawn(async move {
        socket_loop(
            inbound_home,
            daemon,
            inbound_group,
            inbound_http,
            app_token,
            initial_endpoint,
        )
        .await;
    });
    let outbound = spawn_outbound(
        home,
        group_id.to_owned(),
        ledger_events,
        (http, bot_token),
        |sender, targets, event| async move {
            let Some(body) = outbound_text(&event, false) else {
                return;
            };
            for channel in targets {
                if let Err(error) = slack_call(
                    &sender.0,
                    &sender.1,
                    "chat.postMessage",
                    json!({"channel":channel,"text":body}),
                )
                .await
                {
                    tracing::warn!(%error, "failed to send Slack IM message");
                }
            }
        },
    );
    Ok(vec![connection, outbound])
}

async fn socket_loop(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: String,
    http: reqwest::Client,
    app_token: String,
    initial_endpoint: String,
) {
    let mut initial_endpoint = Some(initial_endpoint);
    loop {
        let endpoint = match initial_endpoint.take() {
            Some(endpoint) => Ok(endpoint),
            None => open_socket_url(&http, &app_token).await,
        };
        let endpoint = match endpoint {
            Ok(endpoint) => endpoint,
            Err(error) => {
                tracing::warn!(%error, "failed to open Slack Socket Mode");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };
        let Ok((mut socket, _)) = tokio_tungstenite::connect_async(endpoint).await else {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        };
        while let Some(frame) = socket.next().await {
            let Ok(frame) = frame else { break };
            let Message::Text(raw) = frame else { continue };
            let Ok(envelope) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            if let Some(envelope_id) = envelope.get("envelope_id").and_then(Value::as_str) {
                let _ = socket
                    .send(Message::Text(
                        json!({"envelope_id":envelope_id}).to_string().into(),
                    ))
                    .await;
            }
            let event = &envelope["payload"]["event"];
            if event.get("bot_id").is_some() {
                continue;
            }
            let chat_id = event.get("channel").and_then(Value::as_str).unwrap_or("");
            let sender = event.get("user").and_then(Value::as_str).unwrap_or("user");
            let text = event
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if chat_id.is_empty()
                || text.is_empty()
                || !accepts_inbound(&home, &group_id, PLATFORM, chat_id, text)
            {
                continue;
            }
            if let Err(error) =
                dispatch_inbound(&daemon, &group_id, PLATFORM, chat_id, sender, text).await
            {
                tracing::warn!(%error, "failed to dispatch Slack IM message");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn open_socket_url(http: &reqwest::Client, app_token: &str) -> Result<String, String> {
    slack_call(http, app_token, "apps.connections.open", json!({}))
        .await?
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "Slack Socket Mode response has no url".into())
}

async fn slack_call(
    http: &reqwest::Client,
    token: &str,
    method: &str,
    body: Value,
) -> Result<Value, String> {
    let response = http
        .post(format!("{API}/{method}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if status.is_success() && value.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(value)
    } else {
        Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Slack API request failed")
            .to_owned())
    }
}
