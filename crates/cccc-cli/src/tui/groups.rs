//! Groups as the main screen and `/groups` show them, and the two actions
//! `/groups` offers: starting and stopping a whole group. Everything comes
//! from the daemon's `group_list`, `actor_list`, `group_start` and
//! `group_stop`; the interface keeps nothing about a group beyond the last
//! answer it read.

use super::daemon;
use cccc_client::{ClientError, DaemonClient};
use cccc_contracts::{ActorRuntime, DaemonRequest};
use cccc_core::HomeLayout;
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::time::Instant;
use tokio::sync::{mpsc, watch};

/// A group's state in the words the Web shows for it
/// (`web/src/utils/groupStatus.ts`), in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum GroupStatus {
    Running,
    Idle,
    Paused,
    Offline,
    Stopped,
}

impl GroupStatus {
    /// Lifecycle intent precedes observation: no running actors does not
    /// mean the user stopped the Group.
    fn of(running: bool, state: &str) -> Self {
        if state == "paused" {
            Self::Paused
        } else if state == "stopped" {
            Self::Stopped
        } else if !running {
            Self::Offline
        } else if state == "idle" {
            Self::Idle
        } else {
            Self::Running
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Idle => "Idle",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
            Self::Offline => "Offline",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GroupSummary {
    pub(super) group_id: String,
    pub(super) title: String,
    pub(super) status: GroupStatus,
    /// The recorded lifecycle state: `active`, `idle`, `paused` or `stopped`.
    /// It is not a running count: a new group and a started one with
    /// nothing running are both `active`.
    pub(super) state: String,
    /// Actors the daemon counts as running. A native member counts while
    /// this group may deliver to it, not while its model is busy.
    pub(super) running_actors: u64,
}

impl GroupSummary {
    /// The title, or the ID for a group without one.
    pub(super) fn name(&self) -> &str {
        if self.title.trim().is_empty() {
            &self.group_id
        } else {
            &self.title
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GroupList {
    /// The last list the daemon returned, in display order.
    pub(super) groups: Vec<GroupSummary>,
    pub(super) last_ok: Option<Instant>,
    /// Why the latest read failed; the next successful read clears it.
    pub(super) error: Option<String>,
}

impl GroupList {
    pub(super) fn record(&mut self, read: Result<Vec<GroupSummary>, String>, at: Instant) {
        match read {
            Ok(mut groups) => {
                groups.sort_by(display_order);
                self.groups = groups;
                self.last_ok = Some(at);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// Groups shown as running or idle.
    pub(super) fn running(&self) -> usize {
        self.groups
            .iter()
            .filter(|group| matches!(group.status, GroupStatus::Running | GroupStatus::Idle))
            .count()
    }

    pub(super) fn position(&self, group_id: &str) -> Option<usize> {
        self.groups
            .iter()
            .position(|group| group.group_id == group_id)
    }

    pub(super) fn find(&self, group_id: &str) -> Option<&GroupSummary> {
        self.position(group_id).map(|index| &self.groups[index])
    }
}

// Running groups first; the rest of the order only keeps rows from trading
// places between refreshes.
fn display_order(left: &GroupSummary, right: &GroupSummary) -> Ordering {
    left.status
        .cmp(&right.status)
        .then_with(|| left.name().to_lowercase().cmp(&right.name().to_lowercase()))
        .then_with(|| left.group_id.cmp(&right.group_id))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MemberState {
    Running,
    Stopped,
    /// A managed actor that is not enabled.
    Off,
    /// A native member this group can deliver to now.
    Ready,
    NotReceiving,
    /// A native member whose CCCC access is paused.
    AccessPaused,
}

impl MemberState {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Off => "off",
            Self::Ready => "ready",
            Self::NotReceiving => "not receiving",
            Self::AccessPaused => "access paused",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Member {
    pub(super) id: String,
    pub(super) runtime: String,
    /// `foreman` or `peer`, as the daemon resolves it.
    pub(super) role: String,
    pub(super) native: bool,
    pub(super) state: MemberState,
}

/// The members of one group: the one selected on `/groups`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct MemberList {
    /// The group these members belong to. A read for another group replaces
    /// the list instead of mixing into it.
    pub(super) group_id: String,
    pub(super) members: Vec<Member>,
    pub(super) last_ok: Option<Instant>,
    pub(super) error: Option<String>,
}

impl MemberList {
    pub(super) fn record(
        &mut self,
        group_id: &str,
        read: Result<Vec<Member>, String>,
        at: Instant,
    ) {
        if self.group_id != group_id {
            *self = Self {
                group_id: group_id.to_owned(),
                ..Self::default()
            };
        }
        match read {
            Ok(members) => {
                self.members = members;
                self.last_ok = Some(at);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// Whether the group may have native members: yes, or not known, which
    /// includes when the latest read failed and the members may have changed.
    pub(super) fn may_have_native(&self) -> bool {
        self.last_ok.is_none()
            || self.error.is_some()
            || self.members.iter().any(|member| member.native)
    }
}

pub(super) async fn read_groups(client: &DaemonClient) -> Result<Vec<GroupSummary>, String> {
    let result = daemon::read(client, "group_list", Map::new()).await?;
    let groups = result
        .get("groups")
        .and_then(Value::as_array)
        .ok_or_else(|| "the daemon answered without a group list".to_owned())?;
    // A row without an ID can be neither shown as a group nor acted on.
    Ok(groups.iter().filter_map(summary).collect())
}

fn summary(value: &Value) -> Option<GroupSummary> {
    let group_id = value.get("group_id")?.as_str()?.to_owned();
    let state = value
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("active")
        .to_owned();
    let running = value
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(GroupSummary {
        status: GroupStatus::of(running, &state),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        running_actors: value
            .pointer("/runtime_status/running_actor_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        group_id,
        state,
    })
}

pub(super) async fn read_members(
    client: &DaemonClient,
    group_id: &str,
) -> Result<Vec<Member>, String> {
    let mut args = Map::new();
    args.insert("group_id".into(), Value::String(group_id.to_owned()));
    let result = daemon::read(client, "actor_list", args).await?;
    let actors = result
        .get("actors")
        .and_then(Value::as_array)
        .ok_or_else(|| "the daemon answered without an actor list".to_owned())?;
    Ok(actors.iter().filter_map(member).collect())
}

fn member(value: &Value) -> Option<Member> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default();
    let flag = |key: &str| value.get(key).and_then(Value::as_bool);
    let id = value.get("id")?.as_str()?.to_owned();
    let native = !text("external_session_id").is_empty();
    let enabled = flag("enabled").unwrap_or(true);
    let running = flag("running").unwrap_or(false);
    let state = match (native, enabled, running) {
        (true, false, _) => MemberState::AccessPaused,
        (true, true, true) => MemberState::Ready,
        (true, true, false) => MemberState::NotReceiving,
        (false, false, _) => MemberState::Off,
        (false, true, true) => MemberState::Running,
        (false, true, false) => MemberState::Stopped,
    };
    Some(Member {
        id,
        runtime: runtime_label(text("runtime")),
        role: text("role").to_owned(),
        native,
        state,
    })
}

/// Short runtime names, matching the session list's for the native four.
fn runtime_label(runtime: &str) -> String {
    let Ok(known) = serde_json::from_value::<ActorRuntime>(Value::String(runtime.to_owned()))
    else {
        return runtime.to_owned();
    };
    match known {
        ActorRuntime::Amp => "Amp",
        ActorRuntime::Antigravity => "Antigravity",
        ActorRuntime::Auggie => "Auggie",
        ActorRuntime::Claude => "Claude Code",
        ActorRuntime::Cline => "Cline",
        ActorRuntime::Codex => "Codex",
        ActorRuntime::Deepseek => "DeepSeek",
        ActorRuntime::Copilot => "Copilot",
        ActorRuntime::Cursor => "Cursor",
        ActorRuntime::Devin => "Devin",
        ActorRuntime::Kiro => "Kiro",
        ActorRuntime::Kilo => "Kilo Code",
        ActorRuntime::Droid => "Droid",
        ActorRuntime::Grok => "Grok Build",
        ActorRuntime::Hermes => "Hermes",
        ActorRuntime::Kimi => "Kimi Code",
        ActorRuntime::Opencode => "OpenCode",
        ActorRuntime::Pi => "Pi",
        ActorRuntime::WebModel => "ChatGPT Web",
        ActorRuntime::ChatgptDots => "ChatGPT Dots",
        ActorRuntime::GrokWebModel => "Grok Web",
        ActorRuntime::Custom => "Custom",
    }
    .to_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GroupAction {
    /// `group_start`, worded as resuming for a paused group.
    Start {
        resume: bool,
    },
    Stop,
}

impl GroupAction {
    fn op(self) -> &'static str {
        match self {
            Self::Start { .. } => "group_start",
            Self::Stop => "group_stop",
        }
    }

    pub(super) fn verb(self) -> &'static str {
        match self {
            Self::Start { resume: false } => "start",
            Self::Start { resume: true } => "resume",
            Self::Stop => "stop",
        }
    }

    pub(super) fn doing(self) -> &'static str {
        match self {
            Self::Start { resume: false } => "Starting",
            Self::Start { resume: true } => "Resuming",
            Self::Stop => "Stopping",
        }
    }
}

/// One confirmed action, fixed to the group it was confirmed for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GroupCommand {
    pub(super) serial: u64,
    pub(super) group_id: String,
    pub(super) name: String,
    pub(super) action: GroupAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Done,
    Failed(String),
    /// The request reached the daemon but no answer came back, so it may or
    /// may not have taken effect.
    Unknown(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ActionProgress {
    /// The action waiting for the daemon, if any.
    pub(super) running: Option<GroupCommand>,
    /// How the latest action ended.
    pub(super) last: Option<(GroupCommand, Outcome)>,
}

/// Carry out confirmed actions one at a time until `stop` turns true or the
/// interface goes away. Stopping only stops waiting: a request already sent
/// is the daemon's to finish, and the action stays recorded as running.
pub(super) async fn act(
    home: HomeLayout,
    mut commands: mpsc::UnboundedReceiver<GroupCommand>,
    progress: watch::Sender<ActionProgress>,
    mut stop: watch::Receiver<bool>,
) {
    let client = DaemonClient::new(home);
    loop {
        let command = tokio::select! {
            command = commands.recv() => match command {
                Some(command) => command,
                None => return,
            },
            _ = stop.wait_for(|stop| *stop) => return,
        };
        progress.send_modify(|progress| progress.running = Some(command.clone()));
        let outcome = tokio::select! {
            outcome = run(&client, &command) => outcome,
            _ = stop.wait_for(|stop| *stop) => return,
        };
        progress.send_modify(|progress| {
            progress.running = None;
            progress.last = Some((command, outcome));
        });
    }
}

async fn run(client: &DaemonClient, command: &GroupCommand) -> Outcome {
    let mut args = Map::new();
    args.insert("group_id".into(), Value::String(command.group_id.clone()));
    args.insert("by".into(), Value::String("user".into()));
    let request = DaemonRequest {
        v: 1,
        op: command.action.op().into(),
        args,
    };
    match client.call(&request).await {
        Ok(response) if response.ok => Outcome::Done,
        Ok(response) => Outcome::Failed(daemon::error_text(response.error)),
        Err(ClientError::OutcomeUnknown { message, .. }) => Outcome::Unknown(message),
        Err(error) => Outcome::Failed(format!("daemon unavailable ({error})")),
    }
}

#[cfg(test)]
#[path = "groups_tests.rs"]
mod tests;
