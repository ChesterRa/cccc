use super::processing_reactions::TelegramReactions;
use super::telegram_inbound::{has_attachments, materialize_attachments};
use super::worker::Stopper;
use super::{
    InboundDecision, InboundMetadata, dispatch_inbound_with, inbound_decision, outbound_text,
    resolve_credential, spawn_outbound, string,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Map, Value};
use teloxide::prelude::*;
use tokio::task::JoinHandle;

const PLATFORM: &str = "telegram";

pub(super) async fn start(
    home: HomeLayout,
    client: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<(Vec<JoinHandle<()>>, Stopper), String> {
    let token = resolve_credential(&string(config, "bot_token_env"))?;
    let bot = Bot::new(token);
    bot.get_me()
        .await
        .map_err(|error| format!("Telegram credential verification failed: {error}"))?;
    let reactions = TelegramReactions::new(bot.clone());

    let inbound_bot = bot.clone();
    let inbound_home = home.clone();
    let inbound_client = client.clone();
    let inbound_group = group_id.to_owned();
    let inbound_reactions = reactions.clone();
    let handler = Update::filter_message().endpoint(move |bot: Bot, message: Message| {
        let home = inbound_home.clone();
        let client = inbound_client.clone();
        let group_id = inbound_group.clone();
        let reactions = inbound_reactions.clone();
        async move {
            let chat_id = message.chat.id.0.to_string();
            let text = message
                .text()
                .or_else(|| message.caption())
                .map(str::trim)
                .unwrap_or_default();
            if text.is_empty() && !has_attachments(&message) {
                return respond(());
            }
            match inbound_decision(&home, &group_id, PLATFORM, &chat_id, text).await {
                InboundDecision::Forward => {}
                InboundDecision::Reply(body) => {
                    if let Err(error) = bot.send_message(message.chat.id, body).await {
                        tracing::warn!(%error, "failed to send Telegram command reply");
                    }
                    return respond(());
                }
                InboundDecision::Ignore => return respond(()),
            }
            let sender = message
                .from
                .as_ref()
                .map(|user| user.id.0.to_string())
                .unwrap_or_else(|| "user".into());
            let attachments = materialize_attachments(&home, &group_id, &bot, &message).await;
            if text.is_empty() && attachments.is_empty() {
                return respond(());
            }
            if let Err(error) = dispatch_inbound_with(
                &client,
                &group_id,
                PLATFORM,
                &chat_id,
                &sender,
                text,
                InboundMetadata {
                    message_id: format!("{}:{}", message.chat.id.0, message.id.0),
                    attachments,
                },
            )
            .await
            {
                tracing::warn!(%error, "failed to dispatch Telegram IM message");
            } else {
                reactions.start(message.chat.id, message.id).await;
            }
            respond(())
        }
    });
    let mut dispatcher = Dispatcher::builder(inbound_bot, handler).build();
    let shutdown_token = dispatcher.shutdown_token();
    let inbound = tokio::spawn(async move {
        dispatcher.dispatch().await;
    });
    let stopper: Stopper = std::sync::Arc::new(move || {
        // Calling shutdown signals the dispatcher; its returned future only waits for completion.
        let _ = shutdown_token.shutdown();
    });

    let outbound_reactions = reactions;
    let outbound = spawn_outbound(
        home,
        group_id.to_owned(),
        ledger_events,
        bot,
        move |bot, targets, event| {
            let reactions = outbound_reactions.clone();
            async move {
                let Some(body) = outbound_text(&event, false) else {
                    return;
                };
                for chat_id in targets {
                    let Ok(chat_id) = chat_id.parse::<i64>() else {
                        continue;
                    };
                    if let Err(error) = bot.send_message(ChatId(chat_id), &body).await {
                        tracing::warn!(%error, "failed to send Telegram IM message");
                    } else {
                        reactions.complete(&chat_id.to_string()).await;
                    }
                }
            }
        },
    );
    Ok((vec![inbound, outbound], stopper))
}
