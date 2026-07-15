use super::{
    accepts_inbound, dispatch_inbound, outbound_text, resolve_credential, spawn_outbound, string,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Map, Value};
use serenity::all::{ChannelId, GatewayIntents, Message, Ready};
use serenity::async_trait;
use serenity::http::Http;
use serenity::prelude::{Context, EventHandler};
use std::sync::Arc;
use tokio::task::JoinHandle;

const PLATFORM: &str = "discord";

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
) -> Result<Vec<JoinHandle<()>>, String> {
    let token = resolve_credential(&string(config, "bot_token_env"))?;
    let http = Arc::new(Http::new(&token));
    http.get_current_user()
        .await
        .map_err(|error| format!("Discord credential verification failed: {error}"))?;
    let handler = Handler {
        home: home.clone(),
        daemon,
        group_id: group_id.to_owned(),
    };
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;
    let mut client = serenity::Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|error| format!("Discord gateway setup failed: {error}"))?;
    let connection = tokio::spawn(async move {
        if let Err(error) = client.start().await {
            tracing::error!(%error, "Discord IM gateway stopped");
        }
    });
    let outbound = spawn_outbound(
        home,
        group_id.to_owned(),
        http,
        |http, targets, event| async move {
            let Some(body) = outbound_text(&event, true) else {
                return;
            };
            for channel_id in targets {
                let Ok(channel_id) = channel_id.parse::<u64>() else {
                    continue;
                };
                if let Err(error) = ChannelId::new(channel_id).say(http.as_ref(), &body).await {
                    tracing::warn!(%error, "failed to send Discord IM message");
                }
            }
        },
    );
    Ok(vec![connection, outbound])
}

struct Handler {
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: String,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _context: Context, message: Message) {
        if message.author.bot {
            return;
        }
        let chat_id = message.channel_id.get().to_string();
        let text = message.content.trim();
        if text.is_empty() || !accepts_inbound(&self.home, &self.group_id, PLATFORM, &chat_id, text)
        {
            return;
        }
        if let Err(error) = dispatch_inbound(
            &self.daemon,
            &self.group_id,
            PLATFORM,
            &chat_id,
            &message.author.id.get().to_string(),
            text,
        )
        .await
        {
            tracing::warn!(%error, "failed to dispatch Discord IM message");
        }
    }

    async fn ready(&self, _context: Context, ready: Ready) {
        tracing::info!(user = %ready.user.name, "Discord IM gateway connected");
    }
}
