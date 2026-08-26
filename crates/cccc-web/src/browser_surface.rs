mod frame;
mod interaction;
mod navigation;
mod page_recovery;
mod profile_owner;
mod prompt_submission;
mod proxy;
mod system_browser;

pub use interaction::{serve_socket, serve_vnc_socket};
pub(crate) use prompt_submission::{
    BOUND_CONVERSATION_ERROR_MARKER, PromptSubmissionOutcome, conversation_target_matches,
    conversation_url_for_target, is_chatgpt_url, normalized_chatgpt_conversation_url,
    stored_verified_submission_evidence,
};

use anyhow::{Context, Result, bail};
use cccc_contracts::utc_now;
use chromiumoxide::Page;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::handler::viewport::Viewport;
use futures_util::StreamExt;
use futures_util::future::join_all;
use navigation::goto_dom_content_loaded;
use page_recovery::{close_internal_pages, is_internal_page};
use profile_owner::ProfileLease;
use proxy::BrowserProxy;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use system_browser::SystemBrowserLaunch;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

const BROWSER_EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

pub(crate) fn system_browser_path() -> Option<PathBuf> {
    system_browser::find_system_browser().map(|(path, _)| path)
}

#[derive(Default)]
pub struct BrowserSurfaces {
    pub(super) sessions: Mutex<HashMap<String, Session>>,
    key_operations: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    profile_operations: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    key_profiles: Mutex<HashMap<String, PathBuf>>,
    shutting_down: AtomicBool,
}

pub(super) struct Session {
    pub(super) browser: Browser,
    pub(super) page: Page,
    handler: JoinHandle<()>,
    profile_lease: ProfileLease,
    system_browser: Option<SystemBrowserLaunch>,
    url: String,
    pub(super) width: u32,
    pub(super) height: u32,
    started_at: String,
    pub(super) updated_at: String,
    seq: u64,
    strategy: String,
    metadata: Value,
    recover_closed_page: bool,
}

#[derive(Clone, Copy)]
enum BrowserMode {
    Headless,
    System { background: bool },
}

struct OpenRequest<'a> {
    key: &'a str,
    profile: &'a Path,
    url: &'a str,
    width: u32,
    height: u32,
    storage_state: Option<&'a Value>,
    reuse_existing: bool,
    mode: BrowserMode,
}

pub(super) fn validate_browser_surface_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).context("invalid browser surface URL")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("browser surface URL must use http or https");
    }
    Ok(())
}

impl BrowserSurfaces {
    pub async fn close_missing_groups(&self, active_groups: &HashSet<String>) -> Result<usize> {
        let keys = self
            .sessions
            .lock()
            .await
            .keys()
            .filter_map(|key| {
                let group_id = session_group_id(key)?;
                (!active_groups.contains(group_id)).then(|| key.clone())
            })
            .collect::<Vec<_>>();
        let mut closed = 0;
        for key in keys {
            closed += usize::from(self.close(&key).await?);
        }
        Ok(closed)
    }

    pub async fn close_missing_actors(
        &self,
        active_actors: &HashMap<String, HashSet<String>>,
    ) -> Result<usize> {
        let keys = self
            .sessions
            .lock()
            .await
            .keys()
            .filter(|key| {
                session_actor(key).is_some_and(|(group_id, actor_id)| {
                    !active_actors
                        .get(group_id)
                        .is_some_and(|actors| actors.contains(actor_id))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut closed = 0;
        for key in keys {
            closed += usize::from(self.close(&key).await?);
        }
        Ok(closed)
    }

    pub async fn close_prefixes(&self, prefixes: &[String]) -> Result<usize> {
        let keys = self
            .known_keys()
            .await
            .into_iter()
            .filter(|key| prefixes.iter().any(|prefix| key.starts_with(prefix)))
            .collect::<Vec<_>>();
        let mut closed = 0;
        for key in keys {
            closed += usize::from(self.close(&key).await?);
        }
        Ok(closed)
    }

    pub async fn open(
        &self,
        key: &str,
        profile: &Path,
        url: &str,
        width: u32,
        height: u32,
    ) -> Result<Value> {
        self.open_with(OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state: None,
            reuse_existing: false,
            mode: BrowserMode::Headless,
        })
        .await
    }

    #[cfg(test)]
    pub async fn ensure_open(
        &self,
        key: &str,
        profile: &Path,
        url: &str,
        width: u32,
        height: u32,
    ) -> Result<Value> {
        self.open_with(OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state: None,
            reuse_existing: true,
            mode: BrowserMode::Headless,
        })
        .await
    }

    pub async fn ensure_open_system(
        &self,
        key: &str,
        profile: &Path,
        url: &str,
        width: u32,
        height: u32,
    ) -> Result<Value> {
        self.open_with(OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state: None,
            reuse_existing: true,
            mode: BrowserMode::System { background: false },
        })
        .await
    }

    #[cfg(test)]
    pub async fn open_seeded(
        &self,
        key: &str,
        profile: &Path,
        url: &str,
        width: u32,
        height: u32,
        storage_state: Option<&Value>,
    ) -> Result<Value> {
        self.open_with(OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state,
            reuse_existing: false,
            mode: BrowserMode::Headless,
        })
        .await
    }

    pub async fn open_seeded_system(
        &self,
        key: &str,
        profile: &Path,
        url: &str,
        width: u32,
        height: u32,
        storage_state: Option<&Value>,
    ) -> Result<Value> {
        self.open_with(OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state,
            reuse_existing: false,
            mode: BrowserMode::System { background: true },
        })
        .await
    }

    async fn open_with(&self, request: OpenRequest<'_>) -> Result<Value> {
        let OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state,
            reuse_existing,
            mode,
        } = request;
        validate_browser_surface_url(url)?;
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("browser surfaces are shutting down");
        }
        std::fs::create_dir_all(profile)?;
        let profile = std::fs::canonicalize(profile)?;
        let key_operation = self.key_operation(key).await;
        let _key_operation_guard = key_operation.lock().await;
        let operation = self.register_profile(key, &profile).await?;
        let _operation_guard = operation.lock().await;
        self.bind_profile(key, &profile).await?;
        let result = self
            .open_registered(OpenRequest {
                key,
                profile: &profile,
                url,
                width,
                height,
                storage_state,
                reuse_existing,
                mode,
            })
            .await;
        if result.is_err() {
            self.release_inactive_profile(key, &profile).await;
        }
        result
    }

    async fn open_registered(&self, request: OpenRequest<'_>) -> Result<Value> {
        let OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state,
            reuse_existing,
            mode,
        } = request;
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("browser surfaces are shutting down");
        }
        if reuse_existing {
            let handler_finished = self
                .sessions
                .lock()
                .await
                .get(key)
                .is_some_and(|session| session.handler.is_finished());
            if handler_finished {
                self.close_locked(key).await?;
            } else if let Some(existing) = self.sessions.lock().await.get(key).map(state) {
                return Ok(existing);
            }
        }
        self.close_locked(key).await?;
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
        let mut task = tokio::spawn(async move {
            while let Some(message) = handler.next().await {
                if message.is_err() {
                    break;
                }
            }
        });
        let initialized = async {
            if let Some(cookies) = storage_state
                .and_then(|state| state.get("cookies"))
                .cloned()
            {
                let cookies: Vec<CookieParam> =
                    serde_json::from_value(cookies).context("decode saved browser cookies")?;
                if !cookies.is_empty() {
                    browser.set_cookies(cookies).await?;
                }
            }
            let page = match reusable_page(&browser).await? {
                Some(page) => page,
                None => browser
                    .new_page("about:blank")
                    .await
                    .context("create browser page")?,
            };
            goto_dom_content_loaded(&page, url)
                .await
                .context("open browser page")?;
            close_internal_pages(&browser, &page).await?;
            Ok::<Page, anyhow::Error>(page)
        }
        .await;
        let page = match initialized {
            Ok(page) => page,
            Err(error) => {
                if let Err(cleanup_error) = stop_browser(&mut browser, &mut task).await {
                    tracing::warn!(%cleanup_error, "failed to clean up browser after initialization error");
                }
                let _ = profile_lease.clear_owner();
                if let Some(system_browser) = &mut system_browser {
                    system_browser.stop().await;
                }
                return Err(error);
            }
        };
        let now = utc_now();
        let (strategy, metadata) = system_browser.as_ref().map_or_else(
            || {
                (
                    "cdp_screencast".to_owned(),
                    json!({"visibility":"headless","display_owned":false}),
                )
            },
            |system_browser| {
                (
                    system_browser.strategy(),
                    system_browser.metadata(browser_pid, profile),
                )
            },
        );
        let session = Session {
            browser,
            page,
            handler: task,
            profile_lease,
            system_browser,
            url: url.into(),
            width,
            height,
            started_at: now.clone(),
            updated_at: now,
            seq: 0,
            strategy,
            metadata,
            recover_closed_page: matches!(mode, BrowserMode::Headless),
        };
        let state = state(&session);
        self.sessions.lock().await.insert(key.into(), session);
        Ok(state)
    }

    pub async fn info(&self, key: &str) -> Value {
        let (handler_finished, user_closed_page) = {
            let sessions = self.sessions.lock().await;
            let Some(session) = sessions.get(key) else {
                return idle();
            };
            let handler_finished = session.handler.is_finished();
            let user_closed_page =
                if handler_finished || session.recover_closed_page {
                    false
                } else {
                    let target_id = session.page.target_id();
                    session.browser.pages().await.is_ok_and(|pages| {
                        !pages.into_iter().any(|page| page.target_id() == target_id)
                    })
                };
            (handler_finished, user_closed_page)
        };
        if handler_finished {
            let message = match self.close(key).await {
                Ok(_) => "Browser surface process exited.".to_owned(),
                Err(error) => format!("Browser surface process exited; cleanup failed: {error}"),
            };
            return failed(&message);
        }
        if user_closed_page {
            let _ = self.close(key).await;
            return closed();
        }
        self.sessions.lock().await.get(key).map_or_else(idle, state)
    }

    pub async fn storage_state(&self, key: &str) -> Result<Value> {
        let page = self
            .sessions
            .lock()
            .await
            .get(key)
            .context("browser surface is not active")?
            .page
            .clone();
        let url = page.url().await?.unwrap_or_default();
        let authuser = authuser_from_url(&url);
        let cookies = page
            .get_cookies()
            .await?
            .into_iter()
            .filter(|cookie| {
                let domain = cookie.domain.trim_start_matches('.');
                domain == "google.com" || domain.ends_with(".google.com")
            })
            .collect::<Vec<_>>();
        Ok(json!({"cookies": cookies, "origins": [], "authuser": authuser}))
    }

    pub async fn notebooklm_auth_ready(&self, key: &str) -> Result<bool> {
        let page = self
            .sessions
            .lock()
            .await
            .get(key)
            .context("browser surface is not active")?
            .page
            .clone();
        Ok(page.get_cookies().await?.into_iter().any(|cookie| {
            matches!(
                cookie.name.as_str(),
                "SID" | "SAPISID" | "__Secure-1PSID" | "__Secure-3PSID"
            ) && !cookie.value.is_empty()
        }))
    }

    pub async fn page_available(&self, key: &str) -> bool {
        let sessions = self.sessions.lock().await;
        let Some(session) = sessions.get(key) else {
            return false;
        };
        let target_id = session.page.target_id();
        session
            .browser
            .pages()
            .await
            .is_ok_and(|pages| pages.into_iter().any(|page| page.target_id() == target_id))
    }

    pub async fn vnc_port(&self, key: &str) -> Result<u16> {
        self.sessions
            .lock()
            .await
            .get(key)
            .and_then(|session| session.system_browser.as_ref())
            .and_then(SystemBrowserLaunch::vnc_port)
            .context("VNC viewer is not available for this browser surface")
    }

    pub async fn close(&self, key: &str) -> Result<bool> {
        let key_operation = self.key_operation(key).await;
        let _key_operation_guard = key_operation.lock().await;
        let Some((profile, operation)) = self.operation_for_key(key).await else {
            return Ok(false);
        };
        let _operation_guard = operation.lock().await;
        if self.key_profiles.lock().await.get(key) != Some(&profile) {
            return Ok(false);
        }
        let was_closed = self.close_locked(key).await?;
        let mut key_profiles = self.key_profiles.lock().await;
        if key_profiles.get(key) == Some(&profile) {
            key_profiles.remove(key);
        }
        Ok(was_closed)
    }

    pub async fn shutdown_all(&self) -> Result<usize> {
        self.shutting_down.store(true, Ordering::Release);
        let keys = self.known_keys().await;
        let mut closed = 0;
        let mut first_error = None;
        let results = join_all(keys.into_iter().map(|key| async move {
            let result = self.close(&key).await;
            (key, result)
        }))
        .await;
        for (key, result) in results {
            match result {
                Ok(was_closed) => closed += usize::from(was_closed),
                Err(error) => {
                    tracing::warn!(%error, %key, "failed to close browser surface during shutdown");
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(closed)
    }

    async fn close_locked(&self, key: &str) -> Result<bool> {
        let session = self.sessions.lock().await.remove(key);
        let Some(mut session) = session else {
            return Ok(false);
        };
        if let Err(error) = stop_browser(&mut session.browser, &mut session.handler).await {
            self.sessions.lock().await.insert(key.to_owned(), session);
            return Err(error);
        }
        if let Some(system_browser) = &mut session.system_browser {
            system_browser.stop().await;
        }
        session.profile_lease.clear_owner()?;
        Ok(true)
    }

    async fn register_profile(&self, key: &str, profile: &Path) -> Result<Arc<Mutex<()>>> {
        self.bind_profile(key, profile).await?;
        let mut operations = self.profile_operations.lock().await;
        Ok(Arc::clone(
            operations
                .entry(profile.to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        ))
    }

    async fn bind_profile(&self, key: &str, profile: &Path) -> Result<()> {
        let mut key_profiles = self.key_profiles.lock().await;
        if let Some(existing) = key_profiles.get(key) {
            if existing != profile {
                if self.sessions.lock().await.contains_key(key) {
                    bail!(
                        "browser surface key {key} is already assigned to profile {}",
                        existing.display()
                    );
                }
                key_profiles.insert(key.to_owned(), profile.to_owned());
            }
        } else {
            key_profiles.insert(key.to_owned(), profile.to_owned());
        }
        Ok(())
    }

    async fn key_operation(&self, key: &str) -> Arc<Mutex<()>> {
        let mut operations = self.key_operations.lock().await;
        Arc::clone(
            operations
                .entry(key.to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    async fn release_inactive_profile(&self, key: &str, profile: &Path) {
        if self.sessions.lock().await.contains_key(key) {
            return;
        }
        let mut key_profiles = self.key_profiles.lock().await;
        if key_profiles.get(key).is_some_and(|value| value == profile) {
            key_profiles.remove(key);
        }
    }

    async fn operation_for_key(&self, key: &str) -> Option<(PathBuf, Arc<Mutex<()>>)> {
        let profile = self.key_profiles.lock().await.get(key).cloned()?;
        let operation = self
            .profile_operations
            .lock()
            .await
            .get(&profile)
            .cloned()?;
        Some((profile, operation))
    }

    async fn known_keys(&self) -> HashSet<String> {
        let mut keys = self
            .key_profiles
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        keys.extend(self.sessions.lock().await.keys().cloned());
        keys
    }
}

async fn reusable_page(browser: &Browser) -> Result<Option<Page>> {
    for page in browser.pages().await? {
        let url = page.url().await?.unwrap_or_default();
        if is_internal_page(&url) {
            return Ok(Some(page));
        }
    }
    Ok(None)
}

async fn stop_browser(browser: &mut Browser, handler: &mut JoinHandle<()>) -> Result<()> {
    match tokio::time::timeout(BROWSER_EXIT_TIMEOUT, browser.close()).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::debug!(%error, "Chromium close command failed; waiting for process exit");
        }
        Err(_) => {
            tracing::warn!("Chromium close command timed out; forcing process termination");
        }
    }
    let exited = matches!(
        tokio::time::timeout(BROWSER_EXIT_TIMEOUT, browser.wait()).await,
        Ok(Ok(_))
    );
    if !exited {
        match tokio::time::timeout(BROWSER_EXIT_TIMEOUT, browser.kill()).await {
            Ok(Some(Ok(()))) | Ok(None) => {}
            Ok(Some(Err(error))) => {
                return Err(error).context("kill Chromium after close timeout");
            }
            Err(_) => bail!("Chromium did not exit after forced termination"),
        }
    }
    handler.abort();
    let _ = handler.await;
    Ok(())
}

fn session_group_id(key: &str) -> Option<&str> {
    key.strip_prefix("web-model::")
        .and_then(|value| value.split("::").next())
        .or_else(|| {
            key.split_once("::")
                .map(|(prefix, _)| prefix)
                .filter(|prefix| prefix.starts_with("g_"))
        })
}

fn session_actor(key: &str) -> Option<(&str, &str)> {
    let value = key.strip_prefix("web-model::")?;
    let (group_id, actor_id) = value.split_once("::")?;
    (!group_id.is_empty() && !actor_id.is_empty()).then_some((group_id, actor_id))
}

fn authuser_from_url(raw: &str) -> usize {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return 0;
    };
    if let Some(value) = url
        .query_pairs()
        .find_map(|(key, value)| (key == "authuser").then_some(value))
        .and_then(|value| value.parse().ok())
    {
        return value;
    }
    let segments = url
        .path_segments()
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    segments
        .windows(2)
        .find_map(|pair| (pair[0] == "u").then(|| pair[1].parse().ok()).flatten())
        .unwrap_or(0)
}

fn state(session: &Session) -> Value {
    let viewer = session.system_browser.as_ref().map_or_else(
        || json!({"kind":"screencast","vnc":{"available":false,"error":"unsupported_surface"}}),
        SystemBrowserLaunch::viewer,
    );
    json!({
        "active":true,"state":"ready","message":"Browser surface is ready.","strategy":session.strategy,
        "url":session.url,"width":session.width,"height":session.height,
        "started_at":session.started_at,"updated_at":session.updated_at,
        "last_frame_seq":session.seq,"last_frame_at":session.updated_at,"controller_attached":false,
        "metadata":session.metadata,
        "viewer":viewer
    })
}

fn idle() -> Value {
    json!({
        "active":false,"state":"idle","message":"No browser surface session is active for this slot.",
        "width":0,"height":0,"last_frame_seq":0,"controller_attached":false,
        "viewer":{"kind":"screencast","vnc":{"available":false,"error":"browser_surface_not_active"}}
    })
}

fn closed() -> Value {
    json!({
        "active":false,"state":"closed","message":"Browser surface was closed by the user.",
        "width":0,"height":0,"last_frame_seq":0,"controller_attached":false,
        "viewer":{"kind":"screencast","vnc":{"available":false,"error":"Browser surface was closed"}}
    })
}

fn failed(message: &str) -> Value {
    json!({
        "active":false,"state":"failed","message":message,
        "error":{"code":"browser_surface_process_exited","message":message},
        "width":0,"height":0,"last_frame_seq":0,"controller_attached":false,
        "viewer":{"kind":"screencast","vnc":{"available":false,"error":"Browser process exited"}}
    })
}

#[cfg(test)]
mod browser_surface_tests;
