use anyhow::{Context, Result};
use cccc_daemon::session_owner::browser::BrowserPrograms;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::handler::viewport::Viewport;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::path::Path;
use tokio::task::JoinHandle;

use super::{
    BrowserMode, profile_owner::ProfileLease, proxy::BrowserProxy, resources::HeldResource,
    stop_browser, system_browser::SystemBrowserLaunch,
};

/// Process resources have one owner even when several surfaces share its pages.
pub(super) struct BrowserOwner {
    pub browser: Browser,
    pub handler: JoinHandle<()>,
    pub system_browser: Option<SystemBrowserLaunch>,
    profile_lease: Option<ProfileLease>,
    /// The browser process this owner started; `None` for one it connected to.
    pub pid: Option<u32>,
    /// Set when the session owner keeps the browser and this is only a
    /// connection to it: the owner stops it, never this Web.
    pub held: Option<HeldResource>,
    pub shared: bool,
    pub strategy: String,
    pub metadata: Value,
    pub viewer: Value,
}

/// A launch that failed after the browser started. `started` still holds
/// what was started, so the caller decides whether it is stopped or kept.
pub(super) struct StartFailure {
    pub error: anyhow::Error,
    pub started: Option<BrowserOwner>,
}

impl BrowserOwner {
    pub async fn launch(
        profile: &Path,
        width: u32,
        height: u32,
        mode: BrowserMode,
        shared: bool,
        storage_state: Option<&Value>,
        proxy: Option<BrowserProxy>,
    ) -> Result<Self> {
        match Self::start(
            profile,
            (width, height),
            mode,
            super::system_browser::programs(mode),
            shared,
            storage_state,
            proxy,
        )
        .await
        {
            Ok(owner) => Ok(owner),
            Err(StartFailure {
                error,
                started: None,
            }) => Err(error),
            Err(StartFailure {
                error,
                started: Some(mut owner),
            }) => {
                owner
                    .stop()
                    .await
                    .with_context(|| format!("clean up the browser after: {error:#}"))?;
                Err(error)
            }
        }
    }

    pub async fn start(
        profile: &Path,
        size: (u32, u32),
        mode: BrowserMode,
        programs: BrowserPrograms,
        shared: bool,
        storage_state: Option<&Value>,
        proxy: Option<BrowserProxy>,
    ) -> std::result::Result<Self, StartFailure> {
        let (width, height) = size;
        let not_started = |error| StartFailure {
            error,
            started: None,
        };
        let profile_lease = ProfileLease::acquire(profile).await.map_err(not_started)?;
        let browser_program = programs.browser.clone();
        let mut system_browser = match mode {
            BrowserMode::Headless => None,
            BrowserMode::System { background } => Some(
                SystemBrowserLaunch::prepare(width, height, background, programs)
                    .await
                    .map_err(not_started)?,
            ),
        };
        let proxy_args = proxy.map(|proxy| proxy.chromium_args()).unwrap_or_default();
        let launched = match &mut system_browser {
            Some(system_browser) => system_browser.launch(profile, proxy_args).await,
            None => {
                let mut config = BrowserConfig::builder()
                    .user_data_dir(profile)
                    .window_size(width, height)
                    .viewport(Viewport {
                        width,
                        height,
                        ..Viewport::default()
                    })
                    .new_headless_mode();
                if !proxy_args.is_empty() {
                    config = config.args(proxy_args);
                }
                match browser_program
                    .ok_or_else(|| "Could not auto detect a chrome executable".to_owned())
                    .and_then(|path| config.chrome_executable(path).build())
                {
                    Ok(config) => Browser::launch(config)
                        .await
                        .map(|(mut browser, handler)| {
                            let pid = browser
                                .get_mut_child()
                                .and_then(|child| child.as_mut_inner().id())
                                .unwrap_or_default();
                            (browser, handler, pid)
                        })
                        .map_err(anyhow::Error::from),
                    Err(error) => Err(anyhow::Error::msg(error)),
                }
            }
        };
        let (browser, mut handler, browser_pid) = match launched {
            Ok(browser) => browser,
            Err(error) => {
                // Display children left running end when this launch drops.
                let error = match &mut system_browser {
                    Some(system_browser) => match system_browser.stop().await {
                        Ok(()) => error,
                        Err(stop_error) => anyhow::anyhow!(
                            "{error:#}; stopping its display also failed: {stop_error:#}"
                        ),
                    },
                    None => error,
                };
                // A browser macOS opened that no stop confirmed ended stays in
                // the profile lease, so the next acquire verifies and stops it.
                #[cfg(target_os = "macos")]
                let error = match system_browser
                    .as_ref()
                    .and_then(SystemBrowserLaunch::opened)
                {
                    None => error,
                    Some(opened) => {
                        let mut profile_lease = profile_lease;
                        match profile_lease
                            .record_unstopped_browser(opened.process.as_ref())
                            .await
                        {
                            Ok(()) => error,
                            Err(record_error) => anyhow::anyhow!(
                                "{error:#}; the browser may still be running and could not be recorded: {record_error:#}"
                            ),
                        }
                    }
                };
                return Err(not_started(error));
            }
        };
        let handler = tokio::spawn(async move {
            while let Some(message) = handler.next().await {
                if message.is_err() {
                    break;
                }
            }
        });
        let (strategy, metadata) = system_browser.as_ref().map_or_else(
            || {
                (
                    "cdp_screencast".to_owned(),
                    json!({"visibility":"headless","display_owned":false,"pid":browser_pid}),
                )
            },
            |system| (system.strategy(), system.metadata(browser_pid, profile)),
        );
        let viewer = system_browser.as_ref().map_or_else(
            || json!({"kind":"screencast","vnc":{"available":false,"error":"unsupported_surface"}}),
            SystemBrowserLaunch::viewer,
        );
        let mut owner = Self {
            browser,
            handler,
            system_browser,
            profile_lease: Some(profile_lease),
            pid: Some(browser_pid),
            held: None,
            shared,
            strategy,
            metadata,
            viewer,
        };
        // From here on the browser runs: a failure hands it back with the error.
        let recorded = match owner.profile_lease.as_mut() {
            Some(lease) if owner.system_browser.is_some() => lease.record_pid(browser_pid).await,
            Some(lease) => lease.record_browser(&mut owner.browser).await,
            None => Ok(()),
        };
        if let Err(error) = recorded {
            return Err(StartFailure {
                error,
                started: Some(owner),
            });
        }
        if let Some(cookies) = storage_state
            .and_then(|state| state.get("cookies"))
            .cloned()
        {
            let seeded = async {
                let cookies: Vec<CookieParam> =
                    serde_json::from_value(cookies).context("decode saved browser cookies")?;
                if !cookies.is_empty() {
                    owner.browser.set_cookies(cookies).await?;
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;
            if let Err(error) = seeded {
                return Err(StartFailure {
                    error,
                    started: Some(owner),
                });
            }
        }
        Ok(owner)
    }

    /// Connects to a browser the session owner keeps. This Web owns only the
    /// CDP connection; the owner keeps the process, its profile and display.
    pub async fn adopt(preserved: &super::PreservedBrowser, held: HeldResource) -> Result<Self> {
        let (browser, mut handler) = Browser::connect(preserved.websocket_url.clone())
            .await
            .context("connect to the preserved browser")?;
        let handler = tokio::spawn(async move {
            while let Some(message) = handler.next().await {
                if message.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            browser,
            handler,
            system_browser: None,
            profile_lease: None,
            pid: None,
            held: Some(held),
            shared: preserved.shared,
            strategy: preserved.strategy.clone(),
            metadata: preserved.metadata.clone(),
            viewer: preserved.viewer.clone(),
        })
    }

    /// Whether the browser draws on a desktop or virtual display rather
    /// than headless, whoever runs it.
    pub fn is_system_browser(&self) -> bool {
        self.system_browser.is_some() || self.held.as_ref().is_some_and(HeldResource::system)
    }

    pub fn vnc_port(&self) -> Option<u16> {
        if !super::system_browser::vnc_viewer_enabled() {
            return None;
        }
        self.system_browser
            .as_ref()
            .and_then(SystemBrowserLaunch::vnc_port)
            .or_else(|| self.held.as_ref().and_then(|held| held.vnc_port))
    }

    /// Closes only this Web's connection; the browser and its pages stay.
    pub async fn detach(&mut self) {
        self.handler.abort();
        let _ = (&mut self.handler).await;
    }

    pub async fn stop(&mut self) -> Result<()> {
        stop_browser(&mut self.browser, &mut self.handler).await?;
        // A failure keeps the lease and what failed, so a retry finishes.
        if let Some(system) = &mut self.system_browser {
            system.stop().await?;
        }
        if let Some(lease) = &mut self.profile_lease {
            lease.clear_owner()?;
        }
        // Other in-flight operations may retain an Arc to this stopped owner.
        // Release its lease only after process cleanup, not when the last Arc drops.
        self.profile_lease = None;
        Ok(())
    }
}
