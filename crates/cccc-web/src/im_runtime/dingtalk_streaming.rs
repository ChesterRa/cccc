use super::dingtalk_outbound::DingTalkTarget;
use super::outbound_chunks::fits_message;
use super::outbound_text;
use async_trait::async_trait;
use cccc_contracts::Event;
use dingtalk_stream::DingTalkStreamClient;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const OPENAPI_BASE: &str = "https://api.dingtalk.com";
const CREATE_ENDPOINT: &str = "/v1.0/card/instances/createAndDeliver";
const STREAM_ENDPOINT: &str = "/v1.0/card/streaming";
const AI_CARD_TEMPLATE_ID: &str = "382e4302-551d-4880-bf29-a30acfab2e71.schema";
const STREAM_THROTTLE: Duration = Duration::from_millis(300);
const CARD_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_ACTIVE_STREAMS: usize = 1_024;
const MAX_COMPLETED_STREAMS: usize = 4_096;

type StreamKey = (String, String);

#[async_trait]
trait DingTalkCardToken: Send + Sync {
    async fn access_token(&self) -> Result<String, String>;
}

#[async_trait]
impl DingTalkCardToken for DingTalkStreamClient {
    async fn access_token(&self) -> Result<String, String> {
        self.get_access_token()
            .await
            .map_err(|error| error.to_string())
    }
}

struct ActiveCard {
    out_track_id: String,
    last_update: Option<Instant>,
}

pub(super) struct DingTalkCardStreamer {
    token: Arc<dyn DingTalkCardToken>,
    http: reqwest::Client,
    openapi_base: String,
    robot_code: String,
    active: Mutex<HashMap<StreamKey, ActiveCard>>,
    completed: Mutex<HashSet<StreamKey>>,
    throttle: Duration,
}

impl DingTalkCardStreamer {
    pub(super) fn new(client: Arc<DingTalkStreamClient>, robot_code: String) -> Self {
        Self {
            token: client,
            http: reqwest::Client::new(),
            openapi_base: OPENAPI_BASE.into(),
            robot_code,
            active: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashSet::new()),
            throttle: STREAM_THROTTLE,
        }
    }

    pub(super) async fn send(&self, targets: &[DingTalkTarget], event: &Event) {
        let op = event_string(event, "op");
        let stream_id = event_string(event, "stream_id");
        if stream_id.is_empty() || !matches!(op.as_str(), "start" | "update" | "end") {
            return;
        }
        let raw_text = event
            .data
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = outbound_text(event, true).unwrap_or_default();
        let preview_body = if raw_text.is_empty() {
            format!("{body}…")
        } else {
            body.clone()
        };
        let text = prepare_stream_text(&preview_body);
        let complete = !raw_text.is_empty() && text == body && fits_message(&body, 4_096, Some(64));

        for target in targets {
            let key = (stream_id.clone(), target.chat_id.clone());
            match op.as_str() {
                "start" => self.start_target(key, target, &text).await,
                "update" => self.update_target(&key, &text).await,
                "end" => self.end_target(key, &text, complete).await,
                _ => unreachable!("stream operation was validated"),
            }
        }
    }

    pub(super) fn take_completed_targets(&self, stream_id: &str) -> HashSet<String> {
        if stream_id.is_empty() {
            return HashSet::new();
        }
        self.active
            .lock()
            .expect("DingTalk active card registry poisoned")
            .retain(|(candidate, _), _| candidate != stream_id);
        let mut completed = self
            .completed
            .lock()
            .expect("DingTalk completed card registry poisoned");
        let targets = completed
            .iter()
            .filter(|(candidate, _)| candidate == stream_id)
            .map(|(_, chat_id)| chat_id.clone())
            .collect::<HashSet<_>>();
        completed.retain(|(candidate, _)| candidate != stream_id);
        targets
    }

    async fn start_target(&self, key: StreamKey, target: &DingTalkTarget, text: &str) {
        if self
            .active
            .lock()
            .expect("DingTalk active card registry poisoned")
            .contains_key(&key)
        {
            return;
        }
        match self.create_card(target, text).await {
            Ok(out_track_id) => {
                self.active
                    .lock()
                    .expect("DingTalk active card registry poisoned")
                    .insert(
                        key,
                        ActiveCard {
                            out_track_id,
                            last_update: None,
                        },
                    );
                self.trim_active();
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    stream_id = %key.0,
                    chat_id = %key.1,
                    "failed to create DingTalk AI Card; final message will be used"
                );
            }
        }
    }

    async fn update_target(&self, key: &StreamKey, text: &str) {
        let out_track_id = {
            let active = self
                .active
                .lock()
                .expect("DingTalk active card registry poisoned");
            let Some(card) = active.get(key) else {
                return;
            };
            if card
                .last_update
                .is_some_and(|last| last.elapsed() < self.throttle)
            {
                return;
            }
            card.out_track_id.clone()
        };
        match self.update_card(&out_track_id, text, false).await {
            Ok(()) => {
                if let Some(card) = self
                    .active
                    .lock()
                    .expect("DingTalk active card registry poisoned")
                    .get_mut(key)
                    .filter(|card| card.out_track_id == out_track_id)
                {
                    card.last_update = Some(Instant::now());
                }
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    stream_id = %key.0,
                    chat_id = %key.1,
                    "failed to update DingTalk AI Card frame"
                );
            }
        }
    }

    async fn end_target(&self, key: StreamKey, text: &str, complete: bool) {
        let Some(card) = self
            .active
            .lock()
            .expect("DingTalk active card registry poisoned")
            .remove(&key)
        else {
            return;
        };
        match self.update_card(&card.out_track_id, text, true).await {
            Ok(()) if complete && !text.is_empty() => self.mark_completed(key),
            Ok(()) => {}
            Err(error) => {
                tracing::warn!(
                    %error,
                    stream_id = %key.0,
                    chat_id = %key.1,
                    "failed to finalize DingTalk AI Card; final message will be used"
                );
            }
        }
    }

    async fn create_card(&self, target: &DingTalkTarget, text: &str) -> Result<String, String> {
        let out_track_id = uuid::Uuid::new_v4().simple().to_string();
        let (open_space_id, delivery_key, delivery) = card_route(target, &self.robot_code)?;
        let mut payload = json!({
            "cardTemplateId":AI_CARD_TEMPLATE_ID,
            "outTrackId":out_track_id,
            "cardData":{"cardParamMap":{
                "msgContent":text,
                "msgTitle":"CCCC",
                "flowStatus":"1"
            }},
            "callbackType":"STREAM",
            "openSpaceId":open_space_id,
            "imGroupOpenSpaceModel":{"supportForward":true},
            "imRobotOpenSpaceModel":{"supportForward":true}
        });
        payload[delivery_key] = delivery;
        self.request(Method::POST, CREATE_ENDPOINT, &payload)
            .await?;
        Ok(out_track_id)
    }

    async fn update_card(
        &self,
        out_track_id: &str,
        text: &str,
        finalize: bool,
    ) -> Result<(), String> {
        self.request(
            Method::PUT,
            STREAM_ENDPOINT,
            &json!({
                "outTrackId":out_track_id,
                "guid":uuid::Uuid::new_v4().simple().to_string(),
                "key":"msgContent",
                "content":text,
                "isFull":true,
                "isFinalize":finalize,
                "isError":false
            }),
        )
        .await
    }

    async fn request(&self, method: Method, endpoint: &str, payload: &Value) -> Result<(), String> {
        let token = self.token.access_token().await?;
        if token.trim().is_empty() {
            return Err("DingTalk access token is empty".into());
        }
        let response = self
            .http
            .request(
                method,
                format!("{}{}", self.openapi_base.trim_end_matches('/'), endpoint),
            )
            .header("x-acs-dingtalk-access-token", token)
            .timeout(CARD_REQUEST_TIMEOUT)
            .json(payload)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let body = response.bytes().await.map_err(|error| error.to_string())?;
        validate_card_response(status, &body)
    }

    fn trim_active(&self) {
        let mut active = self
            .active
            .lock()
            .expect("DingTalk active card registry poisoned");
        while active.len() > MAX_ACTIVE_STREAMS {
            let Some(key) = active.keys().next().cloned() else {
                break;
            };
            active.remove(&key);
        }
    }

    fn mark_completed(&self, key: StreamKey) {
        let mut completed = self
            .completed
            .lock()
            .expect("DingTalk completed card registry poisoned");
        completed.insert(key);
        while completed.len() > MAX_COMPLETED_STREAMS {
            let Some(key) = completed.iter().next().cloned() else {
                break;
            };
            completed.remove(&key);
        }
    }
}

fn card_route(
    target: &DingTalkTarget,
    default_robot_code: &str,
) -> Result<(String, &'static str, Value), String> {
    match target.conversation_type.as_str() {
        "1" if !target.user_id.trim().is_empty() => Ok((
            format!("dtv1.card//IM_ROBOT.{}", target.user_id.trim()),
            "imRobotOpenDeliverModel",
            json!({"spaceType":"IM_ROBOT"}),
        )),
        "1" => Err("DingTalk direct-chat card target is missing userId".into()),
        "2" if !target.chat_id.trim().is_empty() => {
            let robot_code = if target.robot_code.trim().is_empty() {
                default_robot_code.trim()
            } else {
                target.robot_code.trim()
            };
            if robot_code.is_empty() {
                return Err("DingTalk group card target is missing robotCode".into());
            }
            Ok((
                format!("dtv1.card//IM_GROUP.{}", target.chat_id.trim()),
                "imGroupOpenDeliverModel",
                json!({"robotCode":robot_code}),
            ))
        }
        "2" => Err("DingTalk group card target is missing chatId".into()),
        "" => Err("DingTalk card target is missing conversationType".into()),
        other => Err(format!(
            "unsupported DingTalk card conversationType: {other}"
        )),
    }
}

fn event_string(event: &Event, key: &str) -> String {
    event
        .data
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn prepare_stream_text(value: &str) -> String {
    let normalized = value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "  ");
    let lines = normalized.lines().map(str::trim_end).collect::<Vec<_>>();
    let Some(first) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return String::new();
    };
    let last = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .expect("non-empty line exists");
    let mut kept = Vec::new();
    let mut previous_blank = false;
    for line in &lines[first..=last] {
        let blank = line.trim().is_empty();
        if !blank || !previous_blank {
            kept.push(*line);
        }
        previous_blank = blank;
        if kept.len() == 64 {
            break;
        }
    }
    let output = kept.join("\n").trim().to_owned();
    if output.chars().count() <= 4_096 {
        output
    } else {
        output.chars().take(4_095).chain(['…']).collect()
    }
}

fn validate_card_response(status: StatusCode, body: &[u8]) -> Result<(), String> {
    if !status.is_success() {
        return Err(format!("DingTalk Card OpenAPI HTTP {status}"));
    }
    if body.is_empty() {
        return Ok(());
    }
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| format!("DingTalk Card OpenAPI returned non-JSON: {error}"))?;
    if value.get("success").and_then(Value::as_bool) == Some(false)
        || nonzero_code(value.get("code"))
        || nonzero_code(value.get("errcode"))
    {
        return Err(value
            .get("message")
            .or_else(|| value.get("errmsg"))
            .and_then(Value::as_str)
            .unwrap_or("DingTalk Card OpenAPI rejected the request")
            .to_owned());
    }
    Ok(())
}

fn nonzero_code(value: Option<&Value>) -> bool {
    value.is_some_and(|value| match value {
        Value::Number(number) => number.as_i64() != Some(0),
        Value::String(code) => code != "0",
        Value::Null => false,
        _ => true,
    })
}

#[cfg(test)]
#[path = "dingtalk_streaming_tests.rs"]
mod tests;
