use super::feishu_inbound::materialize_resources;
use super::feishu_outbound::FeishuOutbound;
use super::{
    InboundDecision, InboundMetadata, dispatch_inbound_with, inbound_decision, resolve_credential,
    spawn_outbound, string,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use lark_channel::lark_openapi::{
    OpenApiClient, ReqwestOpenApiTransport, TokioTungsteniteWebSocketTransport, WebSocketEventAck,
};
use lark_channel::{
    ChannelConfig, ChannelEvent, Domain, EventLoop, EventLoopOptions, MessageSender,
    MessageSenderType, OpenApiWebSocketEventConnector, Recipient,
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
    let app_id = resolve_credential(&string(config, "feishu_app_id"))?;
    let app_secret = resolve_credential(&string(config, "feishu_app_secret"))?;
    let mut channel_config = ChannelConfig::new(app_id, app_secret);
    if string(config, "feishu_domain").contains("larksuite") {
        channel_config.domain = Domain::Lark;
    }
    let base_url = channel_config.base_url().to_string();
    let openapi = OpenApiClient::new(channel_config, ReqwestOpenApiTransport::new());
    openapi
        .tenant_access_token()
        .await
        .map_err(|error| format!("Feishu credential verification failed: {error}"))?;
    let sender = MessageSender::new(openapi.clone());
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
                async move {
                    let ChannelEvent::Message(message) = event.event else {
                        return Ok(WebSocketEventAck::ok());
                    };
                    if message.sender.sender_type == MessageSenderType::Bot {
                        return Ok(WebSocketEventAck::ok());
                    }
                    let text = message.text.trim();
                    if text.is_empty() && message.resources.is_empty() {
                        return Ok(WebSocketEventAck::ok());
                    }
                    match inbound_decision(&home, &group_id, PLATFORM, &message.chat_id, text).await
                    {
                        InboundDecision::Forward => {}
                        InboundDecision::Reply(body) => {
                            if let Err(error) = sender
                                .text_message(Recipient::Chat(message.chat_id.clone()), &body)
                                .send()
                                .await
                            {
                                tracing::warn!(%error, "failed to send Feishu command reply");
                            }
                            return Ok(WebSocketEventAck::ok());
                        }
                        InboundDecision::Ignore => return Ok(WebSocketEventAck::ok()),
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
                    if let Err(error) = dispatch_inbound_with(
                        &daemon,
                        &group_id,
                        PLATFORM,
                        &message.chat_id,
                        &message.sender.open_id,
                        text,
                        InboundMetadata {
                            message_id: message.message_id,
                            attachments,
                        },
                    )
                    .await
                    {
                        tracing::warn!(%error, "failed to dispatch Feishu IM message");
                    }
                    Ok(WebSocketEventAck::ok())
                }
            })
            .await;
        if let Err(error) = result {
            tracing::error!(%error, "Feishu IM event loop stopped");
        }
    });
    let outbound = spawn_outbound(
        home,
        group_id.to_owned(),
        ledger_events,
        outbound_sender,
        |sender, targets, event| async move {
            sender.send(&targets, &event).await;
        },
    );
    Ok(vec![connection, outbound])
}
