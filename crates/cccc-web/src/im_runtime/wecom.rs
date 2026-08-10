use super::wecom_client::WecomClient;
use super::wecom_message::{MessageDeduper, materialize_attachments, parse_inbound};
use super::wecom_outbound::WecomOutbound;
use super::{
    InboundDecision, InboundMetadata, dispatch_inbound_with, inbound_decision,
    is_outbound_or_stream, resolve_config_credential, spawn_outbound_matching,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

const PLATFORM: &str = "wecom";

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
    on_terminal_error: impl Fn(&HomeLayout, &str, &str) + Send + Sync + 'static,
) -> Result<(Vec<JoinHandle<()>>, Arc<WecomClient>), String> {
    let bot_id = resolve_config_credential(config, "wecom_bot_id", "wecom_bot_id_env")?;
    let secret = resolve_config_credential(config, "wecom_secret", "wecom_secret_env")?;
    let deduper = Arc::new(MessageDeduper::default());
    let (inbound_tx, mut inbound_rx) = mpsc::channel(128);
    let callback = move |frame: Value| {
        let inbound_tx = inbound_tx.clone();
        let deduper = Arc::clone(&deduper);
        async move {
            let Some(message) = parse_inbound(&frame) else {
                return;
            };
            if !deduper.accept(&message.chat_id, &message.message_id) {
                return;
            }
            if let Err(error) = inbound_tx.send(message).await {
                tracing::debug!(%error, "WeCom inbound worker stopped");
            }
        }
    };
    let status_home = home.clone();
    let status_group = group_id.to_owned();
    let connection_result =
        WecomClient::connect_with_status(bot_id, secret, callback, move |error| {
            on_terminal_error(&status_home, &status_group, &error)
        })
        .await;
    let (sdk, connection) = match connection_result {
        Ok(connection) => connection,
        Err(error) => return Err(error),
    };
    let inbound_home = home.clone();
    let inbound_group = group_id.to_owned();
    let inbound_sdk = Arc::clone(&sdk);
    let inbound = tokio::spawn(async move {
        let mut command_replies = JoinSet::new();
        loop {
            let message = tokio::select! {
                message = inbound_rx.recv() => {
                    let Some(message) = message else {
                        command_replies.abort_all();
                        break;
                    };
                    message
                }
                Some(_) = command_replies.join_next(), if !command_replies.is_empty() => continue,
            };
            match inbound_decision(
                &inbound_home,
                &inbound_group,
                PLATFORM,
                &message.chat_id,
                &message.text,
            )
            .await
            {
                InboundDecision::Forward => {}
                InboundDecision::Reply(body) => {
                    let sdk = Arc::clone(&inbound_sdk);
                    let chat_id = message.chat_id.clone();
                    command_replies.spawn(async move {
                        if let Err(error) = sdk
                            .send_message(
                                &chat_id,
                                json!({"msgtype":"markdown","markdown":{"content":body}}),
                            )
                            .await
                        {
                            tracing::warn!(%error, "failed to send WeCom command reply");
                        }
                    });
                    continue;
                }
            }
            let attachments =
                materialize_attachments(&inbound_home, &inbound_group, &message.attachments).await;
            if let Err(error) = dispatch_inbound_with(
                &daemon,
                &inbound_group,
                PLATFORM,
                &message.chat_id,
                &message.sender,
                &message.text,
                InboundMetadata {
                    message_id: message.message_id,
                    thread_id: String::new(),
                    attachments,
                },
            )
            .await
            {
                tracing::warn!(%error, "failed to dispatch WeCom IM message");
            }
        }
    });
    let outbound_sender = WecomOutbound::new(home.clone(), group_id.to_owned(), Arc::clone(&sdk));
    let outbound = spawn_outbound_matching(
        home,
        group_id.to_owned(),
        PLATFORM,
        ledger_events,
        outbound_sender,
        is_outbound_or_stream,
        |sender, targets, event| async move {
            sender
                .send(
                    targets.into_iter().map(|target| target.chat_id).collect(),
                    event,
                )
                .await;
        },
    );
    Ok((vec![connection, inbound, outbound], sdk))
}

pub(super) fn persist_terminal_error(home: &HomeLayout, group_id: &str, error: &str) {
    let Ok(store) = cccc_core::GroupStore::new(home.clone()) else {
        return;
    };
    if let Err(persist_error) = cccc_core::im_state::update(&store, group_id, |value| {
        if !value.is_object() {
            *value = json!({});
        }
        let state = value.as_object_mut().expect("IM state initialized");
        state.insert("last_error".into(), json!(error));
        state.insert("updated_at".into(), json!(cccc_contracts::utc_now()));
        Ok(())
    }) {
        tracing::warn!(%persist_error, %group_id, "failed to persist WeCom terminal error");
    }
}
