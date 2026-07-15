use super::{
    accepts_inbound, dispatch_inbound, outbound_text, resolve_credential, spawn_outbound, string,
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
) -> Result<Vec<JoinHandle<()>>, String> {
    let app_id = resolve_credential(&string(config, "feishu_app_id"))?;
    let app_secret = resolve_credential(&string(config, "feishu_app_secret"))?;
    let mut channel_config = ChannelConfig::new(app_id, app_secret);
    if string(config, "feishu_domain").contains("larksuite") {
        channel_config.domain = Domain::Lark;
    }
    let openapi = OpenApiClient::new(channel_config, ReqwestOpenApiTransport::new());
    openapi
        .tenant_access_token()
        .await
        .map_err(|error| format!("Feishu credential verification failed: {error}"))?;
    let sender = MessageSender::new(openapi.clone());
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
    let connection = tokio::spawn(async move {
        let result = event_loop
            .run(move |event| {
                let home = inbound_home.clone();
                let daemon = daemon.clone();
                let group_id = inbound_group.clone();
                async move {
                    let ChannelEvent::Message(message) = event.event else {
                        return Ok(WebSocketEventAck::ok());
                    };
                    if message.sender.sender_type == MessageSenderType::Bot {
                        return Ok(WebSocketEventAck::ok());
                    }
                    let text = message.text.trim();
                    if text.is_empty()
                        || !accepts_inbound(&home, &group_id, PLATFORM, &message.chat_id, text)
                    {
                        return Ok(WebSocketEventAck::ok());
                    }
                    if let Err(error) = dispatch_inbound(
                        &daemon,
                        &group_id,
                        PLATFORM,
                        &message.chat_id,
                        &message.sender.open_id,
                        text,
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
        sender,
        |sender, targets, event| async move {
            let Some(body) = outbound_text(&event, false) else {
                return;
            };
            for chat_id in targets {
                if let Err(error) = sender
                    .text_message(Recipient::Chat(chat_id), &body)
                    .send()
                    .await
                {
                    tracing::warn!(%error, "failed to send Feishu IM message");
                }
            }
        },
    );
    Ok(vec![connection, outbound])
}
