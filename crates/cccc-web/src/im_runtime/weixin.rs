use super::weixin_inbound::{has_media, materialize_media};
use super::weixin_outbound::WeixinOutbound;
use super::{
    InboundDecision, InboundMetadata, dispatch_inbound_with, inbound_decision, spawn_outbound,
};
use async_trait::async_trait;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::Value;
use std::sync::Arc;
use tokio::task::JoinHandle;
use weixin_agent::{MessageContext, MessageHandler, WeixinClient, WeixinConfig};

const PLATFORM: &str = "weixin";

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<(Vec<JoinHandle<()>>, Arc<WeixinClient>), String> {
    let (token, base_url) = load_credentials(&home, group_id)?;
    let _ = super::weixin_login::ensure_stored_login_authorized(&home, group_id)?;
    let mut builder = WeixinConfig::builder().token(token);
    if !base_url.is_empty() {
        builder = builder.base_url(base_url);
    }
    let config = builder
        .build()
        .map_err(|error| format!("Weixin configuration failed: {error}"))?;
    let handler = Handler {
        home: home.clone(),
        daemon,
        group_id: group_id.to_owned(),
    };
    let sdk = Arc::new(
        WeixinClient::builder(config)
            .on_message(handler)
            .build()
            .map_err(|error| format!("Weixin client setup failed: {error}"))?,
    );
    let connection_sdk = Arc::clone(&sdk);
    let sync_buf = load_optional_string(
        &home
            .groups_dir()
            .join(group_id)
            .join("state/im_weixin_sync_buf.txt"),
    );
    let connection = tokio::spawn(async move {
        if let Err(error) = connection_sdk.start(sync_buf).await {
            tracing::error!(%error, "Weixin IM monitor stopped");
        }
    });
    tokio::task::yield_now().await;
    if connection.is_finished() {
        return Err("Weixin monitor failed during startup".into());
    }
    let outbound = spawn_outbound(
        home.clone(),
        group_id.to_owned(),
        PLATFORM,
        ledger_events,
        WeixinOutbound::new(home, group_id, Arc::clone(&sdk)),
        |outbound, targets, event| async move {
            let targets = targets
                .into_iter()
                .map(|target| target.chat_id)
                .collect::<Vec<_>>();
            outbound.send(&targets, &event).await;
        },
    );
    Ok((vec![connection, outbound], sdk))
}

struct Handler {
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: String,
}

#[async_trait]
impl MessageHandler for Handler {
    async fn on_message(&self, context: &MessageContext) -> weixin_agent::Result<()> {
        let text = context.body.as_deref().unwrap_or("").trim();
        if text.is_empty() && !has_media(context) {
            return Ok(());
        }
        match inbound_decision(&self.home, &self.group_id, PLATFORM, &context.from, text).await {
            InboundDecision::Forward => {}
            InboundDecision::Reply(body) => {
                context.reply_text(&body).await?;
                return Ok(());
            }
        }
        let attachments = materialize_media(&self.home, &self.group_id, context).await;
        if text.is_empty() && attachments.is_empty() {
            return Ok(());
        }
        if let Err(error) = dispatch_inbound_with(
            &self.daemon,
            &self.group_id,
            PLATFORM,
            &context.from,
            &context.from,
            text,
            InboundMetadata {
                message_id: context.message_id.clone(),
                thread_id: String::new(),
                attachments,
            },
        )
        .await
        {
            tracing::warn!(%error, "failed to dispatch Weixin IM message");
        }
        Ok(())
    }

    async fn on_sync_buf_updated(&self, sync_buf: &str) -> weixin_agent::Result<()> {
        let path = self
            .home
            .groups_dir()
            .join(&self.group_id)
            .join("state/im_weixin_sync_buf.txt");
        tokio::fs::write(path, sync_buf).await?;
        Ok(())
    }
}

fn load_credentials(home: &HomeLayout, group_id: &str) -> Result<(String, String), String> {
    let path = home
        .groups_dir()
        .join(group_id)
        .join("state/im_weixin_credentials.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| format!("Weixin is not logged in: {}", path.display()))?;
    let value: Value = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    let token = value
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if token.is_empty() {
        return Err("Weixin credential token is empty".into());
    }
    let base_url = super::weixin_login::CANONICAL_WEIXIN_QR_BASE_URL.to_owned();
    Ok((token, base_url))
}

fn load_optional_string(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_provider_base_url_never_controls_authenticated_requests() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let group_id = "weixin-base-url";
        let state_dir = home.groups_dir().join(group_id).join("state");
        std::fs::create_dir_all(&state_dir).expect("state dir");
        std::fs::write(
            state_dir.join("im_weixin_credentials.json"),
            br#"{"token":"secret","baseUrl":"http://127.0.0.1:8080/private"}"#,
        )
        .expect("credentials");

        let (token, base_url) = load_credentials(&home, group_id).expect("load credentials");

        assert_eq!(token, "secret");
        assert_eq!(base_url, "https://ilinkai.weixin.qq.com/");
    }
}
