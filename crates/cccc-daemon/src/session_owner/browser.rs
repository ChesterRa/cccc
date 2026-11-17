//! Browser resources use the same holder as member processes. The executable
//! supplies the existing browser launcher; the holder owns the returned handle.

use anyhow::Result;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserMode {
    Headless,
    System { background: bool },
}

/// Normalized proxy settings, never arbitrary Chromium arguments or credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserProxy {
    pub server: String,
    pub bypass_list: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserLaunchSpec {
    pub profile: PathBuf,
    pub mode: BrowserMode,
    pub width: u32,
    pub height: u32,
    pub proxy: Option<BrowserProxy>,
}

impl BrowserLaunchSpec {
    /// Shared browsers keep the first launch's dimensions; later surfaces
    /// choose their own viewport without replacing the underlying browser.
    pub(crate) fn same_browser(&self, other: &Self) -> bool {
        self.profile == other.profile && self.mode == other.mode && self.proxy == other.proxy
    }
}

/// Connection and presentation data. This is private local IPC, not a public
/// health response: a CDP endpoint grants control of the browser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserConnection {
    pub websocket_url: String,
    pub display: Option<String>,
    pub vnc_port: Option<u16>,
    pub strategy: String,
    pub metadata: Value,
    pub viewer: Value,
}

pub trait BrowserLauncher: Send + Sync {
    fn launch(&self, spec: BrowserLaunchSpec) -> BoxFuture<'_, Result<Box<dyn BrowserProcess>>>;
}

/// Keeps the profile lease and browser/display children until an explicit stop.
/// A failed stop must leave the handle available for observation and retry.
pub trait BrowserProcess: Send {
    fn pid(&self) -> u32;
    fn connection(&self) -> &BrowserConnection;
    fn stop(&mut self) -> BoxFuture<'_, Result<()>>;
    fn initialization_error(&self) -> Option<&str> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserActorIdentity {
    pub group_id: String,
    pub actor_id: String,
    pub actor_created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPage {
    pub key: String,
    pub actor: Option<BrowserActorIdentity>,
    pub target_id: String,
    /// Web's existing session fields, preserved verbatim rather than
    /// interpreted as a second copy of its delivery state.
    pub state: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserResourceState {
    Pending,
    Ready,
    Exited,
    Unconfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserResource {
    pub resource_id: String,
    pub owner_id: String,
    pub instance: String,
    #[serde(flatten)]
    pub spec: BrowserLaunchSpec,
    #[serde(flatten)]
    pub connection: Option<BrowserConnection>,
    pub state: BrowserResourceState,
    pub error: Option<String>,
    pub sessions: Vec<BrowserPage>,
}
