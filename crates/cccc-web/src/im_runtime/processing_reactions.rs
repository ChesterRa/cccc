use dingtalk_stream::DingTalkStreamClient;
use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use teloxide::prelude::*;
use teloxide::types::{MessageId, ReactionType};

const FEISHU_PROCESSING_EMOJI: &str = "OnIt";
const TELEGRAM_PROCESSING_EMOJI: &str = "👀";
const DINGTALK_PROCESSING_EMOJI: &str = "🤔Thinking";
const DINGTALK_SUCCESS_EMOJI: &str = "🥳Done";
const DINGTALK_API_BASE: &str = "https://api.dingtalk.com";

#[derive(Clone)]
struct Active<T>(Arc<Mutex<HashMap<String, T>>>);

impl<T> Default for Active<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
}

impl<T> Active<T> {
    fn insert(&self, chat_id: String, value: T) {
        self.0
            .lock()
            .expect("processing state poisoned")
            .insert(chat_id, value);
    }

    fn take(&self, chat_id: &str) -> Option<T> {
        self.0
            .lock()
            .expect("processing state poisoned")
            .remove(chat_id)
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

    pub(super) async fn start(&self, chat_id: &str, message_id: &str) {
        match self.add(message_id).await {
            Ok(reaction_id) => self.active.insert(
                chat_id.to_owned(),
                FeishuReaction {
                    message_id: message_id.to_owned(),
                    reaction_id,
                },
            ),
            Err(error) => {
                tracing::warn!(%error, %message_id, "failed to add Feishu processing reaction")
            }
        }
    }

    pub(super) async fn complete(&self, chat_id: &str) {
        let Some(reaction) = self.active.take(chat_id) else {
            return;
        };
        if let Err(error) = self.remove(&reaction).await {
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
    active: Active<(ChatId, MessageId)>,
}

impl TelegramReactions {
    pub(super) fn new(bot: Bot) -> Self {
        Self {
            bot,
            active: Active::default(),
        }
    }

    pub(super) async fn start(&self, chat_id: ChatId, message_id: MessageId) {
        let reaction = ReactionType::Emoji {
            emoji: TELEGRAM_PROCESSING_EMOJI.into(),
        };
        match self
            .bot
            .set_message_reaction(chat_id, message_id)
            .reaction([reaction])
            .await
        {
            Ok(_) => self
                .active
                .insert(chat_id.0.to_string(), (chat_id, message_id)),
            Err(error) => tracing::warn!(%error, "failed to add Telegram processing reaction"),
        }
    }

    pub(super) async fn complete(&self, chat_id: &str) {
        let Some((chat_id, message_id)) = self.active.take(chat_id) else {
            return;
        };
        if let Err(error) = self.bot.set_message_reaction(chat_id, message_id).await {
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

    pub(super) async fn start(&self, conversation_id: &str, message_id: &str) {
        if message_id.is_empty() {
            return;
        }
        let reaction = DingTalkReaction {
            message_id: message_id.into(),
            conversation_id: conversation_id.into(),
        };
        match self.send(&reaction, DINGTALK_PROCESSING_EMOJI, false).await {
            Ok(()) => self.active.insert(conversation_id.into(), reaction),
            Err(error) => {
                tracing::warn!(%error, %message_id, "failed to add DingTalk processing reaction")
            }
        }
    }

    pub(super) async fn complete(&self, conversation_id: &str) {
        let Some(reaction) = self.active.take(conversation_id) else {
            return;
        };
        if let Err(error) = self.send(&reaction, DINGTALK_PROCESSING_EMOJI, true).await {
            tracing::warn!(%error, message_id = %reaction.message_id, "failed to recall DingTalk processing reaction");
        }
        if let Err(error) = self.send(&reaction, DINGTALK_SUCCESS_EMOJI, false).await {
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
    fn active_processing_is_replaced_per_chat_and_taken_once() {
        let active = Active::default();
        active.insert("chat".into(), "first");
        active.insert("chat".into(), "second");
        assert_eq!(active.take("chat"), Some("second"));
        assert_eq!(active.take("chat"), None);
    }
}
