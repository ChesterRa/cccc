use super::dingtalk_outbound_media::AttachmentMedia;
use super::dingtalk_outbound_report::{AttachmentDeliveryReport, persist_failures};
use super::outbound_attachment::safe_filename;
use super::outbound_chunks::split_message;
use cccc_core::HomeLayout;
use dingtalk_stream::DingTalkStreamClient;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
const OPENAPI_BASE: &str = "https://api.dingtalk.com";
const OTO_ENDPOINT: &str = "/v1.0/robot/oToMessages/batchSend";
const GROUP_ENDPOINT: &str = "/v1.0/robot/groupMessages/send";
const MAX_MESSAGE_CHARS: usize = 4_096;
const MAX_MESSAGE_LINES: usize = 64;
type TargetRoute<'a> = (&'static str, &'a str, bool);

pub(super) struct DingTalkOutboundSender {
    home: HomeLayout,
    group_id: String,
    media: Arc<dyn AttachmentMedia>,
    http: reqwest::Client,
    openapi_base: String,
    robot_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DingTalkTarget {
    pub(super) chat_id: String,
    pub(super) robot_code: String,
    pub(super) conversation_type: String,
    pub(super) user_id: String,
}

impl DingTalkOutboundSender {
    pub(super) fn new(
        home: HomeLayout,
        group_id: &str,
        media: Arc<DingTalkStreamClient>,
        robot_code: String,
    ) -> Self {
        Self {
            home,
            group_id: group_id.into(),
            media,
            http: reqwest::Client::new(),
            openapi_base: OPENAPI_BASE.into(),
            robot_code,
        }
    }

    pub(super) async fn send_attachments(
        &self,
        targets: &[DingTalkTarget],
        attachments: &[Value],
    ) -> AttachmentDeliveryReport {
        let mut report = AttachmentDeliveryReport::default();
        let mut routes = Vec::new();
        for target in targets {
            match route_target(target) {
                Ok(route) => routes.push((target, route)),
                Err(error) => {
                    report.failed_chat_ids.insert(target.chat_id.clone());
                    report.fail("route", error);
                }
            }
        }
        for attachment in attachments {
            let item = match self.prepare(attachment).await {
                Ok(item) => item,
                Err(error) => {
                    mark_routes_failed(&mut report, &routes);
                    report.fail("prepare", error);
                    continue;
                }
            };
            if routes.is_empty() {
                continue;
            }
            let file_type = if item.is_image { "image" } else { "file" };
            let media_id = match self
                .media
                .upload(&item.raw, file_type, &item.title, &item.mime)
                .await
            {
                Ok(media_id) => media_id,
                Err(error) => {
                    mark_routes_failed(&mut report, &routes);
                    report.fail("upload", error);
                    continue;
                }
            };
            let message = attachment_payload(&media_id, &item.title, item.is_image);
            let token = match self.media.access_token().await {
                Ok(token) => token,
                Err(error) => {
                    mark_routes_failed(&mut report, &routes);
                    report.fail("send", error);
                    continue;
                }
            };
            self.deliver(&token, &routes, &message, &mut report).await;
        }
        persist_failures(&self.home, &self.group_id, &report);
        report
    }

    pub(super) async fn send_text(
        &self,
        targets: &[DingTalkTarget],
        text: &str,
    ) -> HashSet<String> {
        let mut delivered = HashSet::new();
        if text.trim().is_empty() {
            return delivered;
        }
        let routes = targets
            .iter()
            .filter_map(|target| match route_target(target) {
                Ok(route) => Some((target, route)),
                Err(error) => {
                    tracing::warn!(%error, chat_id = %target.chat_id, "failed to route DingTalk text fallback");
                    None
                }
            })
            .collect::<Vec<_>>();
        if routes.is_empty() {
            return delivered;
        }
        let token = match self.media.access_token().await {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(%error, "failed to authorize DingTalk text fallback");
                return delivered;
            }
        };
        for (target, route) in routes {
            let mut target_delivered = false;
            let mut target_failed = false;
            for chunk in split_message(text, MAX_MESSAGE_CHARS, Some(MAX_MESSAGE_LINES)) {
                let message = markdown_payload(&chunk);
                if let Err(error) = self.post(&token, target, &route, &message).await {
                    tracing::warn!(%error, chat_id = %target.chat_id, "failed to send DingTalk text fallback");
                    target_failed = true;
                } else {
                    target_delivered = true;
                }
            }
            if target_delivered && !target_failed {
                delivered.insert(target.chat_id.clone());
            }
        }
        delivered
    }

    async fn prepare(&self, value: &Value) -> Result<PreparedAttachment, String> {
        let relative = value
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "attachment path is missing".to_owned())?;
        let path = cccc_core::blobs::resolve(&self.home, &self.group_id, relative)
            .map_err(|error| error.to_string())?;
        if path.metadata().map_err(|error| error.to_string())?.len() > MAX_ATTACHMENT_BYTES {
            return Err("attachment exceeds 10 MiB before read".into());
        }
        let raw = tokio::fs::read(&path)
            .await
            .map_err(|error| error.to_string())?;
        validate_loaded_size(&raw)?;
        let title = value
            .get("title")
            .and_then(Value::as_str)
            .and_then(safe_filename)
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .unwrap_or("file")
            .to_owned();
        let mime = value
            .get("mime_type")
            .and_then(Value::as_str)
            .filter(|mime| !mime.trim().is_empty())
            .or_else(|| mime_guess::from_path(&path).first_raw())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let is_image = mime.starts_with("image/");
        Ok(PreparedAttachment {
            raw,
            title,
            mime,
            is_image,
        })
    }

    async fn deliver(
        &self,
        token: &str,
        routes: &[(&DingTalkTarget, TargetRoute<'_>)],
        message: &Value,
        report: &mut AttachmentDeliveryReport,
    ) {
        for (target, route) in routes {
            match self.post(token, target, route, message).await {
                Ok(()) => {
                    report.delivered_targets += 1;
                    report.delivered_chat_ids.insert(target.chat_id.clone());
                }
                Err(error) => {
                    report.failed_chat_ids.insert(target.chat_id.clone());
                    report.fail("send", error);
                }
            }
        }
    }

    async fn post(
        &self,
        token: &str,
        target: &DingTalkTarget,
        route: &TargetRoute<'_>,
        message: &Value,
    ) -> Result<(), String> {
        let robot = match target.robot_code.trim() {
            "" => self.robot_code.trim(),
            robot => robot,
        };
        if robot.is_empty() {
            return Err("robotCode is missing".into());
        }
        let (endpoint, id, is_oto) = route;
        let mut payload = json!({
            "robotCode":robot,
            "msgKey":message["msgKey"],
            "msgParam":message["msgParam"],
        });
        payload[if *is_oto {
            "userIds"
        } else {
            "openConversationId"
        }] = if *is_oto { json!([id]) } else { json!(id) };
        // No idempotency key exists, so ambiguous failures are persisted rather than retried.
        let url = format!("{}{}", self.openapi_base.trim_end_matches('/'), endpoint);
        let response = self
            .http
            .post(url)
            .header("x-acs-dingtalk-access-token", token)
            .json(&payload)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let body = response.bytes().await.map_err(|error| error.to_string())?;
        validate_openapi_response(status, &body)
    }
}

fn mark_routes_failed(
    report: &mut AttachmentDeliveryReport,
    routes: &[(&DingTalkTarget, TargetRoute<'_>)],
) {
    report
        .failed_chat_ids
        .extend(routes.iter().map(|(target, _)| target.chat_id.clone()));
}

fn route_target(target: &DingTalkTarget) -> Result<TargetRoute<'_>, String> {
    match target.conversation_type.as_str() {
        "1" if !target.user_id.trim().is_empty() => Ok((OTO_ENDPOINT, target.user_id.trim(), true)),
        "1" => Err("OTO target is missing userId".into()),
        "2" if !target.chat_id.trim().is_empty() => {
            Ok((GROUP_ENDPOINT, target.chat_id.trim(), false))
        }
        "2" => Err("group target is missing chatId".into()),
        "" => Err("conversationType is missing".into()),
        other => Err(format!("unsupported conversationType: {other}")),
    }
}

fn validate_openapi_response(status: StatusCode, body: &[u8]) -> Result<(), String> {
    if !status.is_success() {
        return Err(format!("DingTalk OpenAPI HTTP {status}"));
    }
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| format!("DingTalk OpenAPI returned non-JSON: {error}"))?;
    let results = value.get("sendResults").and_then(Value::as_array);
    match results {
        Some(items) if !items.is_empty() && items.iter().all(send_result_succeeded) => Ok(()),
        Some(_) => Err("DingTalk OpenAPI sendResults contain a failure".into()),
        None if value
            .get("processQueryKey")
            .and_then(Value::as_str)
            .is_some_and(|key| !key.trim().is_empty()) =>
        {
            Ok(())
        }
        None => Err("DingTalk OpenAPI response has no success marker".into()),
    }
}

fn send_result_succeeded(value: &Value) -> bool {
    let success = value
        .get("success")
        .map(|success| success.as_bool() == Some(true));
    let code = value
        .get("code")
        .map(|code| code.as_i64() == Some(0) || code.as_str() == Some("0"));
    let status = value.get("status").map(|status| {
        status
            .as_str()
            .is_some_and(|value| matches!(value.to_ascii_uppercase().as_str(), "OK" | "SUCCESS"))
    });
    [success, code, status]
        .into_iter()
        .flatten()
        .reduce(|accepted, field| accepted && field)
        == Some(true)
}

fn validate_loaded_size(raw: &[u8]) -> Result<(), String> {
    (raw.len() as u64 <= MAX_ATTACHMENT_BYTES)
        .then_some(())
        .ok_or_else(|| "attachment exceeds 10 MiB after read".into())
}

struct PreparedAttachment {
    raw: Vec<u8>,
    title: String,
    mime: String,
    is_image: bool,
}

fn attachment_payload(media_id: &str, filename: &str, is_image: bool) -> Value {
    let (key, params) = if is_image {
        ("sampleImageMsg", json!({"photoURL":media_id}))
    } else {
        (
            "sampleFile",
            json!({"mediaId":media_id,"fileName":filename}),
        )
    };
    let params = serde_json::to_string(&params).expect("DingTalk message params must serialize");
    json!({"msgKey":key,"msgParam":params})
}

fn markdown_payload(text: &str) -> Value {
    let params = serde_json::to_string(&json!({"title":"CCCC","text":text}))
        .expect("DingTalk markdown params must serialize");
    json!({"msgKey":"sampleMarkdown","msgParam":params})
}

#[cfg(test)]
#[path = "dingtalk_outbound_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dingtalk_outbound_contract_tests.rs"]
mod contract_tests;
