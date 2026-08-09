use super::wecom_media;
use cccc_core::{HomeLayout, blobs};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

pub(super) struct ParsedInbound {
    pub chat_id: String,
    pub sender: String,
    pub message_id: String,
    pub text: String,
    pub attachments: Vec<RemoteAttachment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RemoteAttachment {
    pub kind: String,
    pub url: String,
    pub aes_key: String,
    pub filename: String,
    pub mime_type: String,
    pub media_id: String,
}

#[derive(Default)]
pub(super) struct MessageDeduper {
    state: Mutex<DedupState>,
}

#[derive(Default)]
struct DedupState {
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl MessageDeduper {
    pub(super) fn accept(&self, chat_id: &str, message_id: &str) -> bool {
        let message_id = message_id.trim();
        if message_id.is_empty() {
            return true;
        }
        let key = format!("{chat_id}:{message_id}");
        let mut state = self.state.lock().expect("WeCom dedup registry poisoned");
        if !state.ids.insert(key.clone()) {
            return false;
        }
        state.order.push_back(key);
        while state.order.len() > 4096 {
            if let Some(expired) = state.order.pop_front() {
                state.ids.remove(&expired);
            }
        }
        true
    }
}

pub(super) fn parse_inbound(frame: &Value) -> Option<ParsedInbound> {
    let body = frame.get("body")?.as_object()?;
    let body = Value::Object(body.clone());
    let chat_id = text_at(&body, &["/chatid", "/from/userid"]);
    if chat_id.is_empty() {
        return None;
    }
    let sender = text_at(
        &body,
        &["/from/userid", "/sender/userid", "/sender/user_id"],
    );
    let message_id = text_at(&body, &["/msgid", "/msg_id"]);
    let msg_type = text_at(&body, &["/msgtype", "/msg_type"]);
    let mut attachments = Vec::new();
    let text = match msg_type.as_str() {
        "text" | "" => text_at(&body, &["/text/content", "/content/text"]),
        "mixed" => parse_mixed(&body, &mut attachments),
        "image" | "file" | "voice" | "video" => {
            attachments.push(media_attachment(&body, &msg_type));
            if msg_type == "file" {
                let filename = attachments[0].filename.trim();
                if filename.is_empty() {
                    "[file]".into()
                } else {
                    format!("[file: {filename}]")
                }
            } else if msg_type == "voice" {
                let content = text_at(&body, &["/voice/content"]);
                if content.trim().is_empty() {
                    "[voice]".to_owned()
                } else {
                    content
                }
            } else {
                format!("[{msg_type}]")
            }
        }
        other => format!("[{other}]"),
    };
    if !accepts_inbound(&body, &text) {
        return None;
    }
    attachments.retain(|attachment| !attachment.url.is_empty() || !attachment.media_id.is_empty());
    (!text.trim().is_empty() || !attachments.is_empty()).then_some(ParsedInbound {
        chat_id,
        sender: if sender.is_empty() {
            "user".into()
        } else {
            sender
        },
        message_id,
        text: if text.trim().is_empty() {
            "[attachment]".into()
        } else {
            text.trim().to_owned()
        },
        attachments,
    })
}

fn accepts_inbound(body: &Value, text: &str) -> bool {
    let chat_type = text_at(body, &["/chat_type", "/chattype"]).to_ascii_lowercase();
    let is_group = matches!(chat_type.as_str(), "group" | "group_chat" | "2");
    !is_group
        || super::commands::is_recognized_command(text)
        || ["/is_at_bot", "/is_mention_bot", "/is_at_bot_in_group"]
            .into_iter()
            .any(|path| truthy(body.pointer(path)))
}

fn truthy(value: Option<&Value>) -> bool {
    value.is_some_and(|value| match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_i64().is_some_and(|value| value != 0),
        Value::String(value) => matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"),
        _ => false,
    })
}

pub(super) async fn materialize_attachments(
    home: &HomeLayout,
    group_id: &str,
    attachments: &[RemoteAttachment],
) -> Vec<Value> {
    let mut result = Vec::new();
    for attachment in attachments {
        if attachment.url.is_empty() {
            tracing::warn!(
                media_id = %attachment.media_id,
                "ignored WeCom attachment without a download URL"
            );
            continue;
        }
        let downloaded = match wecom_media::download_file(
            &attachment.url,
            (!attachment.aes_key.is_empty()).then_some(attachment.aes_key.as_str()),
        )
        .await
        {
            Ok(downloaded) => downloaded,
            Err(error) => {
                tracing::warn!(%error, kind = %attachment.kind, "failed to download WeCom attachment");
                continue;
            }
        };
        let blob = match blobs::store(home, group_id, &downloaded.bytes) {
            Ok(blob) => blob,
            Err(error) => {
                tracing::warn!(%error, "failed to store WeCom attachment");
                continue;
            }
        };
        let title = if attachment.filename.trim().is_empty() {
            downloaded.filename
        } else {
            attachment.filename.clone()
        };
        let mime_type = if attachment.mime_type.trim().is_empty() {
            mime_guess::from_path(&title)
                .first_or_octet_stream()
                .to_string()
        } else {
            attachment.mime_type.clone()
        };
        result.push(json!({
            "kind":attachment.kind,
            "path":blob.path,
            "title":title,
            "mime_type":mime_type,
            "bytes":blob.bytes,
            "sha256":blob.sha256,
            "source_media_id":attachment.media_id
        }));
    }
    result
}

fn parse_mixed(body: &Value, attachments: &mut Vec<RemoteAttachment>) -> String {
    let mut text = Vec::new();
    let Some(items) = body.pointer("/mixed/msg_item").and_then(Value::as_array) else {
        return "[mixed]".into();
    };
    for item in items {
        let item_type = item
            .get("msgtype")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if item_type == "text" {
            if let Some(content) = item.pointer("/text/content").and_then(Value::as_str)
                && !content.trim().is_empty()
            {
                text.push(content.trim().to_owned());
            }
        } else if matches!(item_type, "image" | "file" | "voice" | "video") {
            attachments.push(media_attachment(item, item_type));
        }
    }
    if text.is_empty() {
        attachments
            .first()
            .map_or_else(|| "[mixed]".into(), |item| format!("[{}]", item.kind))
    } else {
        text.join("\n")
    }
}

fn media_attachment(value: &Value, kind: &str) -> RemoteAttachment {
    let prefix = format!("/{kind}");
    let at = |field: &str| {
        value
            .pointer(&format!("{prefix}/{field}"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let filename = ["filename", "file_name", "fileName", "name"]
        .into_iter()
        .map(at)
        .find(|value| !value.is_empty())
        .or_else(|| {
            ["fileName", "filename", "file_name"]
                .into_iter()
                .map(|field| {
                    value
                        .get(field)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_owned()
                })
                .find(|value| !value.is_empty())
        })
        .unwrap_or_default();
    RemoteAttachment {
        kind: kind.to_owned(),
        url: ["url", "download_url", "downloadUrl"]
            .into_iter()
            .map(at)
            .find(|value| !value.is_empty())
            .unwrap_or_default(),
        aes_key: ["aeskey", "aes_key", "decryption_key"]
            .into_iter()
            .map(at)
            .find(|value| !value.is_empty())
            .unwrap_or_default(),
        filename,
        mime_type: ["content_type", "contentType", "mime_type"]
            .into_iter()
            .map(at)
            .find(|value| !value.is_empty())
            .unwrap_or_default(),
        media_id: ["media_id", "mediaId"]
            .into_iter()
            .map(at)
            .find(|value| !value.is_empty())
            .unwrap_or_default(),
    }
}

fn text_at(value: &Value, paths: &[&str]) -> String {
    paths
        .iter()
        .filter_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_mixed_image_callbacks() {
        let text = parse_inbound(&json!({
            "body":{"chatid":"chat","msgid":"msg-1","msgtype":"text","from":{"userid":"user"},"text":{"content":"hello"}}
        }))
        .expect("text");
        assert_eq!(text.text, "hello");
        assert_eq!(text.message_id, "msg-1");

        let mixed = parse_inbound(&json!({
            "body":{"chatid":"chat","msgid":"msg-2","msgtype":"mixed","from":{"userid":"user"},
                "mixed":{"msg_item":[
                    {"msgtype":"text","text":{"content":"look"}},
                    {"msgtype":"image","image":{"url":"https://example.test/a.enc","aeskey":"key","filename":"a.png"}}
                ]}}
        }))
        .expect("mixed");
        assert_eq!(mixed.text, "look");
        assert_eq!(mixed.attachments.len(), 1);
        assert_eq!(mixed.attachments[0].filename, "a.png");
    }

    #[test]
    fn explicit_group_callbacks_require_a_command_or_bot_mention() {
        let frame = |text: &str, mentioned: bool| {
            json!({"body":{
                "chatid":"group-chat","chat_type":"group","is_at_bot":mentioned,
                "msgid":"msg","msgtype":"text","from":{"userid":"user"},
                "text":{"content":text}
            }})
        };

        assert!(parse_inbound(&frame("ambient message", false)).is_none());
        assert!(parse_inbound(&frame("/status", false)).is_some());
        assert!(parse_inbound(&frame("/weather", false)).is_none());
        assert!(parse_inbound(&frame("addressed message", true)).is_some());
    }

    #[test]
    fn deduplicates_bounded_message_ids() {
        let deduper = MessageDeduper::default();
        assert!(deduper.accept("chat", "msg"));
        assert!(!deduper.accept("chat", "msg"));
        assert!(deduper.accept("other", "msg"));
        assert!(deduper.accept("chat", ""));
    }
}
