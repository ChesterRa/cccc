use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::handler::viewport::Viewport;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::path::Path;
use tokio::task::JoinHandle;

use super::{
    BrowserMode, profile_owner::ProfileLease, proxy::BrowserProxy, stop_browser,
    system_browser::SystemBrowserLaunch,
};

/// Process resources have one owner even when several surfaces share its pages.
pub(super) struct BrowserOwner {
    pub browser: Browser,
    pub handler: JoinHandle<()>,
    pub system_browser: Option<SystemBrowserLaunch>,
    profile_lease: Option<ProfileLease>,
    pub shared: bool,
    pub strategy: String,
    pub metadata: Value,
    pub viewer: Value,
}

impl BrowserOwner {
    pub async fn launch(
        profile: &Path,
        width: u32,
        height: u32,
        mode: BrowserMode,
        shared: bool,
        storage_state: Option<&Value>,
    ) -> Result<Self> {
        let mut profile_lease = ProfileLease::acquire(profile).await?;
        let mut system_browser = match mode {
            BrowserMode::Headless => None,
            BrowserMode::System { background } => {
                Some(SystemBrowserLaunch::prepare(width, height, background).await?)
            }
        };
        let proxy_args = BrowserProxy::from_env()?
            .map(|proxy| proxy.chromium_args())
            .unwrap_or_default();
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
                let config = config.build().map_err(anyhow::Error::msg)?;
                Browser::launch(config)
                    .await
                    .map(|(mut browser, handler)| {
                        let pid = browser
                            .get_mut_child()
                            .and_then(|child| child.as_mut_inner().id())
                            .unwrap_or_default();
                        (browser, handler, pid)
                    })
                    .map_err(anyhow::Error::from)
            }
        };
        let (mut browser, mut handler, browser_pid) = match launched {
            Ok(browser) => browser,
            Err(error) => {
                if let Some(system_browser) = &mut system_browser {
                    system_browser.stop().await;
                }
                return Err(error);
            }
        };
        let recorded = if system_browser.is_some() {
            profile_lease.record_pid(browser_pid).await
        } else {
            profile_lease.record_browser(&mut browser).await
        };
        if let Err(error) = recorded {
            let _ = browser.kill().await;
            if let Some(system_browser) = &mut system_browser {
                system_browser.stop().await;
            }
            return Err(error);
        }
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
            shared,
            strategy,
            metadata,
            viewer,
        };
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
                owner
                    .stop()
                    .await
                    .context("clean up browser after cookie initialization failed")?;
                return Err(error);
            }
        }
        Ok(owner)
    }

    pub async fn stop(&mut self) -> Result<()> {
        stop_browser(&mut self.browser, &mut self.handler).await?;
        if let Some(system) = &mut self.system_browser {
            system.stop().await;
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
