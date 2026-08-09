use super::slack_outbound::SlackOutbound;
use super::{
    InboundDecision, InboundMetadata, dispatch_inbound_with, inbound_decision_for_thread,
    is_outbound_or_stream, resolve_credential, spawn_outbound_matching, string,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use std::error::Error as StdError;
use std::fmt;
use std::fmt::Write as _;
use std::future::Future;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

const PLATFORM: &str = "slack";
const API: &str = "https://slack.com/api";
const STARTUP_RETRY_DELAYS: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];

struct SlackSocketContext {
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: String,
    http: reqwest::Client,
    app_token: String,
    bot_token: String,
    bot_user_id: String,
}

#[derive(Debug)]
enum SlackCallError {
    Transport(String),
    Api(String),
}

impl SlackCallError {
    fn is_transient(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

impl fmt::Display for SlackCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) | Self::Api(error) => formatter.write_str(error),
        }
    }
}

fn error_chain(error: &(dyn StdError + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        let _ = write!(message, ": {error}");
        source = error.source();
    }
    message
}

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<Vec<JoinHandle<()>>, String> {
    let bot_token = resolve_credential(&string(config, "bot_token_env"))?;
    let app_token = resolve_credential(&string(config, "app_token_env"))?;
    let (http, auth) = authenticate_http_client(&bot_token)
        .await
        .map_err(|error| format!("Slack credential verification failed: {error}"))?;
    let bot_user_id = auth
        .get("user_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "Slack credential verification response has no user_id".to_owned())?;
    let initial_endpoint =
        retry_transient(&STARTUP_RETRY_DELAYS, || open_socket_url(&http, &app_token))
            .await
            .map_err(|error| format!("Slack app token verification failed: {error}"))?;

    let socket_context = SlackSocketContext {
        home: home.clone(),
        daemon,
        group_id: group_id.to_owned(),
        http: http.clone(),
        app_token,
        bot_token: bot_token.clone(),
        bot_user_id,
    };
    let connection = tokio::spawn(async move {
        socket_loop(socket_context, initial_endpoint).await;
    });
    let outbound_sender =
        SlackOutbound::new(home.clone(), group_id, http.clone(), bot_token.clone());
    let outbound = spawn_outbound_matching(
        home,
        group_id.to_owned(),
        PLATFORM,
        ledger_events,
        outbound_sender,
        is_outbound_or_stream,
        |sender, targets, event| async move {
            sender.send(&targets, &event).await;
        },
    );
    Ok(vec![connection, outbound])
}

async fn authenticate_http_client(
    bot_token: &str,
) -> Result<(reqwest::Client, Value), SlackCallError> {
    let system = reqwest::Client::new();
    match slack_call(&system, bot_token, "auth.test", json!({})).await {
        Ok(auth) => Ok((system, auth)),
        Err(error) if !error.is_transient() => Err(error),
        Err(system_error) => {
            tracing::warn!(
                %system_error,
                "Slack request through configured proxy failed; falling back to direct connection"
            );
            let direct = reqwest::Client::builder()
                .no_proxy()
                .build()
                .map_err(|error| SlackCallError::Transport(error_chain(&error)))?;
            match retry_transient(&STARTUP_RETRY_DELAYS, || {
                slack_call(&direct, bot_token, "auth.test", json!({}))
            })
            .await
            {
                Ok(auth) => Ok((direct, auth)),
                Err(direct_error) => Err(SlackCallError::Transport(format!(
                    "configured proxy path failed: {system_error}; direct fallback failed: {direct_error}"
                ))),
            }
        }
    }
}

async fn socket_loop(context: SlackSocketContext, initial_endpoint: String) {
    let SlackSocketContext {
        home,
        daemon,
        group_id,
        http,
        app_token,
        bot_token,
        bot_user_id,
    } = context;
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
        let (mut socket, _) = match tokio_tungstenite::connect_async(endpoint).await {
            Ok(socket) => socket,
            Err(error) => {
                tracing::warn!(%error, "failed to connect Slack Socket Mode");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
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
            let thread_id = event
                .get("thread_ts")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            let raw_text = event
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let has_files = super::slack_inbound::has_files(event);
            let mentioned = raw_text.contains(&format!("<@{bot_user_id}>"));
            let text = strip_leading_bot_mentions(raw_text, &bot_user_id);
            if !accepts_slack_message(is_private_channel(event, chat_id), mentioned, text) {
                continue;
            }
            if chat_id.is_empty() || (text.is_empty() && !has_files) {
                continue;
            }
            let decision_text = if text.is_empty() {
                "[attachment]"
            } else {
                text
            };
            match inbound_decision_for_thread(
                &home,
                &group_id,
                PLATFORM,
                chat_id,
                thread_id,
                decision_text,
            )
            .await
            {
                InboundDecision::Forward => {}
                InboundDecision::Reply(body) => {
                    let mut reply = json!({"channel":chat_id,"text":body});
                    if !thread_id.is_empty() {
                        reply["thread_ts"] = json!(thread_id);
                    }
                    if let Err(error) =
                        slack_call(&http, &bot_token, "chat.postMessage", reply).await
                    {
                        tracing::warn!(%error, "failed to send Slack command reply");
                    }
                    continue;
                }
            }
            let attachments =
                super::slack_inbound::materialize_files(&home, &group_id, &http, &bot_token, event)
                    .await;
            if let Err(error) = dispatch_inbound_with(
                &daemon,
                &group_id,
                PLATFORM,
                chat_id,
                sender,
                text,
                InboundMetadata {
                    message_id: super::slack_inbound::message_id(event),
                    thread_id: thread_id.to_owned(),
                    attachments,
                },
            )
            .await
            {
                tracing::warn!(%error, "failed to dispatch Slack IM message");
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn is_private_channel(event: &Value, chat_id: &str) -> bool {
    matches!(
        event.get("channel_type").and_then(Value::as_str),
        Some("im" | "mpim")
    ) || chat_id.starts_with('D')
}

fn strip_leading_bot_mentions<'a>(raw: &'a str, bot_user_id: &str) -> &'a str {
    let mention = format!("<@{bot_user_id}>");
    let mut text = raw.trim();
    while let Some(remainder) = text.strip_prefix(&mention) {
        text = remainder.trim_start();
    }
    text
}

fn accepts_slack_message(is_private: bool, mentioned: bool, text: &str) -> bool {
    is_private || mentioned || super::commands::is_recognized_command(text)
}

async fn retry_transient<T, Operation, OperationFuture>(
    delays: &[Duration],
    mut operation: Operation,
) -> Result<T, SlackCallError>
where
    Operation: FnMut() -> OperationFuture,
    OperationFuture: Future<Output = Result<T, SlackCallError>>,
{
    for (attempt, delay) in delays.iter().enumerate() {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if error.is_transient() => {
                tracing::warn!(
                    %error,
                    attempt = attempt + 1,
                    retry_in_ms = delay.as_millis(),
                    "transient Slack startup request failed; retrying"
                );
                tokio::time::sleep(*delay).await;
            }
            Err(error) => return Err(error),
        }
    }
    operation().await
}

async fn open_socket_url(
    http: &reqwest::Client,
    app_token: &str,
) -> Result<String, SlackCallError> {
    slack_call(http, app_token, "apps.connections.open", json!({}))
        .await?
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| SlackCallError::Api("Slack Socket Mode response has no url".into()))
}

async fn slack_call(
    http: &reqwest::Client,
    token: &str,
    method: &str,
    body: Value,
) -> Result<Value, SlackCallError> {
    let response = http
        .post(format!("{API}/{method}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|error| SlackCallError::Transport(error_chain(&error)))?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|error| SlackCallError::Transport(error_chain(&error)))?;
    if status.is_success() && value.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(value)
    } else {
        Err(SlackCallError::Api(
            value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Slack API request failed")
                .to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn transport_error() -> SlackCallError {
        SlackCallError::Transport("connection reset".into())
    }

    #[tokio::test]
    async fn retries_transient_startup_failures_until_success() {
        let attempts = AtomicUsize::new(0);
        let result = retry_transient(&[Duration::ZERO, Duration::ZERO], || async {
            if attempts.fetch_add(1, Ordering::SeqCst) < 2 {
                Err(transport_error())
            } else {
                Ok("connected")
            }
        })
        .await;

        assert_eq!(result.expect("eventual success"), "connected");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_retry_slack_api_errors() {
        let attempts = AtomicUsize::new(0);
        let result = retry_transient(&[Duration::ZERO], || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(SlackCallError::Api("invalid_auth".into()))
        })
        .await;

        assert!(matches!(result, Err(SlackCallError::Api(_))));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn returns_last_error_after_transient_retries_are_exhausted() {
        let attempts = AtomicUsize::new(0);
        let result = retry_transient(&[Duration::ZERO, Duration::ZERO], || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(transport_error())
        })
        .await;

        assert!(matches!(result, Err(SlackCallError::Transport(_))));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn strips_repeated_leading_bot_mentions_before_command_parsing() {
        assert_eq!(
            strip_leading_bot_mentions("  <@U123> <@U123> /subscribe  ", "U123"),
            "/subscribe"
        );
    }

    #[test]
    fn keeps_non_leading_mentions_in_message_text() {
        assert_eq!(
            strip_leading_bot_mentions("hello <@U123>", "U123"),
            "hello <@U123>"
        );
    }

    #[test]
    fn detects_private_slack_conversations() {
        assert!(is_private_channel(&json!({"channel_type":"im"}), "C123"));
        assert!(is_private_channel(&json!({}), "D123"));
        assert!(!is_private_channel(
            &json!({"channel_type":"channel"}),
            "C123"
        ));
    }

    #[test]
    fn channel_commands_do_not_require_a_bot_mention() {
        assert!(accepts_slack_message(false, false, "/subscribe"));
        assert!(!accepts_slack_message(false, false, "/weather"));
        assert!(!accepts_slack_message(false, false, "ordinary text"));
        assert!(accepts_slack_message(false, true, "ordinary text"));
        assert!(accepts_slack_message(true, false, "ordinary text"));
    }
}
