use super::processing_reactions::{Active, reaction_request, spawn_processing_cleanup};
use serenity::all::{ChannelId, MessageId, ReactionType};
use serenity::http::Http;
use std::sync::Arc;
use tokio::task::JoinHandle;

const PROCESSING_EMOJI: &str = "👀";
const SUCCESS_EMOJI: &str = "✅";
const FAILURE_EMOJI: &str = "❌";

#[derive(Clone)]
pub(super) struct DiscordReactions {
    http: Arc<Http>,
    active: Active<DiscordReaction>,
}

#[derive(Clone)]
struct DiscordReaction {
    channel_id: ChannelId,
    message_id: MessageId,
    source_event_id: Option<String>,
}

impl DiscordReactions {
    pub(super) fn new(http: Arc<Http>) -> Self {
        Self {
            http,
            active: Active::default(),
        }
    }

    pub(super) async fn start(&self, chat_id: &str, message: &serenity::all::Message) {
        let reaction = DiscordReaction {
            channel_id: message.channel_id,
            message_id: message.id,
            source_event_id: None,
        };
        match reaction_request(reaction.channel_id.create_reaction(
            self.http.as_ref(),
            reaction.message_id,
            unicode(PROCESSING_EMOJI),
        ))
        .await
        {
            Ok(()) => {
                self.active.push(chat_id.to_owned(), reaction);
            }
            Err(error) => {
                tracing::warn!(%error, message_id = %message.id, "failed to add Discord processing reaction");
            }
        }
    }

    pub(super) fn bind_message(
        &self,
        chat_id: &str,
        message_id: MessageId,
        source_event_id: String,
    ) {
        if source_event_id.is_empty() {
            return;
        }
        self.active.update_where(
            chat_id,
            |reaction| reaction.message_id == message_id,
            |reaction| reaction.source_event_id = Some(source_event_id),
        );
    }

    pub(super) async fn complete(&self, chat_id: &str, reply_to: Option<&str>) {
        self.finish_reply(chat_id, reply_to, SUCCESS_EMOJI, "completion")
            .await;
    }

    pub(super) async fn fail(&self, chat_id: &str, reply_to: Option<&str>) {
        self.finish_reply(chat_id, reply_to, FAILURE_EMOJI, "failure")
            .await;
    }

    pub(super) async fn fail_message(&self, chat_id: &str, message_id: MessageId) {
        let reaction = self
            .active
            .take_where(chat_id, |reaction| reaction.message_id == message_id);
        self.finish(reaction, FAILURE_EMOJI, "failure").await;
    }

    pub(super) fn cleanup_task(&self) -> JoinHandle<()> {
        let reactions = self.clone();
        spawn_processing_cleanup(move || {
            let reactions = reactions.clone();
            async move {
                for reaction in reactions.active.take_expired() {
                    reactions
                        .finish(Some(reaction), FAILURE_EMOJI, "timeout")
                        .await;
                }
            }
        })
    }

    async fn finish_reply(
        &self,
        chat_id: &str,
        reply_to: Option<&str>,
        final_emoji: &str,
        outcome: &str,
    ) {
        let reaction = match reply_to.map(str::trim).filter(|value| !value.is_empty()) {
            Some(reply_to) => self.active.take_where(chat_id, |reaction| {
                reaction.source_event_id.as_deref() == Some(reply_to)
            }),
            None if self.active.len(chat_id) == 1 => self.active.take_next(chat_id),
            None => None,
        };
        self.finish(reaction, final_emoji, outcome).await;
    }

    async fn finish(&self, reaction: Option<DiscordReaction>, final_emoji: &str, outcome: &str) {
        let Some(reaction) = reaction else {
            return;
        };
        self.replace(reaction, final_emoji, outcome).await;
    }

    async fn replace(&self, reaction: DiscordReaction, final_emoji: &str, outcome: &str) {
        if let Err(error) = reaction_request(reaction.channel_id.delete_reaction(
            self.http.as_ref(),
            reaction.message_id,
            None,
            unicode(PROCESSING_EMOJI),
        ))
        .await
        {
            tracing::warn!(%error, message_id = %reaction.message_id, "failed to remove Discord processing reaction");
        }
        if let Err(error) = reaction_request(reaction.channel_id.create_reaction(
            self.http.as_ref(),
            reaction.message_id,
            unicode(final_emoji),
        ))
        .await
        {
            tracing::warn!(%error, message_id = %reaction.message_id, %outcome, "failed to add Discord final reaction");
        }
    }
}

fn unicode(emoji: &str) -> ReactionType {
    ReactionType::Unicode(emoji.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reactions() -> DiscordReactions {
        DiscordReactions::new(Arc::new(Http::new("token")))
    }

    fn reaction(message_id: u64) -> DiscordReaction {
        DiscordReaction {
            channel_id: ChannelId::new(1),
            message_id: MessageId::new(message_id),
            source_event_id: None,
        }
    }

    #[test]
    fn tracks_bursts_in_fifo_order() {
        let reactions = reactions();
        reactions.active.push("channel".into(), reaction(1));
        reactions.active.push("channel".into(), reaction(2));
        assert_eq!(
            reactions
                .active
                .take_next("channel")
                .map(|item| item.message_id),
            Some(MessageId::new(1))
        );
        assert_eq!(
            reactions
                .active
                .take_next("channel")
                .map(|item| item.message_id),
            Some(MessageId::new(2))
        );
        assert!(reactions.active.take_next("channel").is_none());
    }

    #[test]
    fn inbound_failure_removes_only_its_message() {
        let reactions = reactions();
        reactions.active.push("channel".into(), reaction(1));
        reactions.active.push("channel".into(), reaction(2));
        assert_eq!(
            reactions
                .active
                .take_where("channel", |reaction| reaction.message_id
                    == MessageId::new(2))
                .map(|item| item.message_id),
            Some(MessageId::new(2))
        );
        assert_eq!(
            reactions
                .active
                .take_next("channel")
                .map(|item| item.message_id),
            Some(MessageId::new(1))
        );
    }
}
