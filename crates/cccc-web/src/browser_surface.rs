mod interaction;
mod profile_owner;

pub use interaction::serve_socket;

use anyhow::{Context, Result, bail};
use cccc_contracts::utc_now;
use chromiumoxide::Page;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures_util::StreamExt;
use futures_util::future::join_all;
use profile_owner::ProfileLease;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

const BROWSER_EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Default)]
pub struct BrowserSurfaces {
    pub(super) sessions: Mutex<HashMap<String, Session>>,
    profile_operations: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    key_profiles: Mutex<HashMap<String, PathBuf>>,
    shutting_down: AtomicBool,
}

pub(super) struct Session {
    pub(super) browser: Browser,
    pub(super) page: Page,
    handler: JoinHandle<()>,
    profile_lease: ProfileLease,
    url: String,
    pub(super) width: u32,
    pub(super) height: u32,
    started_at: String,
    pub(super) updated_at: String,
    seq: u64,
}

struct OpenRequest<'a> {
    key: &'a str,
    profile: &'a Path,
    url: &'a str,
    width: u32,
    height: u32,
    storage_state: Option<&'a Value>,
    reuse_existing: bool,
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
        })
        .await
    }

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
        })
        .await
    }

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
        } = request;
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("browser surface URL must use http or https");
        }
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("browser surfaces are shutting down");
        }
        std::fs::create_dir_all(profile)?;
        let profile = std::fs::canonicalize(profile)?;
        let operation = self.register_profile(key, &profile).await?;
        let _operation_guard = operation.lock().await;
        self.bind_profile(key, &profile).await?;
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
        let mut profile_lease = ProfileLease::acquire(&profile).await?;
        let config = BrowserConfig::builder()
            .new_headless_mode()
            .window_size(width, height)
            .user_data_dir(&profile)
            .build()
            .map_err(anyhow::Error::msg)?;
        let (mut browser, mut handler) = Browser::launch(config).await?;
        if let Err(error) = profile_lease.record_browser(&mut browser).await {
            let _ = browser.kill().await;
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
            let page = browser
                .new_page("about:blank")
                .await
                .context("create browser page")?;
            page.goto(url).await.context("open browser page")?;
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
                return Err(error);
            }
        };
        let now = utc_now();
        let session = Session {
            browser,
            page,
            handler: task,
            profile_lease,
            url: url.into(),
            width,
            height,
            started_at: now.clone(),
            updated_at: now,
            seq: 0,
        };
        let state = state(&session);
        self.sessions.lock().await.insert(key.into(), session);
        Ok(state)
    }

    pub async fn info(&self, key: &str) -> Value {
        let handler_finished = self
            .sessions
            .lock()
            .await
            .get(key)
            .is_some_and(|session| session.handler.is_finished());
        if handler_finished {
            let message = match self.close(key).await {
                Ok(_) => "Browser surface process exited.".to_owned(),
                Err(error) => format!("Browser surface process exited; cleanup failed: {error}"),
            };
            return failed(&message);
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
        page.evaluate(
            "(() => { const wiz = globalThis.WIZ_global_data; if (wiz && typeof wiz.SNlM0e === 'string' && wiz.SNlM0e && typeof wiz.FdrFJe === 'string' && wiz.FdrFJe) return true; const html = document.documentElement?.innerHTML || ''; return /[\\\"']SNlM0e[\\\"']\\s*:\\s*[\\\"'][^\\\"']+[\\\"']/.test(html) && /[\\\"']FdrFJe[\\\"']\\s*:\\s*[\\\"'][^\\\"']+[\\\"']/.test(html); })()",
        )
        .await?
        .into_value()
        .map_err(anyhow::Error::from)
    }

    pub async fn close(&self, key: &str) -> Result<bool> {
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
                bail!(
                    "browser surface key {key} is already assigned to profile {}",
                    existing.display()
                );
            }
        } else {
            key_profiles.insert(key.to_owned(), profile.to_owned());
        }
        Ok(())
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

    pub async fn frame(&self, key: &str) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(key)
            .context("browser surface is not active")?;
        let bytes = session
            .page
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Jpeg)
                    .quality(75)
                    .build(),
            )
            .await?;
        session.seq += 1;
        session.updated_at = utc_now();
        session.url = session
            .page
            .url()
            .await?
            .unwrap_or_else(|| session.url.clone());
        Ok(json!({
            "t":"frame",
            "seq":session.seq,
            "mime":"image/jpeg",
            "data_base64":base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
            "width":session.width,
            "height":session.height,
            "captured_at":session.updated_at,
            "url":session.url
        }))
    }
}

async fn stop_browser(browser: &mut Browser, handler: &mut JoinHandle<()>) -> Result<()> {
    if let Err(error) = browser.close().await {
        tracing::debug!(%error, "Chromium close command failed; waiting for process exit");
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
    json!({
        "active":true,"state":"ready","message":"Browser surface is ready.","strategy":"cdp_screencast",
        "url":session.url,"width":session.width,"height":session.height,
        "started_at":session.started_at,"updated_at":session.updated_at,
        "last_frame_seq":session.seq,"last_frame_at":session.updated_at,"controller_attached":false,
        "viewer":{"kind":"screencast","vnc":{"available":false,"error":"VNC is not configured"}}
    })
}

fn idle() -> Value {
    json!({
        "active":false,"state":"idle","message":"No browser surface session is active for this slot.",
        "width":0,"height":0,"last_frame_seq":0,"controller_attached":false,
        "viewer":{"kind":"screencast","vnc":{"available":false,"error":"VNC is not configured"}}
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
