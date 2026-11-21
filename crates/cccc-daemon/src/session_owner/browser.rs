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

/// Programs selected for a new launch. The holder does not store this request
/// with the resource or compare it when reusing a browser.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserPrograms {
    #[serde(default, with = "program_path")]
    pub browser: Option<PathBuf>,
    pub channel: Option<String>,
    #[serde(default, with = "program_path")]
    pub xvfb: Option<PathBuf>,
    #[serde(default, with = "program_path")]
    pub x11vnc: Option<PathBuf>,
    pub desktop_view: bool,
}

// Keep ordinary paths readable in local IPC and preserve native paths that
// cannot be represented as JSON strings.
mod program_path {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::{ffi::OsString, path::PathBuf};

    pub fn serialize<S: Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match path {
            Some(path) if path.to_str().is_none() => path.as_os_str().serialize(serializer),
            path => path.serialize(serializer),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum WirePath {
            Text(String),
            Native(OsString),
        }
        Ok(
            Option::<WirePath>::deserialize(deserializer)?.map(|path| match path {
                WirePath::Text(text) => PathBuf::from(text),
                WirePath::Native(native) => PathBuf::from(native),
            }),
        )
    }
}

#[cfg(unix)]
impl BrowserPrograms {
    pub(super) fn valid(&self) -> bool {
        [&self.browser, &self.xvfb, &self.x11vnc]
            .into_iter()
            .flatten()
            .all(|path| path.is_absolute())
            && self
                .channel
                .as_deref()
                .is_none_or(|channel| matches!(channel, "chrome" | "msedge" | "chromium"))
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
    fn launch(
        &self,
        spec: BrowserLaunchSpec,
        programs: Option<BrowserPrograms>,
    ) -> BoxFuture<'_, Result<Box<dyn BrowserProcess>>>;
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    #[test]
    fn browser_programs_preserve_native_paths_and_require_absolute_paths_and_known_channels() {
        let browser = PathBuf::from(OsString::from_vec(b"/tmp/\xff/chrome".to_vec()));
        let programs = BrowserPrograms {
            browser: Some(browser.clone()),
            channel: Some("chrome".into()),
            xvfb: None,
            x11vnc: Some("/usr/bin/x11vnc".into()),
            desktop_view: true,
        };
        let value = serde_json::to_value(programs).expect("encode browser programs");
        assert_eq!(value["x11vnc"], "/usr/bin/x11vnc");
        let mut decoded: BrowserPrograms =
            serde_json::from_value(value).expect("decode browser programs");
        assert_eq!(decoded.browser.as_ref(), Some(&browser));
        assert!(decoded.valid());
        decoded.channel = Some("other".into());
        assert!(!decoded.valid());
        decoded.channel = None;
        decoded.xvfb = Some("relative/Xvfb".into());
        assert!(!decoded.valid());
    }
}
