use super::{
    accepts_inbound, dispatch_inbound, outbound_text, resolve_credential, spawn_outbound, string,
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
) -> Result<Vec<JoinHandle<()>>, String> {
    let token = resolve_credential(&string(config, "bot_token_env"))?;
    let bot = Bot::new(token);
    bot.get_me()
        .await
        .map_err(|error| format!("Telegram credential verification failed: {error}"))?;

    let inbound_bot = bot.clone();
    let inbound_home = home.clone();
    let inbound_client = client.clone();
    let inbound_group = group_id.to_owned();
    let inbound = tokio::spawn(async move {
        let handler = Update::filter_message().endpoint(move |_bot: Bot, message: Message| {
            let home = inbound_home.clone();
            let client = inbound_client.clone();
            let group_id = inbound_group.clone();
            async move {
                let chat_id = message.chat.id.0.to_string();
                let Some(text) = message
                    .text()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                else {
                    return respond(());
                };
                if !accepts_inbound(&home, &group_id, PLATFORM, &chat_id, text) {
                    return respond(());
                }
                let sender = message
                    .from
                    .as_ref()
                    .map(|user| user.id.0.to_string())
                    .unwrap_or_else(|| "user".into());
                if let Err(error) =
                    dispatch_inbound(&client, &group_id, PLATFORM, &chat_id, &sender, text).await
                {
                    tracing::warn!(%error, "failed to dispatch Telegram IM message");
                }
                respond(())
            }
        });
        Dispatcher::builder(inbound_bot, handler)
            .build()
            .dispatch()
            .await;
    });

    let outbound = spawn_outbound(
        home,
        group_id.to_owned(),
        ledger_events,
        bot,
        |bot, targets, event| async move {
            let Some(body) = outbound_text(&event, false) else {
                return;
            };
            for chat_id in targets {
                let Ok(chat_id) = chat_id.parse::<i64>() else {
                    continue;
                };
                if let Err(error) = bot.send_message(ChatId(chat_id), &body).await {
                    tracing::warn!(%error, "failed to send Telegram IM message");
                }
            }
        },
    );
    Ok(vec![inbound, outbound])
}
