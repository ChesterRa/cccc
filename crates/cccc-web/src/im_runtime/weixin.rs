use super::{accepts_inbound, dispatch_inbound, outbound_text, spawn_outbound};
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
) -> Result<(Vec<JoinHandle<()>>, Arc<WeixinClient>), String> {
    let (token, base_url) = load_credentials(&home, group_id)?;
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
        home,
        group_id.to_owned(),
        Arc::clone(&sdk),
        |sdk, targets, event| async move {
            let Some(body) = outbound_text(&event, false) else {
                return;
            };
            for user_id in targets {
                let context_token = sdk.context_tokens().get(&user_id);
                if let Err(error) = sdk
                    .send_text(&user_id, &body, context_token.as_deref())
                    .await
                {
                    tracing::warn!(%error, "failed to send Weixin IM message");
                }
            }
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
        if text.is_empty()
            || !accepts_inbound(&self.home, &self.group_id, PLATFORM, &context.from, text)
        {
            return Ok(());
        }
        if let Err(error) = dispatch_inbound(
            &self.daemon,
            &self.group_id,
            PLATFORM,
            &context.from,
            &context.from,
            text,
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
    let base_url = value
        .get("baseUrl")
        .or_else(|| value.get("base_url"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    Ok((token, base_url))
}

fn load_optional_string(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
