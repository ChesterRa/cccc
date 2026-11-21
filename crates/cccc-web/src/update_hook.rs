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

/// What an attended update starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateSource {
    /// A published release of this version.
    Release { version: String },
    /// The program at the path the host was started from, as the check
    /// that offered it saw it. The host takes the path from its own record.
    Local { token: String, source_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRequest {
    pub source: UpdateSource,
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

    /// Whether this host can replace its own installation with a release: only
    /// one the CCCC installer put in place can.
    fn release_in_place(&self) -> bool;

    /// Whether this host can restart using the program at its launch path.
    fn local_restart(&self) -> bool {
        false
    }

    /// Whether this host offers to keep agents running through a release
    /// update. The release itself is checked only once it is downloaded.
    fn release_keep(&self) -> bool {
        false
    }

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

/// An attended update's result as the host recorded it, with the agents it
/// stopped or kept as the host found them once CCCC was back.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LastUpdate {
    /// The update this is the result of, as its start answered.
    pub transaction: String,
    pub from_version: Option<String>,
    pub to_version: String,
    pub mode: Option<String>,
    /// `installed`, `restarted`, `abandoned`, `not_started`, `unconfirmed`,
    /// `undone` or `kept`.
    pub result: String,
    pub stage: Option<String>,
    pub reason: Option<String>,
    pub finished_at: String,
    /// `release` or `local`.
    pub source: Option<String>,
    /// For a local restart, the identity token of the file it started.
    pub token: Option<String>,
    /// The build it moved to, as its self-check named it.
    pub source_id: Option<String>,
    pub agents: UpdateAgents,
}

/// What became of the agents an update stopped or kept, as the host found them
/// within a few seconds of CCCC starting again. Each agent is in one list.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateAgents {
    /// `observing` while the host still looks for them, `observed` when every
    /// agent is accounted for, `unconfirmed` when some could not be, or
    /// `not_observed` when the host did not look.
    pub state: String,
    /// Kept running, and found with the same launch.
    pub reattached: Vec<UpdateAgent>,
    /// Running again with a launch of their own.
    pub restarted: Vec<UpdateAgent>,
    /// Stopped or kept by the update, and found not running.
    pub not_back: Vec<UpdateAgent>,
    /// Whether they run could not be told.
    pub unconfirmed: Vec<UpdateAgent>,
}

/// One agent: who it is, the launch the host saw, and its title.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateAgent {
    pub group_id: String,
    pub actor_id: String,
    pub actor_created_at: String,
    /// The launch found running again, or for an agent not found, the one
    /// the update stopped or kept.
    pub generation: Option<String>,
    pub owner_id: Option<String>,
    /// The agent's current title, while it is still the same agent.
    pub title: Option<String>,
}

/// Which checks the administrator asked for. Unqualified requests retain the
/// combined release and rebuild check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckScope {
    #[default]
    All,
    Release,
    Local,
}

/// A release or local restart check, made when an administrator asks for it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
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
    /// The program at the launch path, on a host that can restart using it.
    pub local: Option<LocalBuild>,
}

/// What is now at the path this host was started from, read locally.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LocalBuild {
    /// The program the host is running.
    Current {
        #[serde(flatten)]
        candidate: LocalCandidate,
        /// The result to watch for if the reply to the start is lost.
        transaction: String,
    },
    /// Another program, which answered its self-check.
    Rebuilt {
        #[serde(flatten)]
        candidate: LocalCandidate,
        /// When that file was last written, in UTC.
        built_at: String,
    },
    /// The program identified itself and cannot complete the current host's restart.
    Incompatible { version: String },
    /// The program at the launch path could not be checked.
    Unreadable { reason: String },
}

/// A checked program and the host's offer to restart into it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LocalCandidate {
    pub version: String,
    pub source_id: String,
    /// A value to compare when confirming this offer. It grants no authority.
    pub token: String,
    pub keep: KeepPreview,
}

/// Whether agents can keep running through a local restart,
/// as checked now: the daemon's own decision, sent as it is. A start checks
/// again. Each refusal names a member or session holder, or all of them when
/// it names no one; the page words a code it knows and shows CCCC's own
/// sentence for any other.
pub type KeepPreview = cccc_core::update_capabilities::KeepDecision;
pub use cccc_core::update_capabilities::KeepRefusal;

impl LocalBuild {
    pub fn rebuilt(
        version: String,
        source_id: String,
        written: std::time::SystemTime,
        token: String,
        keep: KeepPreview,
    ) -> Self {
        Self::Rebuilt {
            candidate: LocalCandidate {
                version,
                source_id,
                token,
                keep,
            },
            built_at: chrono::DateTime::<chrono::Utc>::from(written)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }
    }

    pub fn candidate(&self) -> Option<&LocalCandidate> {
        match self {
            Self::Current { candidate, .. } | Self::Rebuilt { candidate, .. } => Some(candidate),
            Self::Incompatible { .. } | Self::Unreadable { .. } => None,
        }
    }

    /// How long ago the rebuilt program was written, when that is known.
    pub fn built_ago(&self) -> Option<std::time::Duration> {
        let Self::Rebuilt { built_at, .. } = self else {
            return None;
        };
        let written = chrono::DateTime::parse_from_rfc3339(built_at).ok()?;
        (chrono::Utc::now() - written.with_timezone(&chrono::Utc))
            .to_std()
            .ok()
    }
}

/// The host's update check, offered whenever a CCCC host serves this Web,
/// including hosts that cannot replace themselves.
pub trait UpdateChecker: Send + Sync {
    fn status(&self) -> UpdateStatus;

    fn check(&self, scope: UpdateCheckScope) -> Pin<Box<dyn Future<Output = UpdateCheck> + Send>>;
}

pub type UpdateCheckHook = Arc<dyn UpdateChecker>;

/// What the supervising host offers this Web for updates.
#[derive(Clone, Default)]
pub struct UpdateServices {
    pub checker: Option<UpdateCheckHook>,
    /// Only a host that can replace or restart itself in place offers this.
    pub host: Option<UpdateHook>,
    /// Given each serve's in-process entry, when `host` is offered.
    pub on_trigger: Option<crate::TriggerCallback>,
}
