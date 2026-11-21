//! The browser launch the Web already uses, run inside the session owner so
//! that a browser, its profile lease and its display belong to the owner and
//! outlive the Web. The Web then holds only a CDP connection to it.

use anyhow::{Result, anyhow};
use cccc_daemon::session_owner::browser::{
    BrowserConnection, BrowserLaunchSpec, BrowserLauncher, BrowserProcess, BrowserPrograms,
};
use futures_util::future::BoxFuture;

use super::{
    owner::{BrowserOwner, StartFailure},
    proxy::BrowserProxy,
};

pub(crate) struct Launcher;

impl BrowserLauncher for Launcher {
    fn launch(
        &self,
        spec: BrowserLaunchSpec,
        programs: Option<BrowserPrograms>,
    ) -> BoxFuture<'_, Result<Box<dyn BrowserProcess>>> {
        Box::pin(async move {
            let proxy = spec.proxy.map(BrowserProxy::from_owner).transpose()?;
            // Saved cookies are the Web's to seed, over its own connection;
            // `shared` only matters to a Web that opens pages.
            let started = BrowserOwner::start(
                &spec.profile,
                (spec.width, spec.height),
                spec.mode,
                programs.unwrap_or_else(|| super::system_browser::programs(spec.mode)),
                true,
                None,
                proxy,
            )
            .await;
            let (mut owner, problem) = match started {
                Ok(owner) if owner.pid.is_some_and(|pid| pid != 0) => (owner, None),
                Ok(owner) => (
                    owner,
                    Some(anyhow!(
                        "the browser started without a process ID to hold it by"
                    )),
                ),
                Err(StartFailure {
                    error,
                    started: None,
                }) => return Err(error),
                Err(StartFailure {
                    error,
                    started: Some(owner),
                }) => (owner, Some(error)),
            };
            let initialization_error = match problem {
                None => None,
                Some(error) => match owner.stop().await {
                    // Nothing it started is left: an ordinary failed launch.
                    Ok(()) => return Err(error),
                    // Something may still run, so the owner keeps the handle
                    // to stop it later rather than losing it here.
                    Err(stop_error) => Some(format!(
                        "{error:#}; stopping what it started also failed: {stop_error:#}"
                    )),
                },
            };
            let pid = owner.pid.unwrap_or_default();
            let connection = BrowserConnection {
                websocket_url: owner.browser.websocket_address().clone(),
                display: owner
                    .system_browser
                    .as_ref()
                    .and_then(|system| system.display_name()),
                vnc_port: owner
                    .system_browser
                    .as_ref()
                    .and_then(|system| system.vnc_port()),
                strategy: owner.strategy.clone(),
                metadata: owner.metadata.clone(),
                viewer: owner.viewer.clone(),
            };
            Ok(Box::new(Held {
                owner,
                pid,
                connection,
                initialization_error,
            }) as Box<dyn BrowserProcess>)
        })
    }
}

struct Held {
    owner: BrowserOwner,
    pid: u32,
    connection: BrowserConnection,
    initialization_error: Option<String>,
}

impl BrowserProcess for Held {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn connection(&self) -> &BrowserConnection {
        &self.connection
    }

    fn initialization_error(&self) -> Option<&str> {
        self.initialization_error.as_deref()
    }

    fn stop(&mut self) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.owner.stop())
    }
}
