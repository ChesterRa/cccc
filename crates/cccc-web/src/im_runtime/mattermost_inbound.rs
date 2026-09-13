use super::inbound_attachments::{AttachmentSpec, MAX_ATTACHMENT_BYTES, store_stream};
use super::mattermost::{MattermostApi, MattermostReactions, PLATFORM, field, valid_id};
use super::{
    InboundDecision, InboundMetadata, dispatch_inbound_with, inbound_decision_for_thread,
    target_key,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) struct MattermostInbound {
    home: HomeLayout,
    group_id: String,
    daemon: DaemonClient,
    api: MattermostApi,
    reactions: MattermostReactions,
    users: HashMap<String, bool>,
    seen: HashSet<String>,
    order: VecDeque<String>,
    files_enabled: bool,
    max_file_bytes: u64,
}

impl MattermostInbound {
    pub(super) fn new(
        home: HomeLayout,
        group_id: &str,
        daemon: DaemonClient,
        api: MattermostApi,
        reactions: MattermostReactions,
        config: &Map<String, Value>,
    ) -> Self {
        let files = config.get("files");
        let max_mb = files
            .and_then(|v| v.get("max_mb"))
            .and_then(Value::as_u64)
            .unwrap_or(10);
        Self {
            home,
            group_id: group_id.to_owned(),
            daemon,
            api,
            reactions,
            users: HashMap::new(),
            seen: HashSet::new(),
            order: VecDeque::new(),
            files_enabled: files
                .and_then(|v| v.get("enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
            max_file_bytes: max_mb.saturating_mul(1024 * 1024).min(MAX_ATTACHMENT_BYTES),
        }
    }

    pub(super) async fn handle(&mut self, event: &Value) -> Result<(), String> {
        if field(event, "event") != "posted" {
            return Ok(());
        }
        let data = &event["data"];
        let post: Value = serde_json::from_str(field(data, "post"))
            .map_err(|_| "Invalid Mattermost post JSON")?;
        if !eligible_post(&post, &self.api.bot_id) || self.seen.contains(field(&post, "id")) {
            return Ok(());
        }
        let sender = field(&post, "user_id");
        let chat_id = field(&post, "channel_id");
        let post_id = field(&post, "id");
        let thread_id = field(&post, "root_id");
        let raw = field(&post, "message");
        let text = strip_leading_mention(raw, &self.api.username);
        let has_files = post["file_ids"].as_array().is_some_and(|v| !v.is_empty());
        if text.is_empty() && !has_files {
            return Ok(());
        }
        let mut channel_type = field(data, "channel_type").to_owned();
        if !matches!(channel_type.as_str(), "O" | "P" | "D" | "G") {
            let channel = self
                .api
                .json(Method::GET, &format!("channels/{chat_id}"), None)
                .await?;
            channel_type = field(&channel, "type").to_owned();
        }
        if !accepts_message(&channel_type, raw, text, &self.api.username) {
            return Ok(());
        }
        let is_bot = match self.users.get(sender) {
            Some(is_bot) => *is_bot,
            None => {
                let user = self
                    .api
                    .json(Method::GET, &format!("users/{sender}"), None)
                    .await?;
                if field(&user, "id") != sender {
                    return Err("Mattermost sender identity mismatch".into());
                }
                let is_bot = user["is_bot"].as_bool().unwrap_or(false);
                if self.users.len() >= 1024 {
                    self.users.clear();
                }
                self.users.insert(sender.to_owned(), is_bot);
                is_bot
            }
        };
        if is_bot {
            return Ok(());
        }
        let decision_text = if text.is_empty() && has_files {
            "[attachment]"
        } else {
            text
        };
        match inbound_decision_for_thread(
            &self.home,
            &self.group_id,
            PLATFORM,
            chat_id,
            thread_id,
            decision_text,
        )
        .await
        {
            InboundDecision::Reply(reply) => {
                // MM 客户端拦截裸 /命令，因此在公共帮助中显示安全的 @Bot 前缀。
                let mut reply = reply;
                for command in [
                    "/subscribe",
                    "/unsubscribe",
                    "/send",
                    "/pause",
                    "/resume",
                    "/verbose",
                    "/status",
                    "/help",
                ] {
                    reply = reply.replace(command, &format!("@{} {command}", self.api.username));
                }
                self.api.post(chat_id, thread_id, &reply, &[]).await?;
            }
            InboundDecision::Forward => {
                let key = target_key(chat_id, thread_id);
                self.reactions.start(&key, post_id).await;
                let result = async {
                    let mut attachments = Vec::new();
                    if has_files && !self.files_enabled {
                        return Err(
                            "Mattermost attachment forwarding is disabled for this group"
                                .to_owned(),
                        );
                    }
                    if let Some(ids) = post["file_ids"].as_array() {
                        for id in ids {
                            let id = id
                                .as_str()
                                .filter(|v| valid_id(v))
                                .ok_or("Invalid Mattermost file id")?;
                            attachments.push(
                                materialize_file(
                                    &self.home,
                                    &self.group_id,
                                    &self.api,
                                    id,
                                    post_id,
                                    self.max_file_bytes,
                                )
                                .await?,
                            );
                        }
                    }
                    // daemon 可能先发布回答再返回提交结果，完成反应必须等待 ID 绑定。
                    let _binding = self.reactions.binding.lock().await;
                    let event_id = dispatch_inbound_with(
                        &self.daemon,
                        &self.group_id,
                        PLATFORM,
                        chat_id,
                        sender,
                        text,
                        InboundMetadata {
                            message_id: post_id.to_owned(),
                            thread_id: thread_id.to_owned(),
                            attachments,
                        },
                    )
                    .await?;
                    self.reactions.bind(&key, post_id, event_id);
                    Ok(())
                }
                .await;
                match result {
                    Ok(()) => {}
                    Err(error) => {
                        self.reactions.fail_post(&key, post_id).await;
                        // 不回显 daemon/远端响应中的私人信息，完整错误仅进入本机错误日志。
                        if let Err(reply_error) = self
                            .api
                            .post(
                                chat_id,
                                thread_id,
                                "消息或附件未能交给 CCCC，请检查连接器错误后重试。",
                                &[],
                            )
                            .await
                        {
                            self.api.log_error(
                                &self.home,
                                &self.group_id,
                                "error_reply",
                                &reply_error,
                            );
                        }
                        return Err(error);
                    }
                }
            }
        }
        self.seen.insert(post_id.to_owned());
        self.order.push_back(post_id.to_owned());
        while self.order.len() > 8192 {
            if let Some(id) = self.order.pop_front() {
                self.seen.remove(&id);
            }
        }
        Ok(())
    }
}

fn eligible_post(post: &Value, bot_id: &str) -> bool {
    ["id", "user_id", "channel_id"]
        .iter()
        .all(|key| valid_id(field(post, key)))
        && (field(post, "root_id").is_empty() || valid_id(field(post, "root_id")))
        && field(post, "user_id") != bot_id
        && field(post, "type").is_empty()
        && post
            .pointer("/props/from_webhook")
            .is_none_or(|v| v != "true" && v != true)
        && post["delete_at"].as_i64().unwrap_or(0) == 0
}

fn mention_remainder<'a>(raw: &'a str, username: &str) -> Option<&'a str> {
    let rest = raw.strip_prefix(&format!("@{username}"))?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        None
    } else {
        Some(rest)
    }
}

fn strip_leading_mention<'a>(raw: &'a str, username: &str) -> &'a str {
    let mut text = raw.trim();
    while let Some(rest) = mention_remainder(text, username) {
        text = rest.trim_start_matches([' ', '\t', '\n', ':', ',', '：', '，']);
    }
    text
}

fn accepts_message(channel_type: &str, raw: &str, text: &str, username: &str) -> bool {
    matches!(channel_type, "D" | "G")
        || super::commands::is_recognized_command(text)
        || raw.match_indices('@').any(|(at, _)| {
            (at == 0
                || raw[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_whitespace()))
                && mention_remainder(&raw[at..], username).is_some()
        })
}

async fn materialize_file(
    home: &HomeLayout,
    group_id: &str,
    api: &MattermostApi,
    id: &str,
    post_id: &str,
    limit: u64,
) -> Result<Value, String> {
    let file = api
        .json(Method::GET, &format!("files/{id}/info"), None)
        .await?;
    if field(&file, "id") != id || field(&file, "post_id") != post_id {
        return Err("Mattermost attachment does not belong to the source post".into());
    }
    let size = file["size"].as_u64();
    if size.is_some_and(|size| size > limit) {
        return Err("Mattermost attachment exceeds configured size limit".into());
    }
    let mime = match field(&file, "mime_type") {
        "" => "application/octet-stream",
        value => value,
    };
    let title = match field(&file, "name") {
        "" => "file",
        value => value,
    };
    let spec = AttachmentSpec::new(
        if mime.starts_with("image/") {
            "image"
        } else {
            "file"
        },
        title,
        mime,
    )
    .with_source_id(id);
    let response = api
        .response(api.request(Method::GET, &format!("files/{id}")))
        .await?;
    if response.content_length().is_some_and(|size| size > limit) {
        return Err("Mattermost attachment exceeds configured size limit".into());
    }
    // 较低的组级限制也必须在写入 Blob 前执行，不能仅在下载完之后报错。
    let stream = response.bytes_stream().scan(0u64, move |total, result| {
        let result = result
            .map_err(|e| e.without_url().to_string())
            .and_then(|chunk| {
                *total = total.saturating_add(chunk.len() as u64);
                if *total > limit {
                    Err("Mattermost attachment exceeds configured size limit".to_owned())
                } else {
                    Ok(chunk)
                }
            });
        std::future::ready(Some(result))
    });
    store_stream(home, group_id, stream, spec).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mentions_are_exact_and_only_a_leading_mention_is_stripped() {
        assert_eq!(
            strip_leading_mention(" @cccc_bot: /send @claude 看看 @gemini 的回答", "cccc_bot"),
            "/send @claude 看看 @gemini 的回答"
        );
        for raw in [
            "@cccc_bot_extra hello",
            "x@cccc_bot hello",
            "someone@cccc_bot.test",
            "普通聊天",
        ] {
            assert!(!accepts_message("O", raw, raw, "cccc_bot"), "{raw}");
        }
        assert!(accepts_message(
            "O",
            "请 @cccc_bot 帮忙",
            "请 @cccc_bot 帮忙",
            "cccc_bot"
        ));
        assert!(accepts_message("D", "hello", "hello", "cccc_bot"));
        assert!(accepts_message("O", "/status", "/status", "cccc_bot"));
        assert_eq!(
            strip_leading_mention("@cccc_bot @cccc_bot hello", "cccc_bot"),
            "hello"
        );
    }

    #[test]
    fn excludes_echoes_system_posts_webhooks_and_invalid_paths() {
        let mut post = json!({"id":"a".repeat(26),"user_id":"b".repeat(26),"channel_id":"c".repeat(26),"type":"","root_id":""});
        assert!(eligible_post(&post, &"d".repeat(26)));
        assert!(!eligible_post(&post, &"b".repeat(26)));
        post["props"] = json!({"from_webhook":"true"});
        assert!(!eligible_post(&post, &"d".repeat(26)));
        post["props"] = json!({});
        post["type"] = json!("system_join_channel");
        assert!(!eligible_post(&post, &"d".repeat(26)));
        post["type"] = json!("");
        post["root_id"] = json!("../other");
        assert!(!eligible_post(&post, &"d".repeat(26)));
    }

    #[tokio::test]
    async fn forbidden_file_metadata_or_body_leaves_no_blob() {
        use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        };
        let reject_info = Arc::new(AtomicBool::new(true));
        let body_calls = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let site = format!("http://{}", listener.local_addr().expect("address"));
        let info = reject_info.clone();
        let calls = body_calls.clone();
        let app = Router::new()
            .route("/api/v4/users/me", get(|| async { axum::Json(json!({"id":"b".repeat(26),"username":"cccc_bot","is_bot":true})) }))
            .route("/api/v4/files/{id}/info", get(move || {
                let reject = info.load(Ordering::SeqCst);
                async move {
                    if reject { StatusCode::FORBIDDEN.into_response() }
                    else { axum::Json(json!({"id":"f".repeat(26),"post_id":"p".repeat(26),"name":"test.txt","size":6})).into_response() }
                }
            }))
            .route("/api/v4/files/{id}", get(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                async { StatusCode::FORBIDDEN }
            }));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        let config = json!({"mattermost_url":site,"bot_token":"test-token"});
        let api =
            MattermostApi::authenticate(config.as_object().expect("config"), "test-token".into())
                .await
                .expect("api");
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let group = store.create("forbidden files", "").expect("group");
        for forbidden_info in [true, false] {
            reject_info.store(forbidden_info, Ordering::SeqCst);
            let error = materialize_file(
                &home,
                &group.group_id,
                &api,
                &"f".repeat(26),
                &"p".repeat(26),
                100,
            )
            .await
            .expect_err("403");
            assert!(error.contains("403"));
            assert!(!error.contains("test-token"));
            assert_eq!(
                body_calls.load(Ordering::SeqCst),
                usize::from(!forbidden_info)
            );
            let blobs = store
                .state_dir(&group.group_id)
                .expect("state")
                .join("blobs");
            assert!(!blobs.exists() || std::fs::read_dir(blobs).expect("blobs").count() == 0);
        }
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn downloads_file_metadata_and_rejects_unknown_length_oversize_without_retaining_blob() {
        use axum::{Router, body::Body, response::Response, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let site = format!("http://{}", listener.local_addr().expect("address"));
        let app = Router::new()
            .route(
                "/api/v4/users/me",
                get(|| async {
                    axum::Json(json!({"id":"b".repeat(26),"username":"cccc_bot","is_bot":true}))
                }),
            )
            .route(
                "/api/v4/files/{id}/info",
                get(|| async {
                    axum::Json(json!({"id":"f".repeat(26),"post_id":"p".repeat(26),"name":"中文.txt","mime_type":"text/plain","size":null}))
                }),
            )
            .route(
                "/api/v4/files/{id}",
                get(|headers: axum::http::HeaderMap| async move {
                    assert_eq!(
                        headers.get("authorization").expect("auth"),
                        "Bearer test-token"
                    );
                    let chunks = futures_util::stream::iter([
                        Ok::<_, std::convert::Infallible>(&b"abc"[..]),
                        Ok(&b"def"[..]),
                    ]);
                    Response::new(Body::from_stream(chunks))
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        let config = json!({"mattermost_url":site,"bot_token":"test-token"});
        let api =
            MattermostApi::authenticate(config.as_object().expect("config"), "test-token".into())
                .await
                .expect("api");
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let group = store.create("files", "").expect("group");
        let error = materialize_file(
            &home,
            &group.group_id,
            &api,
            &"f".repeat(26),
            &"p".repeat(26),
            4,
        )
        .await
        .expect_err("size limit");
        assert!(error.contains("configured size limit"));
        let blobs = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("blobs");
        assert_eq!(std::fs::read_dir(&blobs).expect("blobs").count(), 0);

        let file = materialize_file(
            &home,
            &group.group_id,
            &api,
            &"f".repeat(26),
            &"p".repeat(26),
            6,
        )
        .await
        .expect("download");
        assert_eq!(file["title"], "中文.txt");
        assert_eq!(file["mime_type"], "text/plain");
        assert_eq!(file["bytes"], 6);
        assert_eq!(file["source_media_id"], "f".repeat(26));
        let path =
            cccc_core::blobs::resolve(&home, &group.group_id, file["path"].as_str().expect("path"))
                .expect("blob");
        assert_eq!(std::fs::read(path).expect("read"), b"abcdef");
        let error = materialize_file(
            &home,
            &group.group_id,
            &api,
            &"f".repeat(26),
            &"other".repeat(6),
            6,
        )
        .await
        .expect_err("wrong source post");
        assert!(error.contains("does not belong to the source post"));
        server.abort();
    }
}
