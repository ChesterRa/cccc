use super::discord_gateway_proxy::GatewayRelay;
use super::discord_inbound::materialize_attachments;
use super::discord_outbound::DiscordOutbound;
use super::discord_reactions::DiscordReactions;
use super::worker::Stopper;
use super::{
    InboundDecision, InboundMetadata, completes_processing, dispatch_inbound_with,
    inbound_decision, is_outbound_or_stream, processing_reply_to, resolve_config_credential,
    spawn_outbound_matching, target_key,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Map, Value};
use serenity::all::{GatewayIntents, Message, Ready, RoleId, UserId};
use serenity::async_trait;
use serenity::gateway::GatewayError;
use serenity::http::Http;
use serenity::prelude::{Context, EventHandler};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

const PLATFORM: &str = "discord";
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
    deduper: Arc<super::discord_dedup::DiscordMessageDeduper>,
) -> Result<(Vec<JoinHandle<()>>, Stopper), String> {
    let token = resolve_config_credential(config, "bot_token", "bot_token_env")?;
    let http = Arc::new(Http::new(&token));
    let current_user = http
        .get_current_user()
        .await
        .map_err(|error| format!("Discord credential verification failed: {error}"))?;
    let reactions = DiscordReactions::new(Arc::clone(&http));
    let (ready_tx, ready_rx) = oneshot::channel();
    let handler = Handler {
        home: home.clone(),
        daemon,
        group_id: group_id.to_owned(),
        download_http: reqwest::Client::new(),
        bot_user_id: current_user.id,
        reactions: reactions.clone(),
        deduper,
        ready: Arc::new(std::sync::Mutex::new(Some(ready_tx))),
    };
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;
    let mut client = serenity::Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|error| format!("Discord gateway setup failed: {error}"))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut gateway_relay =
        super::discord_gateway_proxy::start_from_env(shutdown_rx.clone()).await?;
    if let Some(relay) = &gateway_relay {
        *client.ws_url.lock().await = relay.local_url.clone();
    }
    let shard_manager = Arc::clone(&client.shard_manager);
    let connection_shards = Arc::clone(&shard_manager);
    let mut connection_shutdown_rx = shutdown_rx;
    let (connection_error_tx, connection_error_rx) = oneshot::channel();
    let mut connection = tokio::spawn(async move {
        tokio::select! {
            result = client.start() => {
                if let Err(error) = result {
                    tracing::error!(%error, "Discord IM gateway stopped");
                    let _ = connection_error_tx.send(describe_gateway_error(&error));
                }
            }
            changed = connection_shutdown_rx.changed() => {
                if changed.is_ok() && *connection_shutdown_rx.borrow() {
                    connection_shards.shutdown_all().await;
                }
            }
        }
    });
    let startup = tokio::time::timeout(READY_TIMEOUT, async {
        tokio::select! {
            ready = ready_rx => ready.map_err(|_| "Discord gateway stopped before READY".to_owned()),
            error = connection_error_rx => Err(error.unwrap_or_else(|_| "Discord gateway stopped before READY".to_owned())),
        }
    })
    .await;
    let startup_error = match startup {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some(
            gateway_relay
                .as_ref()
                .and_then(GatewayRelay::latest_error)
                .map_or_else(
                    || "Discord gateway READY timed out after 30 seconds".to_owned(),
                    |error| format!("Discord gateway READY timed out after 30 seconds: {error}"),
                ),
        ),
    };
    if let Some(error) = startup_error {
        let _ = shutdown_tx.send(true);
        shard_manager.shutdown_all().await;
        finish_connection(&mut connection).await;
        if let Some(relay) = &mut gateway_relay {
            finish_connection(&mut relay.task).await;
        }
        return Err(error);
    }
    let stopper: Stopper = Arc::new(move || {
        let _ = shutdown_tx.send(true);
    });
    let outbound_sender = DiscordOutbound::new(home.clone(), group_id, http);
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
                let completes_processing = completes_processing(&event);
                let reply_to = processing_reply_to(&event).map(str::to_owned);
                for target in targets {
                    let key = target.key();
                    if let Err(error) = sender.send_target(&target, &event).await {
                        tracing::warn!(%error, "failed to send Discord IM message");
                        if completes_processing {
                            reactions.fail(&key, reply_to.as_deref()).await;
                        }
                    } else if completes_processing {
                        reactions.complete(&key, reply_to.as_deref()).await;
                    }
                }
            }
        },
    );
    let mut tasks = vec![connection, outbound, reaction_cleanup];
    if let Some(relay) = gateway_relay {
        tasks.push(relay.task);
    }
    Ok((tasks, stopper))
}

fn describe_gateway_error(error: &serenity::Error) -> String {
    match error {
        serenity::Error::Gateway(GatewayError::DisallowedGatewayIntents) => {
            "Discord gateway rejected MESSAGE_CONTENT intent; enable Message Content Intent in the Discord Developer Portal".into()
        }
        serenity::Error::Gateway(GatewayError::InvalidGatewayIntents) => {
            "Discord gateway rejected invalid intents".into()
        }
        serenity::Error::Gateway(GatewayError::InvalidAuthentication) => {
            "Discord gateway rejected the bot token; reset the token and update CCCC".into()
        }
        _ => format!("Discord gateway stopped before READY: {error}"),
    }
}

async fn finish_connection(connection: &mut JoinHandle<()>) {
    if tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut *connection)
        .await
        .is_err()
    {
        connection.abort();
        let _ = connection.await;
    }
}

struct Handler {
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: String,
    download_http: reqwest::Client,
    bot_user_id: UserId,
    reactions: DiscordReactions,
    deduper: Arc<super::discord_dedup::DiscordMessageDeduper>,
    ready: Arc<std::sync::Mutex<Option<oneshot::Sender<()>>>>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, context: Context, message: Message) {
        if message.author.bot {
            return;
        }
        if !self.deduper.accept(&self.group_id, message.id) {
            tracing::debug!(message_id = %message.id, "ignored duplicate Discord message");
            return;
        }
        let chat_id = message.channel_id.get().to_string();
        let raw_text = message.content.trim();
        let mut text = strip_leading_bot_mentions(raw_text, self.bot_user_id);
        let mut addressed = text != raw_text;
        if !addressed
            && let Some(guild_id) = message.guild_id
            && !message.mention_roles.is_empty()
        {
            match guild_id.member(&context.http, self.bot_user_id).await {
                Ok(member) => {
                    text = strip_leading_bot_role_mentions(text, &member.roles);
                    addressed = text != raw_text;
                }
                Err(error) => {
                    tracing::warn!(%error, %guild_id, "failed to resolve Discord bot roles");
                }
            }
        }
        if !accepts_channel_message(message.guild_id.is_none(), addressed, text) {
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
        }
        let processing_key = target_key(&chat_id, "");
        self.reactions.start(&processing_key, &message).await;
        let attachments = materialize_attachments(
            &self.home,
            &self.group_id,
            &self.download_http,
            &message.attachments,
        )
        .await;
        if text.is_empty() && attachments.is_empty() {
            self.reactions
                .fail_message(&processing_key, message.id)
                .await;
            return;
        }
        match dispatch_inbound_with(
            &self.daemon,
            &self.group_id,
            PLATFORM,
            &chat_id,
            &message.author.id.get().to_string(),
            text,
            InboundMetadata {
                message_id: message.id.to_string(),
                thread_id: String::new(),
                attachments,
            },
        )
        .await
        {
            Ok(source_event_id) => {
                self.reactions
                    .bind_message(&processing_key, message.id, source_event_id);
            }
            Err(error) => {
                tracing::warn!(%error, "failed to dispatch Discord IM message");
                self.reactions
                    .fail_message(&processing_key, message.id)
                    .await;
            }
        }
    }

    async fn ready(&self, _context: Context, ready: Ready) {
        if let Some(sender) = self
            .ready
            .lock()
            .expect("Discord READY signal poisoned")
            .take()
        {
            let _ = sender.send(());
        }
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

fn strip_leading_bot_role_mentions<'a>(raw: &'a str, bot_roles: &[RoleId]) -> &'a str {
    let mut text = raw.trim();
    loop {
        let Some((_, remainder)) = text
            .strip_prefix("<@&")
            .and_then(|value| value.split_once('>'))
            .and_then(|(id, remainder)| id.parse::<u64>().ok().map(|id| (id, remainder)))
            .filter(|(id, _)| bot_roles.contains(&RoleId::new(*id)))
        else {
            return text;
        };
        text = remainder.trim_start();
    }
}

fn accepts_channel_message(
    is_direct_message: bool,
    explicitly_addressed: bool,
    normalized_text: &str,
) -> bool {
    is_direct_message
        || explicitly_addressed
        || super::commands::is_recognized_command(normalized_text)
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
    fn strips_only_roles_assigned_to_the_bot() {
        assert_eq!(
            strip_leading_bot_role_mentions(
                " <@&456> <@&456> hello ",
                &[RoleId::new(123), RoleId::new(456)],
            ),
            "hello"
        );
        assert_eq!(
            strip_leading_bot_role_mentions("<@&789> hello", &[RoleId::new(456)]),
            "<@&789> hello"
        );
    }

    #[test]
    fn guilds_accept_mentions_and_commands_but_not_unaddressed_chat() {
        assert!(accepts_channel_message(false, true, "hello"));
        assert!(accepts_channel_message(false, false, "/subscribe"));
        assert!(!accepts_channel_message(false, false, "/weather"));
        assert!(!accepts_channel_message(false, false, "hello"));
        assert!(accepts_channel_message(true, false, "hello"));
    }

    #[test]
    fn gateway_errors_explain_required_user_actions() {
        let disallowed = serenity::Error::Gateway(GatewayError::DisallowedGatewayIntents);
        assert!(describe_gateway_error(&disallowed).contains("Message Content Intent"));

        let invalid_token = serenity::Error::Gateway(GatewayError::InvalidAuthentication);
        assert!(describe_gateway_error(&invalid_token).contains("reset the token"));
    }
}
