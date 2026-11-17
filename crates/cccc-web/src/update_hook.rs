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
