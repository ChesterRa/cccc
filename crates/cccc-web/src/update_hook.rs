//! The Web side of an attended update. The host that supervises this Web
//! process prepares and carries out the update; the Web only checks who asks
//! and what is asked, then hands the request over.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Whether running agents are kept or restarted while CCCC is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    Keep,
    Restart,
}

impl UpdateMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Restart => "restart",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "keep" => Some(Self::Keep),
            "restart" => Some(Self::Restart),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateRequest {
    pub version: String,
    pub mode: UpdateMode,
}

/// Why the host did not start an update. Nothing was stopped or replaced.
#[derive(Debug, Clone)]
pub struct UpdateRejection {
    pub code: String,
    pub message: String,
}

/// An update the host has reserved and is preparing in the background.
pub struct UpdatePreparation {
    pub transaction: String,
    /// Resolves once the update is ready to be carried out. Dropping this does
    /// not stop the preparation; [`UpdateHost::abandon`] stops it at once and
    /// undoes what it did.
    pub ready: Pin<Box<dyn Future<Output = Result<(), UpdateRejection>> + Send>>,
}

/// The supervising host's side of an attended update.
pub trait UpdateHost: Send + Sync {
    /// Reserves the host's one update and starts preparing it while this Web
    /// keeps serving. Rejects at once when another update holds the host.
    fn begin(&self, request: UpdateRequest) -> Result<UpdatePreparation, UpdateRejection>;

    /// Gives up a reserved update this Web did not hand over: a running
    /// preparation stops at once, and what it did is undone. Does nothing for
    /// another or an already finished one.
    fn abandon(&self, transaction: &str);
}

/// Once a prepared update is handed over, this Web closes and returns
/// [`crate::ServeOutcome::UpdateRequested`] so the host can finish it.
pub type UpdateHook = Arc<dyn UpdateHost>;

/// Local facts about this CCCC's updates. Reading them never uses the network.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateStatus {
    pub current: String,
    /// `standalone`, `pip`, `unmanaged` or `unrecognized`.
    pub installation: String,
    /// The latest attended update's recorded result, if there is one.
    pub last: Option<LastUpdate>,
}

/// An attended update's result as the host recorded it. It says nothing about
/// whether agents came back: the host does not observe them.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LastUpdate {
    /// The update this is the result of, as its start answered.
    pub transaction: String,
    pub from_version: Option<String>,
    pub to_version: String,
    pub mode: Option<String>,
    /// `installed`, `abandoned`, `not_started`, `unconfirmed`, `undone` or `kept`.
    pub result: String,
    pub stage: Option<String>,
    pub reason: Option<String>,
    pub finished_at: String,
}

/// One release check, made when an administrator asks for it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UpdateCheck {
    pub current: String,
    /// `stable` or `rc`.
    pub channel: String,
    /// `standalone`, `pip`, `unmanaged` or `unrecognized`.
    pub installation: String,
    /// Absent when the check failed; whether an update exists is then unknown.
    pub latest: Option<String>,
    /// `newer`, `channel_switch`, `current`, `installed_newer` or `no_package`;
    /// absent when the check failed.
    pub status: Option<String>,
    /// Why the latest version is unknown.
    pub failure: Option<String>,
    pub release_notes_url: Option<String>,
    /// The command that updates this installation by hand, when there is one.
    pub command: Option<String>,
}

/// The host's update check, offered whenever a CCCC host serves this Web,
/// including hosts that cannot replace themselves.
pub trait UpdateChecker: Send + Sync {
    fn status(&self) -> UpdateStatus;

    /// Asks release discovery once. A failure is reported, never read as current.
    fn check(&self) -> Pin<Box<dyn Future<Output = UpdateCheck> + Send>>;
}

pub type UpdateCheckHook = Arc<dyn UpdateChecker>;

/// What the supervising host offers this Web for updates.
#[derive(Clone, Default)]
pub struct UpdateServices {
    pub checker: Option<UpdateCheckHook>,
    /// Only a host that can replace itself in place offers this.
    pub host: Option<UpdateHook>,
    /// Given each serve's in-process entry, when `host` is offered.
    pub on_trigger: Option<crate::TriggerCallback>,
}
