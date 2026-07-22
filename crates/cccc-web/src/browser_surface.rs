mod interaction;

pub use interaction::serve_socket;

use anyhow::{Context, Result, bail};
use cccc_contracts::utc_now;
use chromiumoxide::Page;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Default)]
pub struct BrowserSurfaces {
    pub(super) sessions: Mutex<HashMap<String, Session>>,
}

pub(super) struct Session {
    pub(super) browser: Browser,
    pub(super) page: Page,
    handler: JoinHandle<()>,
    url: String,
    pub(super) width: u32,
    pub(super) height: u32,
    started_at: String,
    pub(super) updated_at: String,
    seq: u64,
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
            .sessions
            .lock()
            .await
            .keys()
            .filter(|key| prefixes.iter().any(|prefix| key.starts_with(prefix)))
            .cloned()
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
        self.open_seeded(key, profile, url, width, height, None)
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
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("browser surface URL must use http or https");
        }
        self.close(key).await?;
        std::fs::create_dir_all(profile)?;
        let config = BrowserConfig::builder()
            .new_headless_mode()
            .window_size(width, height)
            .user_data_dir(profile)
            .build()
            .map_err(anyhow::Error::msg)?;
        let (browser, mut handler) = Browser::launch(config).await?;
        let task = tokio::spawn(async move {
            while let Some(message) = handler.next().await {
                if message.is_err() {
                    break;
                }
            }
        });
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
        let page = browser.new_page(url).await.context("open browser page")?;
        let now = utc_now();
        let session = Session {
            browser,
            page,
            handler: task,
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
        self.sessions
            .lock()
            .await
            .get(key)
            .map(state)
            .unwrap_or_else(idle)
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
        let session = self.sessions.lock().await.remove(key);
        let Some(mut session) = session else {
            return Ok(false);
        };
        let result = session.browser.close().await;
        session.handler.abort();
        result?;
        Ok(true)
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

#[cfg(test)]
mod browser_surface_tests;
