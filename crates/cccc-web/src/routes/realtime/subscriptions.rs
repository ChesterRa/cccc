use super::event::EventStream;
use crate::{
    AppState,
    auth::Principal,
    connect_frames::{ResourceFrameQuery, live_access, live_group_access},
    routes::{headless, streams},
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::{sync::mpsc::Sender, task::JoinHandle};

#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub(super) enum Channel {
    Global,
    Ledger,
    Headless,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub(super) enum Command {
    Subscribe {
        channel: Channel,
        id: u64,
        group_id: Option<String>,
        cursor: Option<String>,
        replay: Option<bool>,
    },
    Unsubscribe {
        channel: Channel,
        id: u64,
    },
}

pub(super) struct Packet {
    pub channel: Channel,
    pub id: u64,
    pub value: Value,
}
struct Subscription {
    id: u64,
    task: JoinHandle<()>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[derive(Default)]
pub(super) struct Subscriptions(HashMap<Channel, Subscription>);

impl Subscriptions {
    pub fn is_current(&self, packet: &Packet) -> bool {
        self.0
            .get(&packet.channel)
            .is_some_and(|entry| entry.id == packet.id)
    }
    pub fn apply(
        &mut self,
        command: Command,
        state: &AppState,
        principal: &Principal,
        frame: &ResourceFrameQuery,
        sender: &Sender<Packet>,
    ) -> Result<Option<Value>, Value> {
        let (channel, id, group_id, cursor, replay) = match command {
            Command::Unsubscribe { channel, id } => {
                if self.0.get(&channel).is_some_and(|entry| entry.id == id) {
                    self.0.remove(&channel);
                }
                return Ok(None);
            }
            Command::Subscribe {
                channel,
                id,
                group_id,
                cursor,
                replay,
            } => (channel, id, group_id, cursor, replay),
        };
        // Remove the previous producer first, including buffered packets from its generation.
        self.0.remove(&channel);
        let deny = |code| json!({"type":"closed","channel":channel,"id":id,"code":code});
        if id == 0 || cursor.as_ref().is_some_and(|cursor| cursor.len() > 512) {
            return Err(deny("invalid_subscription"));
        }
        let frame_id = frame.connect_frame.clone();
        let stream: EventStream = if channel == Channel::Global {
            if live_access(state, principal, frame_id.as_deref()).is_none() {
                return Err(deny("auth_required"));
            }
            streams::global_source(
                state.clone(),
                principal.clone(),
                ResourceFrameQuery {
                    connect_frame: frame_id,
                },
            )
        } else {
            let group = group_id
                .filter(|group| !group.is_empty() && group.len() <= 128)
                .ok_or_else(|| deny("invalid_group"))?;
            if !live_group_access(state, principal, &group, frame_id.as_deref()) {
                return Err(deny("permission_denied"));
            }
            // Subscription paths do not pass through path-based group authorization.
            // Validate both scope and existence before opening either producer.
            state
                .ledger_events
                .subscribe_group(&group)
                .map_err(|_| deny("group_not_found"))?;
            match channel {
                Channel::Ledger => streams::group_source(
                    state.clone(),
                    group,
                    principal.clone(),
                    ResourceFrameQuery {
                        connect_frame: frame_id,
                    },
                    cursor.unwrap_or_default(),
                )
                .map_err(|_| deny("group_not_found"))?,
                Channel::Headless => headless::headless_source(
                    state.clone(),
                    group,
                    principal.clone(),
                    replay.unwrap_or(true),
                    frame_id,
                ),
                Channel::Global => unreachable!(),
            }
        };
        let sender = sender.clone();
        let task = tokio::spawn(async move {
            let mut stream = stream;
            while let Some(Ok(event)) = stream.next().await {
                if event.event == "connected" {
                    continue;
                }
                let packet = Packet {
                    channel,
                    id,
                    value: json!({"type":"event","channel":channel,"id":id,"message":event}),
                };
                if sender.send(packet).await.is_err() {
                    return;
                }
            }
            let _ = sender
                .send(Packet {
                    channel,
                    id,
                    value: json!({"type":"closed","channel":channel,"id":id}),
                })
                .await;
        });
        self.0.insert(channel, Subscription { id, task });
        Ok(Some(json!({"type":"ready","channel":channel,"id":id})))
    }
}
