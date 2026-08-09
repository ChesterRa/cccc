use dingtalk_stream::DingTalkStreamClient;
use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use teloxide::prelude::*;
use teloxide::requests::Request;
use teloxide::types::{MessageId, ReactionType};
use tokio::task::JoinHandle;

const FEISHU_PROCESSING_EMOJI: &str = "OnIt";
const TELEGRAM_PROCESSING_EMOJI: &str = "👀";
const DINGTALK_PROCESSING_EMOJI: &str = "🤔Thinking";
const DINGTALK_SUCCESS_EMOJI: &str = "🥳Done";
const DINGTALK_FAILURE_EMOJI: &str = "❌Failed";
const DINGTALK_API_BASE: &str = "https://api.dingtalk.com";
const PROCESSING_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const PROCESSING_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const REACTION_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn spawn_processing_cleanup<F, Fut>(cleanup: F) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PROCESSING_CLEANUP_INTERVAL);
        interval.tick().await;
        loop {
            interval.tick().await;
            cleanup().await;
        }
    })
}

pub(super) async fn reaction_request<T, E>(
    request: impl Future<Output = Result<T, E>>,
) -> Result<T, String>
where
    E: std::fmt::Display,
{
    tokio::time::timeout(REACTION_REQUEST_TIMEOUT, request)
        .await
        .map_err(|_| "processing reaction request timed out after 5 seconds".to_owned())?
        .map_err(|error| error.to_string())
}

#[derive(Clone)]
pub(super) struct Active<T>(Arc<Mutex<HashMap<String, VecDeque<Timed<T>>>>>);

struct Timed<T> {
    value: T,
    expires_at: Instant,
}

impl<T> Default for Active<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
}

impl<T> Active<T> {
    pub(super) fn push(&self, key: String, value: T) {
        self.0
            .lock()
            .expect("processing state poisoned")
            .entry(key)
            .or_default()
            .push_back(Timed {
                value,
                expires_at: Instant::now() + PROCESSING_TIMEOUT,
            });
    }

    pub(super) fn take_next(&self, key: &str) -> Option<T> {
        self.take_where(key, |_| true)
    }

    pub(super) fn take_where(&self, key: &str, predicate: impl Fn(&T) -> bool) -> Option<T> {
        let mut active = self.0.lock().expect("processing state poisoned");
        let queue = active.get_mut(key)?;
        let index = queue.iter().position(|item| predicate(&item.value))?;
        let value = queue.remove(index).map(|item| item.value);
        if queue.is_empty() {
            active.remove(key);
        }
        value
    }

    pub(super) fn update_where(
        &self,
        key: &str,
        predicate: impl Fn(&T) -> bool,
        update: impl FnOnce(&mut T),
    ) -> bool {
        let mut active = self.0.lock().expect("processing state poisoned");
        let Some(item) = active
            .get_mut(key)
            .and_then(|queue| queue.iter_mut().find(|item| predicate(&item.value)))
        else {
            return false;
        };
        update(&mut item.value);
        true
    }

    pub(super) fn len(&self, key: &str) -> usize {
        self.0
            .lock()
            .expect("processing state poisoned")
            .get(key)
            .map_or(0, VecDeque::len)
    }

    pub(super) fn take_expired(&self) -> Vec<T> {
        self.take_expired_at(Instant::now())
    }

    fn take_expired_at(&self, now: Instant) -> Vec<T> {
        let mut active = self.0.lock().expect("processing state poisoned");
        let mut expired = Vec::new();
        for queue in active.values_mut() {
            while queue.front().is_some_and(|item| item.expires_at <= now) {
                if let Some(item) = queue.pop_front() {
                    expired.push(item.value);
                }
            }
        }
        active.retain(|_, queue| !queue.is_empty());
        expired
    }
}

#[derive(Clone)]
pub(super) struct FeishuReactions {
    http: Client,
    api: OpenApiClient<ReqwestOpenApiTransport>,
    base_url: String,
    active: Active<FeishuReaction>,
}

#[derive(Clone)]
struct FeishuReaction {
    message_id: String,
    reaction_id: String,
    source_event_id: Option<String>,
}

impl FeishuReactions {
    pub(super) fn new(
        http: Client,
        api: OpenApiClient<ReqwestOpenApiTransport>,
        base_url: String,
    ) -> Self {
        Self {
            http,
            api,
            base_url,
            active: Active::default(),
        }
    }

    pub(super) async fn start(&self, key: &str, message_id: &str) {
        match reaction_request(self.add(message_id)).await {
            Ok(reaction_id) => {
                self.active.push(
                    key.to_owned(),
                    FeishuReaction {
                        message_id: message_id.to_owned(),
                        reaction_id,
                        source_event_id: None,
                    },
                );
            }
            Err(error) => {
                tracing::warn!(%error, %message_id, "failed to add Feishu processing reaction")
            }
        }
    }

    pub(super) fn bind_message(&self, key: &str, message_id: &str, source_event_id: String) {
        if source_event_id.is_empty() {
            return;
        }
        self.active.update_where(
            key,
            |reaction| reaction.message_id == message_id,
            |reaction| reaction.source_event_id = Some(source_event_id),
        );
    }

    pub(super) async fn complete(&self, key: &str, reply_to: Option<&str>) {
        let reaction = take_for_reply(&self.active, key, reply_to, |reaction| {
            reaction.source_event_id.as_deref()
        });
        self.remove_active(reaction).await;
    }

    pub(super) async fn abort_message(&self, key: &str, message_id: &str) {
        let reaction = self
            .active
            .take_where(key, |reaction| reaction.message_id == message_id);
        self.remove_active(reaction).await;
    }

    pub(super) fn cleanup_task(&self) -> JoinHandle<()> {
        let reactions = self.clone();
        spawn_processing_cleanup(move || {
            let reactions = reactions.clone();
            async move {
                for reaction in reactions.active.take_expired() {
                    reactions.remove_active(Some(reaction)).await;
                }
            }
        })
    }

    async fn remove_active(&self, reaction: Option<FeishuReaction>) {
        let Some(reaction) = reaction else {
            return;
        };
        if let Err(error) = reaction_request(self.remove(&reaction)).await {
            tracing::warn!(%error, message_id = %reaction.message_id, "failed to remove Feishu processing reaction");
        }
    }

    async fn add(&self, message_id: &str) -> Result<String, String> {
        let token = self
            .api
            .tenant_access_token()
            .await
            .map_err(|error| error.to_string())?;
        let url = feishu_reaction_url(&self.base_url, message_id, None)?;
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&json!({
                "reaction_type":{"emoji_type":FEISHU_PROCESSING_EMOJI}
            }))
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let value: Value = response.json().await.map_err(|error| error.to_string())?;
        if !status.is_success() || value["code"].as_i64() != Some(0) {
            return Err(format!(
                "HTTP {status}: {}",
                value["msg"].as_str().unwrap_or("reaction rejected")
            ));
        }
        value
            .pointer("/data/reaction_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| "Feishu reaction response has no reaction_id".into())
    }

    async fn remove(&self, reaction: &FeishuReaction) -> Result<(), String> {
        let token = self
            .api
            .tenant_access_token()
            .await
            .map_err(|error| error.to_string())?;
        let url = feishu_reaction_url(
            &self.base_url,
            &reaction.message_id,
            Some(&reaction.reaction_id),
        )?;
        let response = self
            .http
            .delete(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let value: Value = response.json().await.map_err(|error| error.to_string())?;
        if status.is_success() && value["code"].as_i64() == Some(0) {
            Ok(())
        } else {
            Err(format!(
                "HTTP {status}: {}",
                value["msg"].as_str().unwrap_or("reaction removal rejected")
            ))
        }
    }
}

fn feishu_reaction_url(
    base_url: &str,
    message_id: &str,
    reaction_id: Option<&str>,
) -> Result<Url, String> {
    let suffix = reaction_id.map_or_else(String::new, |id| format!("/{id}"));
    Url::parse(&format!(
        "{}/open-apis/im/v1/messages/{message_id}/reactions{suffix}",
        base_url.trim_end_matches('/')
    ))
    .map_err(|error| error.to_string())
}

#[derive(Clone)]
pub(super) struct TelegramReactions {
    bot: Bot,
    active: Active<TelegramReaction>,
}

#[derive(Clone)]
struct TelegramReaction {
    chat_id: ChatId,
    message_id: MessageId,
    source_event_id: Option<String>,
}
impl TelegramReactions {
    pub(super) fn new(bot: Bot) -> Self {
        Self {
            bot,
            active: Active::default(),
        }
    }

    pub(super) async fn start(&self, key: &str, chat_id: ChatId, message_id: MessageId) {
        let reaction = ReactionType::Emoji {
            emoji: TELEGRAM_PROCESSING_EMOJI.into(),
        };
        match reaction_request(
            self.bot
                .set_message_reaction(chat_id, message_id)
                .reaction([reaction])
                .send(),
        )
        .await
        {
            Ok(_) => {
                self.active.push(
                    key.to_owned(),
                    TelegramReaction {
                        chat_id,
                        message_id,
                        source_event_id: None,
                    },
                );
            }
            Err(error) => tracing::warn!(%error, "failed to add Telegram processing reaction"),
        }
    }

    pub(super) fn bind_message(&self, key: &str, message_id: MessageId, source_event_id: String) {
        if source_event_id.is_empty() {
            return;
        }
        self.active.update_where(
            key,
            |reaction| reaction.message_id == message_id,
            |reaction| reaction.source_event_id = Some(source_event_id),
        );
    }

    pub(super) async fn complete(&self, key: &str, reply_to: Option<&str>) {
        let reaction = take_for_reply(&self.active, key, reply_to, |reaction| {
            reaction.source_event_id.as_deref()
        });
        self.remove_active(reaction).await;
    }

    pub(super) async fn abort_message(&self, key: &str, message_id: MessageId) {
        let reaction = self
            .active
            .take_where(key, |reaction| reaction.message_id == message_id);
        self.remove_active(reaction).await;
    }

    pub(super) fn cleanup_task(&self) -> JoinHandle<()> {
        let reactions = self.clone();
        spawn_processing_cleanup(move || {
            let reactions = reactions.clone();
            async move {
                for reaction in reactions.active.take_expired() {
                    reactions.remove_active(Some(reaction)).await;
                }
            }
        })
    }

    async fn remove_active(&self, reaction: Option<TelegramReaction>) {
        let Some(reaction) = reaction else {
            return;
        };
        if let Err(error) = reaction_request(
            self.bot
                .set_message_reaction(reaction.chat_id, reaction.message_id)
                .send(),
        )
        .await
        {
            tracing::warn!(%error, "failed to remove Telegram processing reaction");
        }
    }
}

#[derive(Clone)]
pub(super) struct DingTalkReactions {
    http: Client,
    api: Arc<DingTalkStreamClient>,
    robot_code: String,
    active: Active<DingTalkReaction>,
}

#[derive(Clone)]
struct DingTalkReaction {
    message_id: String,
    conversation_id: String,
    source_event_id: Option<String>,
}

impl DingTalkReactions {
    pub(super) fn new(api: Arc<DingTalkStreamClient>, robot_code: String) -> Self {
        Self {
            http: Client::new(),
            api,
            robot_code,
            active: Active::default(),
        }
    }

    pub(super) async fn start(&self, key: &str, conversation_id: &str, message_id: &str) {
        if message_id.is_empty() {
            return;
        }
        let reaction = DingTalkReaction {
            message_id: message_id.into(),
            conversation_id: conversation_id.into(),
            source_event_id: None,
        };
        match reaction_request(self.send(&reaction, DINGTALK_PROCESSING_EMOJI, false)).await {
            Ok(()) => {
                self.active.push(key.into(), reaction);
            }
            Err(error) => {
                tracing::warn!(%error, %message_id, "failed to add DingTalk processing reaction")
            }
        }
    }

    pub(super) fn bind_message(&self, key: &str, message_id: &str, source_event_id: String) {
        if source_event_id.is_empty() {
            return;
        }
        self.active.update_where(
            key,
            |reaction| reaction.message_id == message_id,
            |reaction| reaction.source_event_id = Some(source_event_id),
        );
    }

    pub(super) async fn complete(&self, key: &str, reply_to: Option<&str>) {
        let reaction = take_for_reply(&self.active, key, reply_to, |reaction| {
            reaction.source_event_id.as_deref()
        });
        self.finish(reaction, Some(DINGTALK_SUCCESS_EMOJI)).await;
    }

    pub(super) async fn fail(&self, key: &str, reply_to: Option<&str>) {
        let reaction = take_for_reply(&self.active, key, reply_to, |reaction| {
            reaction.source_event_id.as_deref()
        });
        self.finish(reaction, Some(DINGTALK_FAILURE_EMOJI)).await;
    }

    pub(super) async fn fail_message(&self, key: &str, message_id: &str) {
        let reaction = self
            .active
            .take_where(key, |reaction| reaction.message_id == message_id);
        self.finish(reaction, Some(DINGTALK_FAILURE_EMOJI)).await;
    }

    pub(super) fn cleanup_task(&self) -> JoinHandle<()> {
        let reactions = self.clone();
        spawn_processing_cleanup(move || {
            let reactions = reactions.clone();
            async move {
                for reaction in reactions.active.take_expired() {
                    reactions
                        .finish(Some(reaction), Some(DINGTALK_FAILURE_EMOJI))
                        .await;
                }
            }
        })
    }

    async fn finish(&self, reaction: Option<DingTalkReaction>, replacement: Option<&str>) {
        let Some(reaction) = reaction else {
            return;
        };
        if let Err(error) =
            reaction_request(self.send(&reaction, DINGTALK_PROCESSING_EMOJI, true)).await
        {
            tracing::warn!(%error, message_id = %reaction.message_id, "failed to recall DingTalk processing reaction");
        }
        if let Some(replacement) = replacement
            && let Err(error) = reaction_request(self.send(&reaction, replacement, false)).await
        {
            tracing::warn!(%error, message_id = %reaction.message_id, "failed to add DingTalk completion reaction");
        }
    }

    async fn send(
        &self,
        reaction: &DingTalkReaction,
        emoji: &str,
        recall: bool,
    ) -> Result<(), String> {
        let token = self
            .api
            .get_access_token()
            .await
            .map_err(|error| error.to_string())?;
        let (url, payload) = dingtalk_reaction_request(
            DINGTALK_API_BASE,
            &self.robot_code,
            &reaction.message_id,
            &reaction.conversation_id,
            emoji,
            recall,
        )?;
        let response = self
            .http
            .post(url)
            .header("x-acs-dingtalk-access-token", token)
            .json(&payload)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(format!(
                "HTTP {status}: {}",
                body.chars().take(300).collect::<String>()
            ))
        }
    }
}

fn take_for_reply<T>(
    active: &Active<T>,
    key: &str,
    reply_to: Option<&str>,
    source_event_id: impl for<'a> Fn(&'a T) -> Option<&'a str>,
) -> Option<T> {
    match reply_to.map(str::trim).filter(|value| !value.is_empty()) {
        Some(reply_to) => active.take_where(key, |item| source_event_id(item) == Some(reply_to)),
        None if active.len(key) == 1 => active.take_next(key),
        None => None,
    }
}

fn dingtalk_reaction_request(
    base_url: &str,
    robot_code: &str,
    message_id: &str,
    conversation_id: &str,
    emoji: &str,
    recall: bool,
) -> Result<(Url, Value), String> {
    let action = if recall { "recall" } else { "reply" };
    let url = Url::parse(&format!(
        "{}/v1.0/robot/emotion/{action}",
        base_url.trim_end_matches('/')
    ))
    .map_err(|error| error.to_string())?;
    Ok((
        url,
        json!({
            "robotCode":robot_code,"openMsgId":message_id,"openConversationId":conversation_id,
            "emotionType":2,"emotionName":emoji,
            "textEmotion":{"emotionId":"2659900","emotionName":emoji,"text":emoji,"backgroundId":"im_bg_1"}
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_feishu_reaction_urls() {
        assert_eq!(
            feishu_reaction_url("https://open.feishu.cn/", "om_1", None)
                .expect("Feishu create-reaction URL should be valid")
                .as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_1/reactions"
        );
        assert_eq!(
            feishu_reaction_url("https://open.feishu.cn", "om_1", Some("r_1"))
                .expect("Feishu delete-reaction URL should be valid")
                .as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_1/reactions/r_1"
        );
    }

    #[test]
    fn builds_dingtalk_processing_and_recall_requests() {
        let (url, payload) = dingtalk_reaction_request(
            "https://api.dingtalk.com",
            "robot",
            "msg",
            "cid",
            DINGTALK_PROCESSING_EMOJI,
            false,
        )
        .expect("DingTalk processing-reaction request should be valid");
        assert_eq!(url.path(), "/v1.0/robot/emotion/reply");
        assert_eq!(payload["emotionName"], DINGTALK_PROCESSING_EMOJI);
        let (url, _) = dingtalk_reaction_request(
            "https://api.dingtalk.com",
            "robot",
            "msg",
            "cid",
            DINGTALK_PROCESSING_EMOJI,
            true,
        )
        .expect("DingTalk recall-reaction request should be valid");
        assert_eq!(url.path(), "/v1.0/robot/emotion/recall");
    }

    #[test]
    fn active_processing_is_queued_and_can_remove_a_specific_item() {
        let active = Active::default();
        active.push("chat".into(), "first");
        active.push("chat".into(), "second");
        active.push("chat".into(), "third");
        assert_eq!(
            active.take_where("chat", |item| *item == "second"),
            Some("second")
        );
        assert_eq!(active.take_next("chat"), Some("first"));
        assert_eq!(active.take_next("chat"), Some("third"));
        assert_eq!(active.take_next("chat"), None);
    }

    #[test]
    fn processing_completion_is_correlated_and_not_fifo_guessed() {
        #[derive(Debug, PartialEq)]
        struct Pending {
            message: &'static str,
            source_event_id: Option<&'static str>,
        }

        let active = Active::default();
        active.push(
            "chat".into(),
            Pending {
                message: "first",
                source_event_id: Some("event-1"),
            },
        );
        active.push(
            "chat".into(),
            Pending {
                message: "second",
                source_event_id: Some("event-2"),
            },
        );

        assert_eq!(
            take_for_reply(&active, "chat", Some("event-1"), |item| item
                .source_event_id),
            Some(Pending {
                message: "first",
                source_event_id: Some("event-1"),
            })
        );
        assert_eq!(
            take_for_reply(&active, "chat", Some("event-1"), |item| item
                .source_event_id),
            None
        );
        assert_eq!(
            take_for_reply(&active, "chat", Some("event-2"), |item| item
                .source_event_id),
            Some(Pending {
                message: "second",
                source_event_id: Some("event-2"),
            })
        );
    }

    #[test]
    fn expired_processing_state_is_removed_from_the_registry() {
        let active = Active::default();
        active.push("chat".into(), "pending");

        assert_eq!(
            active.take_expired_at(Instant::now() + PROCESSING_TIMEOUT + Duration::from_secs(1)),
            vec!["pending"]
        );
        assert_eq!(active.len("chat"), 0);
    }
}
