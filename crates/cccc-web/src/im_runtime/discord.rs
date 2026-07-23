use super::discord_inbound::materialize_attachments;
use super::discord_reactions::DiscordReactions;
use super::{
    InboundDecision, InboundMetadata, dispatch_inbound_with, inbound_decision, outbound_text,
    resolve_credential, spawn_outbound, string,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Map, Value};
use serenity::all::{ChannelId, GatewayIntents, Message, Ready, UserId};
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
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<Vec<JoinHandle<()>>, String> {
    let token = resolve_credential(&string(config, "bot_token_env"))?;
    let http = Arc::new(Http::new(&token));
    let current_user = http
        .get_current_user()
        .await
        .map_err(|error| format!("Discord credential verification failed: {error}"))?;
    let reactions = DiscordReactions::new(Arc::clone(&http));
    let handler = Handler {
        home: home.clone(),
        daemon,
        group_id: group_id.to_owned(),
        download_http: reqwest::Client::new(),
        bot_user_id: current_user.id,
        reactions: reactions.clone(),
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
    let outbound_reactions = reactions;
    let outbound = spawn_outbound(
        home,
        group_id.to_owned(),
        ledger_events,
        http,
        move |http, targets, event| {
            let reactions = outbound_reactions.clone();
            async move {
                let Some(body) = outbound_text(&event, true) else {
                    return;
                };
                for chat_id in targets {
                    let Ok(channel_id) = chat_id.parse::<u64>() else {
                        reactions.fail(&chat_id).await;
                        continue;
                    };
                    if let Err(error) = ChannelId::new(channel_id).say(http.as_ref(), &body).await {
                        tracing::warn!(%error, "failed to send Discord IM message");
                        reactions.fail(&chat_id).await;
                    } else {
                        reactions.complete(&chat_id).await;
                    }
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
    download_http: reqwest::Client,
    bot_user_id: UserId,
    reactions: DiscordReactions,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, context: Context, message: Message) {
        if message.author.bot {
            return;
        }
        let chat_id = message.channel_id.get().to_string();
        let raw_text = message.content.trim();
        let text = strip_leading_bot_mentions(raw_text, self.bot_user_id);
        if !accepts_channel_message(message.guild_id.is_none(), raw_text, text) {
            return;
        }
        if text.is_empty() && message.attachments.is_empty() {
            return;
        }
        match inbound_decision(&self.home, &self.group_id, PLATFORM, &chat_id, text).await {
            InboundDecision::Forward => {}
            InboundDecision::Reply(body) => {
                if let Err(error) = message.channel_id.say(&context.http, body).await {
                    tracing::warn!(%error, "failed to send Discord command reply");
                }
                return;
            }
            InboundDecision::Ignore => return,
        }
        self.reactions.start(&chat_id, &message).await;
        let attachments = materialize_attachments(
            &self.home,
            &self.group_id,
            &self.download_http,
            &message.attachments,
        )
        .await;
        if text.is_empty() && attachments.is_empty() {
            self.reactions.fail_message(&chat_id, message.id).await;
            return;
        }
        if let Err(error) = dispatch_inbound_with(
            &self.daemon,
            &self.group_id,
            PLATFORM,
            &chat_id,
            &message.author.id.get().to_string(),
            text,
            InboundMetadata {
                message_id: message.id.to_string(),
                attachments,
            },
        )
        .await
        {
            tracing::warn!(%error, "failed to dispatch Discord IM message");
            self.reactions.fail_message(&chat_id, message.id).await;
        }
    }

    async fn ready(&self, _context: Context, ready: Ready) {
        tracing::info!(user = %ready.user.name, "Discord IM gateway connected");
    }
}

fn strip_leading_bot_mentions(raw: &str, bot_user_id: UserId) -> &str {
    let regular = format!("<@{}>", bot_user_id.get());
    let nickname = format!("<@!{}>", bot_user_id.get());
    let mut text = raw.trim();
    loop {
        let remainder = text
            .strip_prefix(&regular)
            .or_else(|| text.strip_prefix(&nickname));
        let Some(remainder) = remainder else {
            return text;
        };
        text = remainder.trim_start();
    }
}

fn accepts_channel_message(is_direct_message: bool, raw_text: &str, normalized_text: &str) -> bool {
    is_direct_message || raw_text.trim() != normalized_text || normalized_text.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_repeated_discord_bot_mentions_before_command_parsing() {
        assert_eq!(
            strip_leading_bot_mentions("  <@123> <@!123> /subscribe  ", UserId::new(123)),
            "/subscribe"
        );
    }

    #[test]
    fn keeps_non_leading_mentions_in_message_text() {
        assert_eq!(
            strip_leading_bot_mentions("hello <@123>", UserId::new(123)),
            "hello <@123>"
        );
    }

    #[test]
    fn guilds_accept_mentions_and_commands_but_not_unaddressed_chat() {
        assert!(accepts_channel_message(false, "<@123> hello", "hello"));
        assert!(accepts_channel_message(false, "/subscribe", "/subscribe"));
        assert!(!accepts_channel_message(false, "hello", "hello"));
        assert!(accepts_channel_message(true, "hello", "hello"));
    }
}
