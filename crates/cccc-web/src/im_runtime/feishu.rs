use super::feishu_inbound::materialize_resources;
use super::feishu_outbound::FeishuOutbound;
use super::processing_reactions::FeishuReactions;
use super::{
    InboundDecision, InboundMetadata, completes_processing, dispatch_inbound_with,
    inbound_decision_for_thread, is_outbound_or_stream, processing_reply_to,
    resolve_config_credential, spawn_outbound_matching, string, target_key,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use lark_channel::lark_openapi::{
    OpenApiClient, ReqwestOpenApiTransport, TokioTungsteniteWebSocketTransport, WebSocketEventAck,
};
use lark_channel::{
    ChannelConfig, ChannelEvent, Domain, EventLoop, EventLoopOptions, MessageChatType, MessageId,
    MessageMention, MessageSender, MessageSenderType, OpenApiWebSocketEventConnector, Recipient,
};
use serde_json::{Map, Value};
use std::time::Duration;
use tokio::task::JoinHandle;

const PLATFORM: &str = "feishu";

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<Vec<JoinHandle<()>>, String> {
    let app_id = resolve_config_credential(config, "feishu_app_id", "feishu_app_id_env")?;
    let app_secret =
        resolve_config_credential(config, "feishu_app_secret", "feishu_app_secret_env")?;
    let mut channel_config = ChannelConfig::new(app_id, app_secret);
    if uses_lark_domain(config) {
        channel_config.domain = Domain::Lark;
    }
    let base_url = channel_config.base_url().to_string();
    let openapi = OpenApiClient::new(channel_config, ReqwestOpenApiTransport::new());
    let tenant_token = openapi
        .tenant_access_token()
        .await
        .map_err(|error| format!("Feishu credential verification failed: {error}"))?;
    let bot_open_id = load_bot_open_id(&base_url, &tenant_token).await?;
    let sender = MessageSender::new(openapi.clone());
    let reactions = FeishuReactions::new(reqwest::Client::new(), openapi.clone(), base_url.clone());
    let outbound_sender = FeishuOutbound::new(
        home.clone(),
        group_id,
        reqwest::Client::new(),
        base_url.clone(),
        sender.clone(),
    );
    let inbound_openapi = openapi.clone();
    let connector =
        OpenApiWebSocketEventConnector::new(openapi, TokioTungsteniteWebSocketTransport::new());
    let mut event_loop = EventLoop::with_options(
        connector,
        EventLoopOptions::new()
            .with_max_reconnects(1_000_000)
            .with_reconnect_delay(Duration::from_secs(2))
            .with_server_reconnect_config(true),
    );
    let inbound_home = home.clone();
    let inbound_group = group_id.to_owned();
    let inbound_sender = sender.clone();
    let inbound_http = reqwest::Client::new();
    let inbound_reactions = reactions.clone();
    let inbound_bot_open_id = bot_open_id;
    let connection = tokio::spawn(async move {
        let result = event_loop
            .run(move |event| {
                let home = inbound_home.clone();
                let daemon = daemon.clone();
                let group_id = inbound_group.clone();
                let sender = inbound_sender.clone();
                let openapi = inbound_openapi.clone();
                let http = inbound_http.clone();
                let base_url = base_url.clone();
                let reactions = inbound_reactions.clone();
                let bot_open_id = inbound_bot_open_id.clone();
                async move {
                    let ChannelEvent::Message(message) = event.event else {
                        return Ok(WebSocketEventAck::ok());
                    };
                    if message.sender.sender_type == MessageSenderType::Bot {
                        return Ok(WebSocketEventAck::ok());
                    }
                    let text =
                        strip_leading_bot_mentions(&message.text, &message.mentions, &bot_open_id);
                    if !accepts_feishu_message(&message, &bot_open_id, text) {
                        return Ok(WebSocketEventAck::ok());
                    }
                    if text.is_empty() && message.resources.is_empty() {
                        return Ok(WebSocketEventAck::ok());
                    }
                    let thread_id = message
                        .root_id
                        .as_deref()
                        .or(message.parent_id.as_deref())
                        .unwrap_or_default();
                    match inbound_decision_for_thread(
                        &home,
                        &group_id,
                        PLATFORM,
                        &message.chat_id,
                        thread_id,
                        text,
                    )
                    .await
                    {
                        InboundDecision::Forward => {}
                        InboundDecision::Reply(body) => {
                            let result = if thread_id.is_empty() {
                                sender
                                    .text_message(Recipient::Chat(message.chat_id.clone()), &body)
                                    .send()
                                    .await
                            } else {
                                sender
                                    .text_reply(MessageId(thread_id.to_owned()), &body)
                                    .reply_in_thread(true)
                                    .send()
                                    .await
                            };
                            if let Err(error) = result {
                                tracing::warn!(%error, "failed to send Feishu command reply");
                            }
                            return Ok(WebSocketEventAck::ok());
                        }
                    }
                    let attachments = materialize_resources(
                        &home,
                        &group_id,
                        &http,
                        &openapi,
                        &base_url,
                        &message.resources,
                    )
                    .await;
                    if text.is_empty() && attachments.is_empty() {
                        return Ok(WebSocketEventAck::ok());
                    }
                    let message_id = message.message_id.clone();
                    let processing_key = target_key(&message.chat_id, thread_id);
                    reactions.start(&processing_key, &message_id).await;
                    match dispatch_inbound_with(
                        &daemon,
                        &group_id,
                        PLATFORM,
                        &message.chat_id,
                        &message.sender.open_id,
                        text,
                        InboundMetadata {
                            message_id: message_id.clone(),
                            thread_id: thread_id.to_owned(),
                            attachments,
                        },
                    )
                    .await
                    {
                        Ok(source_event_id) => {
                            reactions.bind_message(&processing_key, &message_id, source_event_id);
                        }
                        Err(error) => {
                            tracing::warn!(%error, "failed to dispatch Feishu IM message");
                            reactions.abort_message(&processing_key, &message_id).await;
                        }
                    }
                    Ok(WebSocketEventAck::ok())
                }
            })
            .await;
        if let Err(error) = result {
            tracing::error!(%error, "Feishu IM event loop stopped");
        }
    });
    let reaction_cleanup = reactions.cleanup_task();
    let outbound_reactions = reactions;
    let outbound = spawn_outbound_matching(
        home,
        group_id.to_owned(),
        PLATFORM,
        ledger_events,
        outbound_sender,
        is_outbound_or_stream,
        move |sender, targets, event| {
            let reactions = outbound_reactions.clone();
            async move {
                let delivered = sender.send(&targets, &event).await;
                if completes_processing(&event) {
                    let reply_to = processing_reply_to(&event);
                    for target in targets {
                        if !delivered.contains(&target.key()) {
                            tracing::warn!(chat_id = %target.chat_id, "Feishu final response was not delivered");
                        }
                        reactions.complete(&target.key(), reply_to).await;
                    }
                }
            }
        },
    );
    Ok(vec![connection, outbound, reaction_cleanup])
}

async fn load_bot_open_id(base_url: &str, token: &str) -> Result<String, String> {
    let response = reqwest::Client::new()
        .get(format!(
            "{}/open-apis/bot/v3/info",
            base_url.trim_end_matches('/')
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("Feishu bot identity request failed: {error}"))?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("Feishu bot identity response is invalid: {error}"))?;
    let open_id = parse_bot_open_id(&value);
    if status.is_success() && value.get("code").and_then(Value::as_i64) == Some(0) {
        return open_id
            .map(str::to_owned)
            .ok_or_else(|| "Feishu bot identity response has no open_id".to_owned());
    }
    Err(value
        .get("msg")
        .and_then(Value::as_str)
        .unwrap_or("Feishu bot identity request failed")
        .to_owned())
}

fn parse_bot_open_id(value: &Value) -> Option<&str> {
    value
        .pointer("/bot/open_id")
        .or_else(|| value.pointer("/data/bot/open_id"))
        .or_else(|| value.pointer("/data/open_id"))
        .or_else(|| value.get("bot_open_id"))
        .or_else(|| value.pointer("/data/bot_open_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn accepts_feishu_message(
    message: &lark_channel::NormalizedMessage,
    bot_open_id: &str,
    text: &str,
) -> bool {
    message.chat_type == MessageChatType::P2p
        || super::commands::is_recognized_command(text)
        || message.mentions_bot(bot_open_id)
}

fn strip_leading_bot_mentions<'a>(
    text: &'a str,
    mentions: &[MessageMention],
    bot_open_id: &str,
) -> &'a str {
    let mut text = text.trim();
    loop {
        let Some(prefix_len) = mentions
            .iter()
            .filter(|mention| {
                mention.mentioned_type == MessageSenderType::Bot && mention.open_id == bot_open_id
            })
            .find_map(|mention| leading_mention_len(text, mention))
        else {
            return text;
        };
        text = text[prefix_len..].trim_start();
    }
}

fn leading_mention_len(text: &str, mention: &MessageMention) -> Option<usize> {
    let name = mention.name.as_deref().map(str::trim).unwrap_or("");
    let prefix_len = if !name.is_empty() {
        if name.starts_with('@') && text.starts_with(name) {
            name.len()
        } else if text.starts_with('@') && text[1..].starts_with(name) {
            name.len() + 1
        } else {
            0
        }
    } else if !mention.key.is_empty() && text.starts_with(&mention.key) {
        mention.key.len()
    } else {
        0
    };
    if prefix_len == 0 {
        return None;
    }
    text[prefix_len..]
        .chars()
        .next()
        .is_none_or(char::is_whitespace)
        .then_some(prefix_len)
}

#[cfg(test)]
mod domain_tests {
    use super::*;

    #[test]
    fn bot_identity_parser_accepts_feishu_standard_top_level_bot() {
        let value = serde_json::json!({
            "code": 0,
            "msg": "ok",
            "bot": { "open_id": " ou_standard " }
        });

        assert_eq!(parse_bot_open_id(&value), Some("ou_standard"));
    }

    #[test]
    fn bot_identity_parser_keeps_legacy_wrapped_shapes() {
        assert_eq!(
            parse_bot_open_id(&serde_json::json!({
                "data": { "bot": { "open_id": "ou_nested" } }
            })),
            Some("ou_nested")
        );
        assert_eq!(
            parse_bot_open_id(&serde_json::json!({
                "data": { "bot_open_id": "ou_flat" }
            })),
            Some("ou_flat")
        );
    }

    fn message(
        chat_type: MessageChatType,
        mentions: Vec<MessageMention>,
    ) -> lark_channel::NormalizedMessage {
        lark_channel::NormalizedMessage {
            message_id: "om_1".into(),
            chat_id: "oc_1".into(),
            chat_type,
            sender_id: "ou_user".into(),
            sender: Default::default(),
            message_type: "text".into(),
            text: "hello".into(),
            raw_content: String::new(),
            content: None,
            root_id: None,
            parent_id: None,
            thread_id: None,
            mentions,
            resources: Vec::new(),
            raw: Value::Null,
        }
    }

    fn mention(name: &str, mentioned_type: MessageSenderType) -> MessageMention {
        MessageMention {
            key: format!("@_{name}"),
            open_id: format!("ou_{name}"),
            user_id: None,
            union_id: None,
            name: Some(name.to_owned()),
            mentioned_type,
        }
    }

    #[test]
    fn leading_bot_mentions_are_removed_before_command_parsing() {
        let mentions = vec![mention("CCCC Bot", MessageSenderType::Bot)];

        assert_eq!(
            strip_leading_bot_mentions(
                "  @CCCC Bot   /send @all hello  ",
                &mentions,
                "ou_CCCC Bot",
            ),
            "/send @all hello"
        );
    }

    #[test]
    fn user_mentions_and_non_prefix_bot_mentions_are_preserved() {
        let mentions = vec![
            mention("CCCC Bot", MessageSenderType::Bot),
            mention("Alice", MessageSenderType::User),
        ];

        assert_eq!(
            strip_leading_bot_mentions("@Alice hello @CCCC Bot", &mentions, "ou_CCCC Bot"),
            "@Alice hello @CCCC Bot"
        );
    }

    #[test]
    fn groups_require_a_command_or_the_current_bot_mention() {
        let current = mention("CCCC Bot", MessageSenderType::Bot);
        let other = mention("Other Bot", MessageSenderType::Bot);
        assert!(!accepts_feishu_message(
            &message(MessageChatType::Group, Vec::new()),
            &current.open_id,
            "hello",
        ));
        assert!(!accepts_feishu_message(
            &message(MessageChatType::Group, vec![other]),
            &current.open_id,
            "hello",
        ));
        assert!(accepts_feishu_message(
            &message(MessageChatType::Group, vec![current.clone()]),
            &current.open_id,
            "hello",
        ));
        assert!(accepts_feishu_message(
            &message(MessageChatType::Group, Vec::new()),
            &current.open_id,
            "/status",
        ));
        assert!(!accepts_feishu_message(
            &message(MessageChatType::Group, Vec::new()),
            &current.open_id,
            "/weather",
        ));
        assert!(accepts_feishu_message(
            &message(MessageChatType::P2p, Vec::new()),
            &current.open_id,
            "hello",
        ));
    }
}

fn uses_lark_domain(config: &Map<String, Value>) -> bool {
    let domain = string(config, "feishu_domain");
    domain.contains("larkoffice") || domain.contains("larksuite")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stable_lark_domain_selects_the_global_endpoint() {
        let config = json!({"feishu_domain":"https://open.larkoffice.com"});
        assert!(uses_lark_domain(config.as_object().expect("configuration")));
        let config = json!({"feishu_domain":"https://open.feishu.cn"});
        assert!(!uses_lark_domain(
            config.as_object().expect("configuration")
        ));
    }
}
