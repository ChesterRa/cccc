use anyhow::{Context, Result};
use cccc_contracts::{DaemonRequest, DaemonResponse, Event};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior};

const STREAMABLE_KINDS: &[&str] = &[
    "chat.message",
    "mail.read",
    "chat.reply_request.cancelled",
    "runtime.delivery",
    "system.notify",
];
const REPLAY_LIMIT: usize = 2_000;
const RECENT_EVENT_LIMIT: usize = 2_048;
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn handle<S>(
    mut stream: BufReader<S>,
    home: HomeLayout,
    request: DaemonRequest,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let prepared = match prepare(&home, &request) {
        Ok(prepared) => prepared,
        Err(response) => {
            write_line(stream.get_mut(), &response).await?;
            return Ok(());
        }
    };
    let Prepared {
        store,
        group,
        group_id,
        by,
        kinds,
        ledger_path,
        mut follower,
        replay,
    } = prepared;
    write_line(
        stream.get_mut(),
        &DaemonResponse::success(
            json!({"group_id":group_id})
                .as_object()
                .cloned()
                .expect("events_stream handshake is an object"),
        ),
    )
    .await?;

    let mut recent = RecentEvents::default();
    for event in replay {
        if visible(&group, &by, &kinds, &event) && recent.insert(&event.id) {
            write_event(stream.get_mut(), &event).await?;
        }
    }
    let (mut read, mut write) = tokio::io::split(stream);
    let mut client_input = [0_u8; 1];

    let mut poll = tokio::time::interval_at(Instant::now() + POLL_INTERVAL, POLL_INTERVAL);
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut heartbeat =
        tokio::time::interval_at(Instant::now() + HEARTBEAT_INTERVAL, HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = poll.tick() => {
                let events = follower.poll(&ledger_path)?;
                if events.is_empty() {
                    continue;
                }
                let current_group = if by == "user" {
                    None
                } else {
                    Some(store.load(&group_id)?)
                };
                let routing_group = current_group.as_ref().unwrap_or(&group);
                for event in events {
                    if visible(routing_group, &by, &kinds, &event) && recent.insert(&event.id) {
                        write_event(&mut write, &event).await?;
                    }
                }
            }
            _ = heartbeat.tick() => {
                write_line(
                    &mut write,
                    &json!({"t":"heartbeat","ts":cccc_contracts::utc_now()}),
                )
                .await?;
            }
            _ = read.read(&mut client_input) => {
                // After the handshake this protocol is daemon-to-client only.
                // Any client input or EOF retires the dedicated subscription.
                return Ok(());
            }
            changed = shutdown.changed() => {
                changed.ok();
                return Ok(());
            }
        }
    }
}

struct Prepared {
    store: GroupStore,
    group: GroupDoc,
    group_id: String,
    by: String,
    kinds: BTreeSet<String>,
    ledger_path: PathBuf,
    follower: ledger::LedgerFollower,
    replay: Vec<Event>,
}

fn prepare(home: &HomeLayout, request: &DaemonRequest) -> Result<Prepared, DaemonResponse> {
    let group_id = request
        .args
        .get("group_id")
        .and_then(non_blank)
        .ok_or_else(|| DaemonResponse::failure("missing_group_id", "missing group_id"))?;
    let by = request
        .args
        .get("by")
        .and_then(non_blank)
        .unwrap_or_else(|| "user".into());
    let kinds = requested_kinds(request.args.get("kinds"))?;
    let store = GroupStore::new(home.clone())
        .map_err(|error| DaemonResponse::failure("daemon_error", error.to_string()))?;
    let group = store.load(&group_id).map_err(|_| {
        DaemonResponse::failure("group_not_found", format!("group not found: {group_id}"))
    })?;
    if by != "user" && cccc_core::actors::find(&group, &by).is_none() {
        return Err(DaemonResponse::failure(
            "unknown_actor",
            format!("unknown actor: {by}"),
        ));
    }
    let ledger_path = store
        .ledger_path(&group_id)
        .map_err(|error| DaemonResponse::failure("daemon_error", error.to_string()))?;
    let mut follower = ledger::LedgerFollower::default();
    follower
        .poll(&ledger_path)
        .map_err(|error| DaemonResponse::failure("daemon_error", error.to_string()))?;
    let replay = replay_events(
        &ledger_path,
        request
            .args
            .get("since_event_id")
            .and_then(non_blank)
            .as_deref(),
        request.args.get("since_ts").and_then(non_blank).as_deref(),
    )
    .map_err(|error| DaemonResponse::failure("daemon_error", error.to_string()))?;
    Ok(Prepared {
        store,
        group,
        group_id,
        by,
        kinds,
        ledger_path,
        follower,
        replay,
    })
}

fn requested_kinds(value: Option<&Value>) -> Result<BTreeSet<String>, DaemonResponse> {
    let supported = STREAMABLE_KINDS
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect::<BTreeSet<_>>();
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(supported);
    };
    let Some(items) = value.as_array() else {
        return Err(DaemonResponse::failure(
            "invalid_args",
            "kinds must be an array of strings or null",
        ));
    };
    if items.iter().any(|item| !item.is_string()) {
        return Err(DaemonResponse::failure(
            "invalid_args",
            "kinds must contain only strings",
        ));
    }
    let requested = items.iter().filter_map(non_blank).collect::<BTreeSet<_>>();
    if requested.is_empty() {
        let mut response = DaemonResponse::failure("invalid_kinds", "no supported kinds requested");
        if let Some(error) = response.error.as_mut() {
            error.details.insert("supported".into(), json!(supported));
        }
        return Err(response);
    }
    let filtered = requested
        .intersection(&supported)
        .cloned()
        .collect::<BTreeSet<_>>();
    if filtered.is_empty() {
        let mut response = DaemonResponse::failure("invalid_kinds", "no supported kinds requested");
        if let Some(error) = response.error.as_mut() {
            error.details.insert("supported".into(), json!(supported));
        }
        return Err(response);
    }
    Ok(filtered)
}

fn replay_events(
    path: &std::path::Path,
    since_event_id: Option<&str>,
    since_ts: Option<&str>,
) -> std::io::Result<Vec<Event>> {
    if since_event_id.is_none() && since_ts.is_none() {
        return Ok(Vec::new());
    }
    let events = ledger::tail(path, REPLAY_LIMIT)?;
    if let Some(event_id) = since_event_id
        && let Some(index) = events.iter().position(|event| event.id == event_id)
    {
        return Ok(events.into_iter().skip(index + 1).collect());
    }
    let Some(since_ts) = since_ts else {
        return Ok(Vec::new());
    };
    let Ok(cutoff) = chrono::DateTime::parse_from_rfc3339(since_ts) else {
        return Ok(Vec::new());
    };
    Ok(events
        .into_iter()
        .filter(|event| chrono::DateTime::parse_from_rfc3339(&event.ts).is_ok_and(|ts| ts > cutoff))
        .collect())
}

fn visible(group: &GroupDoc, by: &str, kinds: &BTreeSet<String>, event: &Event) -> bool {
    if !kinds.contains(&event.kind) {
        return false;
    }
    if by == "user" {
        return true;
    }
    if !matches!(event.kind.as_str(), "chat.message" | "system.notify") {
        return false;
    }
    if event.kind == "chat.message" && event.by == by {
        return false;
    }
    inbox::is_for_actor(group, event, by)
}

fn non_blank(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

async fn write_event<W>(write: &mut W, event: &Event) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_line(write, &json!({"t":"event","event":event})).await
}

async fn write_line<W>(write: &mut W, value: &impl Serialize) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut payload = serde_json::to_vec(value)?;
    payload.push(b'\n');
    tokio::time::timeout(WRITE_TIMEOUT, async {
        write.write_all(&payload).await?;
        write.flush().await
    })
    .await
    .context("events_stream client stopped reading")??;
    Ok(())
}

#[derive(Default)]
struct RecentEvents {
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl RecentEvents {
    fn insert(&mut self, event_id: &str) -> bool {
        if event_id.is_empty() || !self.ids.insert(event_id.to_owned()) {
            return false;
        }
        self.order.push_back(event_id.to_owned());
        if self.order.len() > RECENT_EVENT_LIMIT
            && let Some(expired) = self.order.pop_front()
        {
            self.ids.remove(&expired);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;
    use serde_json::Map;
    use tokio::io::{AsyncBufReadExt, BufReader};

    fn message(group_id: &str, id: &str, to: &str, text: &str) -> Event {
        let mut event = Event::new("chat.message", group_id);
        event.id = id.into();
        event.by = "user".into();
        event.data = json!({
            "text":text,
            "message_mode":"send",
            "to":[to]
        })
        .as_object()
        .cloned()
        .expect("message data");
        event
    }

    #[tokio::test]
    async fn resumes_filters_and_follows_the_ledger() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("stream", "").expect("group");
        group.actors.push(Actor::new("peer1"));
        group.actors.push(Actor::new("peer2"));
        store.save(&group).expect("save actors");
        let path = store.ledger_path(&group.group_id).expect("ledger path");
        let first = message(&group.group_id, "event-first", "peer1", "first");
        let hidden = message(&group.group_id, "event-hidden", "peer2", "hidden");
        let resumed = message(&group.group_id, "event-resumed", "peer1", "resumed");
        for event in [&first, &hidden, &resumed] {
            ledger::append(&path, event).expect("append fixture event");
        }

        let request = DaemonRequest {
            v: 1,
            op: "events_stream".into(),
            args: json!({
                "group_id":group.group_id.clone(),
                "by":"peer1",
                "since_event_id":first.id,
                "kinds":["chat.message"]
            })
            .as_object()
            .cloned()
            .unwrap_or_else(Map::new),
        };
        let (client, server) = tokio::io::duplex(32 * 1024);
        let (shutdown, receiver) = watch::channel(false);
        let task = tokio::spawn(handle(BufReader::new(server), home, request, receiver));
        let mut client = BufReader::new(client);

        let mut line = String::new();
        client.read_line(&mut line).await.expect("handshake");
        let handshake: DaemonResponse = serde_json::from_str(&line).expect("handshake JSON");
        assert!(handshake.ok, "{:?}", handshake.error);
        line.clear();
        client.read_line(&mut line).await.expect("resumed event");
        let item: Value = serde_json::from_str(&line).expect("stream item");
        assert_eq!(item["t"], "event");
        assert_eq!(item["event"]["id"], "event-resumed");

        let live = message(&group.group_id, "event-live", "peer1", "live");
        ledger::append(&path, &live).expect("append live event");
        line.clear();
        tokio::time::timeout(Duration::from_secs(2), client.read_line(&mut line))
            .await
            .expect("live event timeout")
            .expect("live event");
        let item: Value = serde_json::from_str(&line).expect("live stream item");
        assert_eq!(item["event"]["id"], "event-live");

        shutdown.send(true).expect("shutdown stream");
        task.await.expect("stream task").expect("stream handler");
    }

    #[test]
    fn rejects_an_explicit_unsupported_kind() {
        let error = requested_kinds(Some(&json!(["chat.stream"]))).expect_err("invalid kinds");
        assert_eq!(error.error.expect("error").code, "invalid_kinds");
        let error = requested_kinds(Some(&json!([]))).expect_err("empty kinds");
        assert_eq!(error.error.expect("error").code, "invalid_kinds");
        let error =
            requested_kinds(Some(&json!(["chat.message", 1]))).expect_err("non-string kind");
        assert_eq!(error.error.expect("error").code, "invalid_args");
    }

    #[test]
    fn actor_visibility_matches_the_python_stream_boundary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("stream", "").expect("group");
        group.actors.push(Actor::new("peer1"));
        let kinds = STREAMABLE_KINDS
            .iter()
            .map(|kind| (*kind).to_owned())
            .collect::<BTreeSet<_>>();

        let directed = message(&group.group_id, "event-directed", "peer1", "visible");
        assert!(visible(&group, "peer1", &kinds, &directed));

        let mut own = message(&group.group_id, "event-own", "peer1", "own");
        own.by = "peer1".into();
        assert!(!visible(&group, "peer1", &kinds, &own));

        let mut delivery = Event::new("runtime.delivery", &group.group_id);
        delivery.by = "system".into();
        assert!(!visible(&group, "peer1", &kinds, &delivery));
        assert!(visible(&group, "user", &kinds, &delivery));
    }
}
