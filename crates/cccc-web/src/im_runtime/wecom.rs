use super::wecom_client::WecomClient;
use super::wecom_message::{MessageDeduper, materialize_attachments, parse_inbound};
use super::wecom_outbound::WecomOutbound;
use super::{
    InboundMetadata, accepts_inbound, dispatch_inbound_with, is_outbound_or_stream,
    resolve_credential, spawn_outbound_matching, string,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use tokio::task::JoinHandle;

const PLATFORM: &str = "wecom";

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
) -> Result<(Vec<JoinHandle<()>>, Arc<WecomClient>), String> {
    let bot_id = resolve_credential(&string(config, "wecom_bot_id"))?;
    let secret = resolve_credential(&string(config, "wecom_secret"))?;
    let callback_home = home.clone();
    let callback_daemon = daemon.clone();
    let callback_group = group_id.to_owned();
    let deduper = Arc::new(MessageDeduper::default());
    let callback = move |frame: Value| {
        let Some(message) = parse_inbound(&frame) else {
            return;
        };
        if !deduper.accept(&message.chat_id, &message.message_id) {
            return;
        }
        let home = callback_home.clone();
        let daemon = callback_daemon.clone();
        let group_id = callback_group.clone();
        tokio::spawn(async move {
            if !accepts_inbound(&home, &group_id, PLATFORM, &message.chat_id, &message.text) {
                return;
            }
            let attachments = materialize_attachments(&home, &group_id, &message.attachments).await;
            if let Err(error) = dispatch_inbound_with(
                &daemon,
                &group_id,
                PLATFORM,
                &message.chat_id,
                &message.sender,
                &message.text,
                InboundMetadata {
                    message_id: message.message_id,
                    attachments,
                },
            )
            .await
            {
                tracing::warn!(%error, "failed to dispatch WeCom IM message");
            }
        });
    };
    let status_home = home.clone();
    let status_group = group_id.to_owned();
    let (sdk, connection) =
        WecomClient::connect_with_status(bot_id, secret, callback, move |error| {
            persist_terminal_error(&status_home, &status_group, &error)
        })
        .await?;
    let outbound_sender = WecomOutbound::new(home.clone(), group_id.to_owned(), Arc::clone(&sdk));
    let outbound = spawn_outbound_matching(
        home,
        group_id.to_owned(),
        outbound_sender,
        is_outbound_or_stream,
        |sender, targets, event| async move {
            sender.send(targets, event).await;
        },
    );
    Ok((vec![connection, outbound], sdk))
}

fn persist_terminal_error(home: &HomeLayout, group_id: &str, error: &str) {
    let Ok(store) = cccc_core::GroupStore::new(home.clone()) else {
        return;
    };
    if let Err(persist_error) =
        cccc_core::integration_state::group_update(&store, group_id, "im_bridge", |value| {
            if !value.is_object() {
                *value = json!({});
            }
            let state = value.as_object_mut().expect("IM state initialized");
            state.insert("last_error".into(), json!(error));
            state.insert("updated_at".into(), json!(cccc_contracts::utc_now()));
            Ok(())
        })
    {
        tracing::warn!(%persist_error, %group_id, "failed to persist WeCom terminal error");
    }
}
