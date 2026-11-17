use cccc_contracts::{Actor, Event};
use cccc_core::GroupDoc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) const CONTRACT: u32 = 1;
pub(super) const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// An operation belongs to one live owner instance and one control stream.
/// A sequence is allocated when that stream writes, never by a worker in advance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct OperationId {
    pub instance: String,
    pub epoch: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct ActorRef {
    pub group_id: String,
    pub actor_id: String,
    pub actor_created_at: String,
    pub generation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct OperationSource {
    pub actor: ActorRef,
    pub event_ids: Vec<String>,
    /// Detect a caller accidentally reusing source references for other input.
    pub input_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(super) enum OperationState {
    Pending,
    Finished {
        result: Result<Value, Failure>,
    },
    /// Terminal: this closed epoch can no longer admit this sequence.
    NotAdmitted,
    /// Missing detail is never permission to submit the input again.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Failure {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Failure {}

impl Failure {
    pub(super) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum PreparedLaunch {
    Managed {
        group: GroupDoc,
        actor: Actor,
        host_environment: std::collections::BTreeMap<String, String>,
    },
    Pty {
        spec: cccc_runtime::LaunchSpec,
        history: cccc_runtime::HistoryConfig,
        launch: cccc_runtime::ActorLaunch,
        preamble: String,
    },
    DeepSeek {
        group: GroupDoc,
        actor: Actor,
        cwd: std::path::PathBuf,
        private_values: Vec<String>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Mutation {
    Start {
        actor: ActorRef,
        launch: Box<PreparedLaunch>,
    },
    Stop {
        actor: ActorRef,
    },
    StopAll,
    Submit {
        source: OperationSource,
        group: Box<GroupDoc>,
        actor: Box<Actor>,
        events: Vec<Event>,
        text: String,
    },
    Permission {
        actor: ActorRef,
        request_id: String,
        allow: bool,
    },
    Interaction {
        actor: ActorRef,
        request_id: String,
        reply: Value,
    },
    CancelTurn {
        actor: ActorRef,
    },
    EnsureViewer {
        actor: ActorRef,
    },
    DetachViewer {
        actor: ActorRef,
    },
    PauseConfirmation {
        actor: ActorRef,
    },
    RetryConfirmation {
        actor: ActorRef,
    },
    Acknowledge {
        operation: OperationId,
    },
    Terminal {
        actor: ActorRef,
        action: TerminalAction,
    },
    Retire,
    Recovery {
        action: super::RecoveryAction,
    },
}

impl Mutation {
    pub(super) fn source(&self) -> Option<&OperationSource> {
        match self {
            Self::Submit { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(super) enum Request {
    Hello {
        contract: u32,
    },
    Acquire {
        contract: u32,
        instance: String,
    },
    Mutate {
        epoch: u64,
        sequence: u64,
        mutation: Box<Mutation>,
    },
    Snapshot,
    RecoveryList,
    Query {
        operation: OperationId,
    },
    Find {
        source: OperationSource,
    },
    FindEvent {
        group_id: String,
        actor_id: String,
        actor_created_at: String,
        event_id: String,
    },
    TerminalRead {
        actor: ActorRef,
        read: TerminalRead,
    },
    TerminalStream {
        actor: ActorRef,
        epoch: u64,
        request: cccc_contracts::DaemonRequest,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub(super) enum Response {
    Hello {
        contract: u32,
        instance: String,
        pid: u32,
        build: Value,
        accepting_starts: bool,
    },
    Acquired {
        epoch: u64,
    },
    Admitted {
        operation: OperationId,
    },
    Snapshot {
        actors: Vec<ActorSnapshot>,
    },
    Operation {
        state: OperationState,
    },
    Found {
        operations: Vec<OperationId>,
    },
    FoundEvent {
        operations: Vec<ExecutionFact>,
    },
    Acknowledged,
    Retired,
    Recovery {
        value: Value,
    },
    Terminal {
        value: Value,
    },
    Failed {
        error: Failure,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum TerminalRead {
    History {
        before: Option<u64>,
        limit: usize,
    },
    Retained,
    RetainedTail {
        limit: usize,
    },
    Since {
        after: u64,
        limit: usize,
    },
    Replay {
        after: u64,
        end: Option<u64>,
        limit: usize,
    },
    LastOutput,
    BracketedPaste,
    Writable {
        attachment_id: u64,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum TerminalAction {
    Write {
        data: Vec<u8>,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    ResizeAttachment {
        attachment_id: u64,
        cols: u16,
        rows: u16,
    },
    Clear,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ExecutionFact {
    pub operation: OperationId,
    pub source: OperationSource,
    pub state: OperationState,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ActorSnapshot {
    pub actor: ActorRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) observation_error: Option<Failure>,
    pub owner: Option<crate::ops::actor_runtime::RegisteredOwner>,
    pub terminal: Option<cccc_runtime::SessionStatus>,
    // SessionStatus's public encoding deliberately omits the private launch.
    pub terminal_launch: Option<cccc_runtime::ActorLaunch>,
    pub managed_status: Option<crate::ops::local_headless::HeadlessStatus>,
    pub provider_exited: bool,
    pub workspace: Option<crate::ops::actor_runtime::LaunchWorkspace>,
    pub structured: Option<Value>,
    pub ready_for_mail_notice: bool,
    pub waiting_for_workspace_trust: bool,
    pub confirmation_terminal: bool,
    pub confirmation_pending: bool,
    pub confirmation_paused: bool,
}
