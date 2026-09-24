use super::*;
use cccc_core::GroupStore;
use std::sync::Mutex;

#[derive(Default)]
struct FakeSender {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl WeixinSender for FakeSender {
    fn context_token(&self, user_id: &str) -> Option<String> {
        Some(format!("token:{user_id}"))
    }

    async fn send_text(
        &self,
        user_id: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<(), String> {
        self.calls.lock().expect("calls").push(format!(
            "text:{user_id}:{text}:{}",
            context_token.unwrap_or_default()
        ));
        Ok(())
    }

    async fn send_media(
        &self,
        user_id: &str,
        path: &Path,
        context_token: Option<&str>,
    ) -> Result<(), String> {
        assert!(path.exists());
        self.calls.lock().expect("calls").push(format!(
            "media:{user_id}:{}:{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
            context_token.unwrap_or_default()
        ));
        Ok(())
    }
}

fn setup() -> (tempfile::TempDir, WeixinOutbound, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("weixin", "")
        .expect("group");
    let attachment = super::super::inbound_attachments::store_bytes(
        &home,
        &group.group_id,
        b"image",
        super::super::inbound_attachments::AttachmentSpec::new("image", "source.png", "image/png"),
    )
    .expect("blob");
    (
        temp,
        WeixinOutbound {
            home,
            group_id: group.group_id,
            sdk: None,
        },
        attachment["path"].as_str().expect("path").to_owned(),
    )
}

#[tokio::test]
async fn sends_sender_title_then_attachment_with_original_filename() {
    let (_temp, outbound, path) = setup();
    let sender = FakeSender::default();
    let event: Event = serde_json::from_value(serde_json::json!({
        "v":1,"id":"event","ts":"now","kind":"chat.message",
        "group_id":"group","scope_key":"","by":"assistant",
        "data":{"text":"result","sender_title":"Helpful Assistant","attachments":[{
            "path":path,"title":"photo.png","mime_type":"image/png","bytes":5
        }]}
    }))
    .expect("event");

    outbound
        .send_with(&sender, &["wx-user".into()], &event)
        .await;

    assert_eq!(
        *sender.calls.lock().expect("calls"),
        vec![
            "text:wx-user:Helpful Assistant\n\nresult:token:wx-user",
            "media:wx-user:photo.png:token:wx-user"
        ]
    );
}

/// The iLink rejection for a user whose context token was never received or restored.
struct TokenlessSender;

#[async_trait]
impl WeixinSender for TokenlessSender {
    fn context_token(&self, _user_id: &str) -> Option<String> {
        None
    }

    async fn send_text(&self, user_id: &str, _: &str, _: Option<&str>) -> Result<(), String> {
        Err(format!(
            "No context_token for user {user_id}. A message from this user must be received first."
        ))
    }

    async fn send_media(&self, _: &str, _: &Path, _: Option<&str>) -> Result<(), String> {
        unreachable!("no attachment is sent")
    }
}

#[tokio::test]
async fn a_rejected_reply_is_recorded_in_the_group_im_log() {
    let (_temp, outbound, _path) = setup();
    let event: Event = serde_json::from_value(serde_json::json!({
        "v":1,"id":"event","ts":"now","kind":"chat.message",
        "group_id":"group","scope_key":"","by":"assistant",
        "data":{"text":"result","to":["user"]}
    }))
    .expect("event");

    outbound
        .send_with(&TokenlessSender, &["wx-user".into()], &event)
        .await;

    let log = std::fs::read_to_string(
        GroupStore::new(outbound.home.clone())
            .expect("store")
            .state_dir(&outbound.group_id)
            .expect("state dir")
            .join("im_bridge.log"),
    )
    .expect("the failure reaches the IM log");
    let line: Value = serde_json::from_str(log.trim()).expect("one JSON line");
    assert_eq!(line["platform"], "weixin");
    assert_eq!(line["operation"], "send_text");
    assert_eq!(line["user_id"], "wx-user");
    assert_eq!(line["has_context_token"], false);
    assert!(
        line["error"]
            .as_str()
            .is_some_and(|error| error.contains("must be received first"))
    );
}

#[tokio::test]
async fn rejects_attachment_title_with_path_components() {
    let (_temp, outbound, path) = setup();
    let prepared = outbound
        .prepare(&serde_json::json!({"path":path,"title":"../photo.png"}))
        .await
        .expect("prepared");

    assert_eq!(prepared.title, "file");
    assert_eq!(
        prepared.path.file_name().and_then(|name| name.to_str()),
        Some("file")
    );
}

#[tokio::test]
async fn long_unicode_reply_is_sent_in_lossless_chunks() {
    let (_temp, outbound, _path) = setup();
    let sender = FakeSender::default();
    let text = "你".repeat(5_000);
    let event: Event = serde_json::from_value(serde_json::json!({
        "v":1,"id":"event","ts":"now","kind":"chat.message",
        "group_id":"group","scope_key":"","by":"assistant",
        "data":{"text":text,"sender_title":"Helpful Assistant","to":["user"]}
    }))
    .expect("event");

    outbound
        .send_with(&sender, &["wx-user".into()], &event)
        .await;

    let calls = sender.calls.lock().expect("calls");
    let chunks = calls
        .iter()
        .map(|call| {
            call.strip_prefix("text:wx-user:")
                .and_then(|call| call.strip_suffix(":token:wx-user"))
                .expect("text call")
        })
        .collect::<Vec<_>>();
    assert!(chunks.len() > 1);
    assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 4_000));
    assert_eq!(chunks.concat(), format!("Helpful Assistant\n\n{text}"));
}
