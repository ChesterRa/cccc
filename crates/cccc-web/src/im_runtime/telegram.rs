use super::processing_reactions::TelegramReactions;
use super::telegram_inbound::{has_attachments, materialize_attachments};
use super::telegram_outbound::TelegramOutbound;
use super::worker::Stopper;
use super::{
    InboundDecision, InboundMetadata, completes_processing, dispatch_inbound_with,
    inbound_decision_for_thread, is_outbound_or_stream, processing_reply_to,
    resolve_config_credential, spawn_outbound_matching, target_key,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Map, Value};
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;
use teloxide::types::{MessageEntityKind, MessageEntityRef};
use tokio::task::JoinHandle;

const PLATFORM: &str = "telegram";

pub(super) async fn start(
    home: HomeLayout,
    client: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<(Vec<JoinHandle<()>>, Stopper), String> {
    let token = resolve_config_credential(config, "bot_token", "bot_token_env")?;
    let bot = Bot::new(token);
    let bot_username = bot
        .get_me()
        .await
        .map_err(|error| format!("Telegram credential verification failed: {error}"))?
        .username()
        .to_owned();
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
        let bot_username = bot_username.clone();
        async move {
            if !accepts_inbound_message(&message, &bot_username) {
                return respond(());
            }
            let chat_id = message.chat.id.0.to_string();
            let thread_id = message
                .thread_id
                .map(|thread_id| thread_id.0.0.to_string())
                .unwrap_or_default();
            let raw_text = message
                .text()
                .or_else(|| message.caption())
                .map(str::trim)
                .unwrap_or_default();
            let Some(text) = normalize_telegram_text(raw_text, &bot_username) else {
                return respond(());
            };
            if text.is_empty() && !has_attachments(&message) {
                return respond(());
            }
            match inbound_decision_for_thread(
                &home, &group_id, PLATFORM, &chat_id, &thread_id, text,
            )
            .await
            {
                InboundDecision::Forward => {}
                InboundDecision::Reply(body) => {
                    let request = bot.send_message(message.chat.id, body);
                    let result = match message.thread_id {
                        Some(thread_id) => request.message_thread_id(thread_id).await,
                        None => request.await,
                    };
                    if let Err(error) = result {
                        tracing::warn!(%error, "failed to send Telegram command reply");
                    }
                    return respond(());
                }
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
            let processing_key = target_key(&chat_id, &thread_id);
            reactions
                .start(&processing_key, message.chat.id, message.id)
                .await;
            match dispatch_inbound_with(
                &client,
                &group_id,
                PLATFORM,
                &chat_id,
                &sender,
                text,
                InboundMetadata {
                    message_id: format!("{}:{}", message.chat.id.0, message.id.0),
                    thread_id,
                    attachments,
                },
            )
            .await
            {
                Ok(source_event_id) => {
                    reactions.bind_message(&processing_key, message.id, source_event_id);
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to dispatch Telegram IM message");
                    reactions.abort_message(&processing_key, message.id).await;
                }
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

    let reaction_cleanup = reactions.cleanup_task();
    let outbound_reactions = reactions;
    let outbound_sender = TelegramOutbound::new(home.clone(), group_id, bot);
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
                    if let Err(error) = sender.send_target(&target, &event).await {
                        tracing::warn!(%error, "failed to send Telegram IM message");
                        if completes_processing {
                            reactions.complete(&target.key(), reply_to.as_deref()).await;
                        }
                    } else if completes_processing {
                        reactions.complete(&target.key(), reply_to.as_deref()).await;
                    }
                }
            }
        },
    );
    Ok((vec![inbound, outbound, reaction_cleanup], stopper))
}

fn accepts_inbound_message(message: &Message, bot_username: &str) -> bool {
    if message.chat.is_private() {
        return true;
    }
    let text = message
        .text()
        .or_else(|| message.caption())
        .and_then(|text| normalize_telegram_text(text, bot_username));
    text.is_some_and(super::commands::is_recognized_command)
        || text.is_some_and(|text| command_targets_bot(text, bot_username))
        || directed_entities(message)
            .iter()
            .any(|entity| mention_targets_bot(entity, bot_username))
}

fn command_targets_bot(text: &str, bot_username: &str) -> bool {
    text.split_whitespace()
        .next()
        .and_then(|command| command.strip_prefix('/'))
        .and_then(|command| command.split_once('@'))
        .is_some_and(|(_, target)| target.eq_ignore_ascii_case(bot_username))
}

fn normalize_telegram_text<'a>(raw: &'a str, bot_username: &str) -> Option<&'a str> {
    let mut text = raw.trim();
    while let Some((first, remainder)) = split_first_token(text)
        && first
            .strip_prefix('@')
            .is_some_and(|username| username.eq_ignore_ascii_case(bot_username))
    {
        text = remainder.trim_start();
    }
    let command = text.split_whitespace().next().unwrap_or_default();
    if command.starts_with('/')
        && let Some((_, target)) = command.split_once('@')
        && !target.eq_ignore_ascii_case(bot_username)
    {
        return None;
    }
    Some(text)
}

fn split_first_token(text: &str) -> Option<(&str, &str)> {
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    (!text.is_empty()).then(|| (&text[..end], &text[end..]))
}

fn directed_entities(message: &Message) -> Vec<MessageEntityRef<'_>> {
    message
        .parse_entities()
        .or_else(|| message.parse_caption_entities())
        .unwrap_or_default()
}

fn mention_targets_bot(entity: &MessageEntityRef<'_>, bot_username: &str) -> bool {
    matches!(entity.kind(), MessageEntityKind::Mention)
        && entity
            .text()
            .strip_prefix('@')
            .is_some_and(|username| username.eq_ignore_ascii_case(bot_username))
}

#[cfg(test)]
mod tests {
    use super::{accepts_inbound_message, normalize_telegram_text};
    use serde_json::{Value, json};
    use teloxide::types::Message;

    fn message(chat: Value, text: &str, entities: Value) -> Message {
        serde_json::from_value(json!({
            "message_id": 1,
            "date": 0,
            "chat": chat,
            "text": text,
            "entities": entities,
        }))
        .expect("Telegram message fixture")
    }

    fn private_message(text: &str) -> Message {
        message(
            json!({"id": 1, "type": "private", "first_name": "User"}),
            text,
            json!([]),
        )
    }

    fn group_message(text: &str, entities: Value) -> Message {
        message(
            json!({"id": -1, "type": "group", "title": "Group"}),
            text,
            entities,
        )
    }

    fn photo_message(chat: Value, caption: Option<&str>, entities: Value) -> Message {
        let mut value = json!({
            "message_id": 1,
            "date": 0,
            "chat": chat,
            "photo": [{
                "file_id": "photo-id",
                "file_unique_id": "photo-unique-id",
                "width": 1,
                "height": 1
            }],
        });
        if let Some(caption) = caption {
            value["caption"] = json!(caption);
            value["caption_entities"] = entities;
        }
        serde_json::from_value(value).expect("Telegram photo fixture")
    }

    #[test]
    fn private_chats_accept_ordinary_messages() {
        assert!(accepts_inbound_message(
            &private_message("ordinary text"),
            "cccc_bot"
        ));
    }

    #[test]
    fn ambient_group_messages_are_not_forwarded_or_replied_to() {
        assert!(!accepts_inbound_message(
            &group_message("ordinary text", json!([])),
            "cccc_bot"
        ));
    }

    #[test]
    fn group_attachments_require_an_explicit_bot_mention() {
        assert!(!accepts_inbound_message(
            &photo_message(
                json!({"id": -1, "type": "group", "title": "Group"}),
                None,
                json!([]),
            ),
            "cccc_bot"
        ));
        assert!(accepts_inbound_message(
            &photo_message(
                json!({"id": -1, "type": "group", "title": "Group"}),
                Some("@cccc_bot inspect"),
                json!([{"type": "mention", "offset": 0, "length": 9}]),
            ),
            "cccc_bot"
        ));
        assert!(accepts_inbound_message(
            &photo_message(
                json!({"id": 1, "type": "private", "first_name": "User"}),
                None,
                json!([]),
            ),
            "cccc_bot"
        ));
    }

    #[test]
    fn group_commands_and_explicit_bot_mentions_are_accepted() {
        assert!(accepts_inbound_message(
            &group_message(
                "/weather@cccc_bot",
                json!([{"type": "bot_command", "offset": 0, "length": 17}]),
            ),
            "cccc_bot"
        ));
        assert!(accepts_inbound_message(
            &group_message(
                "/subscribe@cccc_bot",
                json!([{"type": "bot_command", "offset": 0, "length": 19}]),
            ),
            "cccc_bot"
        ));
        assert!(accepts_inbound_message(
            &group_message(
                "hello @CCCC_BOT",
                json!([{"type": "mention", "offset": 6, "length": 9}]),
            ),
            "cccc_bot"
        ));
        assert!(!accepts_inbound_message(
            &group_message(
                "hello @another_bot",
                json!([{"type": "mention", "offset": 6, "length": 12}]),
            ),
            "cccc_bot"
        ));
        assert!(!accepts_inbound_message(
            &group_message(
                "/unsubscribe@another_bot",
                json!([{"type": "bot_command", "offset": 0, "length": 24}]),
            ),
            "cccc_bot"
        ));
        assert!(!accepts_inbound_message(
            &group_message(
                "/weather",
                json!([{"type": "bot_command", "offset": 0, "length": 8}]),
            ),
            "cccc_bot"
        ));
    }

    #[test]
    fn normalizes_only_messages_addressed_to_this_bot() {
        assert_eq!(
            normalize_telegram_text("@CCCC_BOT /send @all hello", "cccc_bot"),
            Some("/send @all hello")
        );
        assert_eq!(
            normalize_telegram_text("/subscribe@CCCC_BOT", "cccc_bot"),
            Some("/subscribe@CCCC_BOT")
        );
        assert_eq!(
            normalize_telegram_text("/unsubscribe@another_bot", "cccc_bot"),
            None
        );
        assert_eq!(
            normalize_telegram_text("@cccc_bot /unsubscribe@another_bot", "cccc_bot"),
            None
        );
    }
}
