use serenity::all::{ChannelId, MessageId, ReactionType};
use serenity::http::Http;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

const PROCESSING_EMOJI: &str = "👀";
const SUCCESS_EMOJI: &str = "✅";
const FAILURE_EMOJI: &str = "❌";

#[derive(Clone)]
pub(super) struct DiscordReactions {
    http: Arc<Http>,
    active: Arc<Mutex<HashMap<String, VecDeque<DiscordReaction>>>>,
}

#[derive(Clone, Copy)]
struct DiscordReaction {
    channel_id: ChannelId,
    message_id: MessageId,
}

impl DiscordReactions {
    pub(super) fn new(http: Arc<Http>) -> Self {
        Self {
            http,
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(super) async fn start(&self, chat_id: &str, message: &serenity::all::Message) {
        let reaction = DiscordReaction {
            channel_id: message.channel_id,
            message_id: message.id,
        };
        match reaction
            .channel_id
            .create_reaction(
                self.http.as_ref(),
                reaction.message_id,
                unicode(PROCESSING_EMOJI),
            )
            .await
        {
            Ok(()) => {
                self.push(chat_id, reaction);
            }
            Err(error) => {
                tracing::warn!(%error, message_id = %message.id, "failed to add Discord processing reaction");
            }
        }
    }

    pub(super) async fn complete(&self, chat_id: &str) {
        self.finish_next(chat_id, SUCCESS_EMOJI, "completion").await;
    }

    pub(super) async fn fail(&self, chat_id: &str) {
        self.finish_next(chat_id, FAILURE_EMOJI, "failure").await;
    }

    pub(super) async fn fail_message(&self, chat_id: &str, message_id: MessageId) {
        let reaction = self.take_message(chat_id, message_id);
        self.finish(reaction, FAILURE_EMOJI, "failure").await;
    }

    async fn finish_next(&self, chat_id: &str, final_emoji: &str, outcome: &str) {
        let reaction = self.take_next(chat_id);
        self.finish(reaction, final_emoji, outcome).await;
    }

    async fn finish(&self, reaction: Option<DiscordReaction>, final_emoji: &str, outcome: &str) {
        let Some(reaction) = reaction else {
            return;
        };
        self.replace(reaction, final_emoji, outcome).await;
    }

    async fn replace(&self, reaction: DiscordReaction, final_emoji: &str, outcome: &str) {
        if let Err(error) = reaction
            .channel_id
            .delete_reaction(
                self.http.as_ref(),
                reaction.message_id,
                None,
                unicode(PROCESSING_EMOJI),
            )
            .await
        {
            tracing::warn!(%error, message_id = %reaction.message_id, "failed to remove Discord processing reaction");
        }
        if let Err(error) = reaction
            .channel_id
            .create_reaction(
                self.http.as_ref(),
                reaction.message_id,
                unicode(final_emoji),
            )
            .await
        {
            tracing::warn!(%error, message_id = %reaction.message_id, %outcome, "failed to add Discord final reaction");
        }
    }

    fn push(&self, chat_id: &str, reaction: DiscordReaction) {
        self.active
            .lock()
            .expect("Discord reaction state poisoned")
            .entry(chat_id.to_owned())
            .or_default()
            .push_back(reaction);
    }

    fn take_next(&self, chat_id: &str) -> Option<DiscordReaction> {
        let reaction = self
            .active
            .lock()
            .expect("Discord reaction state poisoned")
            .get_mut(chat_id)
            .and_then(VecDeque::pop_front);
        self.remove_empty(chat_id);
        reaction
    }

    fn take_message(&self, chat_id: &str, message_id: MessageId) -> Option<DiscordReaction> {
        let reaction = self
            .active
            .lock()
            .expect("Discord reaction state poisoned")
            .get_mut(chat_id)
            .and_then(|queue| {
                let index = queue
                    .iter()
                    .position(|reaction| reaction.message_id == message_id)?;
                queue.remove(index)
            });
        self.remove_empty(chat_id);
        reaction
    }

    fn remove_empty(&self, chat_id: &str) {
        let mut active = self.active.lock().expect("Discord reaction state poisoned");
        if active.get(chat_id).is_some_and(VecDeque::is_empty) {
            active.remove(chat_id);
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
        }
    }

    #[test]
    fn tracks_bursts_in_fifo_order() {
        let reactions = reactions();
        reactions.push("channel", reaction(1));
        reactions.push("channel", reaction(2));
        assert_eq!(
            reactions.take_next("channel").map(|item| item.message_id),
            Some(MessageId::new(1))
        );
        assert_eq!(
            reactions.take_next("channel").map(|item| item.message_id),
            Some(MessageId::new(2))
        );
        assert!(reactions.take_next("channel").is_none());
    }

    #[test]
    fn inbound_failure_removes_only_its_message() {
        let reactions = reactions();
        reactions.push("channel", reaction(1));
        reactions.push("channel", reaction(2));
        assert_eq!(
            reactions
                .take_message("channel", MessageId::new(2))
                .map(|item| item.message_id),
            Some(MessageId::new(2))
        );
        assert_eq!(
            reactions.take_next("channel").map(|item| item.message_id),
            Some(MessageId::new(1))
        );
    }
}
