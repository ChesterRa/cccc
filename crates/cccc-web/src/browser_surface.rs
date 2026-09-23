mod frame;
mod interaction;
mod navigation;
mod owner;
mod page_recovery;
mod profile_owner;
mod prompt_submission;
mod proxy;
mod system_browser;

#[cfg(test)]
mod chrome_test_guard;
#[cfg(test)]
pub(crate) use chrome_test_guard::chrome_test_guard;

pub use interaction::{serve_socket, serve_vnc_socket};
pub(crate) use prompt_submission::{
    BOUND_CONVERSATION_ERROR_MARKER, PromptSubmissionOutcome, conversation_target_matches,
    conversation_url_for_target, is_chatgpt_url, normalized_chatgpt_conversation_url,
};

use anyhow::{Context, Result, bail};
use cccc_contracts::utc_now;
use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::target::{CreateTargetParams, GetTargetsParams};
use futures_util::future::join_all;
use navigation::goto_dom_content_loaded;
use owner::BrowserOwner;
mod pairing;
use page_recovery::{candidate_page_url, close_internal_pages, is_internal_page};
pub(crate) use pairing::PairingPage;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

const BROWSER_EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

pub(crate) fn system_browser_path() -> Option<PathBuf> {
    system_browser::find_system_browser().map(|(path, _)| path)
}

#[derive(Default)]
pub struct BrowserSurfaces {
    pub(super) sessions: Mutex<HashMap<String, Session>>,
    owners: Mutex<HashMap<PathBuf, Arc<RwLock<BrowserOwner>>>>,
    key_operations: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    profile_operations: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    key_profiles: Mutex<HashMap<String, PathBuf>>,
    shutting_down: AtomicBool,
    closed_pages: Mutex<HashMap<String, Option<String>>>,
}

#[derive(Clone)]
pub(super) struct Session {
    owner: Arc<RwLock<BrowserOwner>>,
    pub(super) page: Page,
    is_system_browser: bool,
    shared_browser: bool,
    viewer: Value,
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
    shared_browser: bool,
    actor_generation: Option<&'a str>,
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
            .known_keys()
            .await
            .into_iter()
            .filter(|key| session_group_id(key).is_some_and(|id| !active_groups.contains(id)))
            .collect::<Vec<_>>();
        let mut closed = 0;
        for key in keys {
            closed += usize::from(self.close(&key).await?);
        }
        Ok(closed)
    }

    pub async fn close_missing_actors(&self, store: &cccc_core::GroupStore) -> Result<usize> {
        // Snapshot registered Actor surfaces first. Unused Groups need no
        // document reads, and a newly opened surface waits for the next pass.
        let mut keys = self
            .sessions
            .lock()
            .await
            .iter()
            .filter(|(key, _)| session_actor(key).is_some())
            .map(|(key, s)| {
                (
                    key.clone(),
                    Some(s.page.target_id().clone()),
                    s.metadata["actor_generation"].as_str().map(str::to_owned),
                )
            })
            .collect::<Vec<_>>();
        // Closing a window removes its session, but its pause intent still
        // belongs to that Actor generation and must participate in cleanup.
        keys.extend(
            self.closed_pages
                .lock()
                .await
                .iter()
                .map(|(key, generation)| (key.clone(), None, generation.clone())),
        );
        let mut groups = HashMap::new();
        let keys = keys
            .into_iter()
            .filter_map(|(key, target, generation)| {
                let (group_id, actor_id) = session_actor(&key)?;
                let group = groups
                    .entry(group_id.to_owned())
                    .or_insert_with(|| store.load(group_id).ok());
                let valid = group.as_ref().is_some_and(|g| {
                    g.actors.iter().any(|a| {
                        a.id == actor_id
                            && generation.as_ref().is_none_or(|old| {
                                a.runtime == cccc_contracts::ActorRuntime::WebModel
                                    && cccc_core::actors::generation_identity(a) == *old
                            })
                    })
                });
                (!valid).then_some((key, target, generation))
            })
            .collect::<Vec<_>>();
        let mut closed = 0;
        for (key, target, generation) in keys {
            let operation = self.key_operation(&key).await;
            let _guard = operation.lock().await;
            // A replacement opened after the snapshot owns its own lifecycle.
            let same_owner = if let Some(target) = target {
                self.sessions
                    .lock()
                    .await
                    .get(&key)
                    .is_some_and(|s| s.page.target_id() == &target)
            } else {
                !self.sessions.lock().await.contains_key(&key)
                    && self.closed_pages.lock().await.get(&key) == Some(&generation)
            };
            if same_owner {
                closed += usize::from(self.close_key_locked(&key).await?);
            }
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
            shared_browser: false,
            actor_generation: None,
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
            shared_browser: false,
            actor_generation: None,
        })
        .await
    }

    pub async fn ensure_open_shared_system(
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
            shared_browser: true,
            actor_generation: None,
        })
        .await?;
        // Explicitly opening login should reveal its window, while Actor
        // warmup and polling must never take desktop focus. Preserve an active
        // sign-in flow; only an internal/blank page needs its entry URL restored.
        let operation = self.key_operation(key).await;
        let _guard = operation.lock().await;
        let page = self
            .sessions
            .lock()
            .await
            .get(key)
            .map(|s| s.page.clone())
            .context("shared login window is not active")?;
        if is_internal_page(&page.url().await?.unwrap_or_default()) {
            goto_dom_content_loaded(&page, url).await?;
        }
        page.bring_to_front()
            .await
            .context("show shared login window")?;
        let current_url = page.url().await?.unwrap_or_else(|| url.to_owned());
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(key)
            .context("shared login window is not active")?;
        session.url = current_url;
        session.updated_at = utc_now();
        Ok(state(session))
    }

    pub async fn ensure_open_shared_actor(
        &self,
        key: &str,
        profile: &Path,
        url: &str,
        dimensions: (u32, u32),
        generation: &str,
    ) -> Result<Value> {
        self.open_with(OpenRequest {
            key,
            profile,
            url,
            width: dimensions.0,
            height: dimensions.1,
            storage_state: None,
            reuse_existing: true,
            mode: BrowserMode::System { background: false },
            shared_browser: true,
            actor_generation: Some(generation),
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
            shared_browser: false,
            actor_generation: None,
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
            shared_browser: false,
            actor_generation: None,
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
            shared_browser,
            actor_generation,
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
        self.closed_pages.lock().await.remove(key);
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
                shared_browser,
                actor_generation,
            })
            .await;
        // Process/profile registration is serialized; a slow page navigation
        // must not hold the shared profile lock and block another Actor window.
        drop(_operation_guard);
        let (value, page) = match result {
            Ok(value) => value,
            Err(error) => {
                self.release_inactive_profile(key, &profile).await;
                return Err(error);
            }
        };
        if let Some(page) = page {
            let opened = async {
                goto_dom_content_loaded(&page, url)
                    .await
                    .context("open browser page")?;
                if !shared_browser {
                    let owner = self
                        .sessions
                        .lock()
                        .await
                        .get(key)
                        .map(|s| Arc::clone(&s.owner))
                        .context("browser surface changed")?;
                    close_internal_pages(&owner.read().await.browser, &page).await?;
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;
            if let Err(error) = opened {
                let _guard = operation.lock().await;
                self.close_locked(key)
                    .await
                    .context("clean up failed browser page")?;
                self.release_inactive_profile(key, &profile).await;
                return Err(error);
            }
        }
        Ok(value)
    }

    async fn open_registered(&self, request: OpenRequest<'_>) -> Result<(Value, Option<Page>)> {
        let OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state,
            reuse_existing,
            mode,
            shared_browser,
            actor_generation,
        } = request;
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("browser surfaces are shutting down");
        }
        if reuse_existing {
            let existing = self
                .sessions
                .lock()
                .await
                .get(key)
                .map(|s| (Arc::clone(&s.owner), state(s), s.page.target_id().clone()));
            if let Some((owner, state, target_id)) = existing {
                let owner = owner.read().await;
                if !owner.handler.is_finished()
                    && actor_generation.is_none_or(|g| state["metadata"]["actor_generation"] == g)
                {
                    let targets = owner
                        .browser
                        .execute(GetTargetsParams::default())
                        .await
                        .context("inspect owned browser target")?;
                    if targets
                        .result
                        .target_infos
                        .iter()
                        .any(|t| t.target_id == target_id)
                    {
                        return Ok((state, None));
                    }
                }
            }
        }
        self.close_locked(key).await?;
        let existing = self.owners.lock().await.get(profile).cloned();
        let owner = if let Some(owner) = existing {
            if !shared_browser || !owner.read().await.shared {
                bail!(
                    "browser profile is already managed by another surface: {}",
                    profile.display()
                );
            }
            if owner.read().await.handler.is_finished() {
                self.close_owner_locked(profile).await?;
                None
            } else {
                Some(owner)
            }
        } else {
            None
        };
        let new_owner = owner.is_none();
        let owner = match owner {
            Some(owner) => owner,
            None => {
                let owner = Arc::new(RwLock::new(
                    BrowserOwner::launch(
                        profile,
                        width,
                        height,
                        mode,
                        shared_browser,
                        storage_state,
                    )
                    .await?,
                ));
                self.owners
                    .lock()
                    .await
                    .insert(profile.to_owned(), Arc::clone(&owner));
                owner
            }
        };
        let initialized = async {
            let owner = owner.read().await;
            if shared_browser {
                let params = CreateTargetParams::builder()
                    .url("about:blank")
                    .new_window(true)
                    .background(actor_generation.is_some())
                    .build()
                    .map_err(anyhow::Error::msg)?;
                let page = owner
                    .browser
                    .new_page(params)
                    .await
                    .context("create shared browser window")?;
                // Only the freshly launched owner has unowned startup pages.
                // Later blank windows may belong to another Actor navigating.
                if new_owner {
                    close_internal_pages(&owner.browser, &page).await?;
                }
                Ok(page)
            } else {
                match reusable_page(&owner.browser).await? {
                    Some(page) => Ok(page),
                    None => owner
                        .browser
                        .new_page("about:blank")
                        .await
                        .context("create browser page"),
                }
            }
        }
        .await;
        let page = match initialized {
            Ok(page) => page,
            Err(error) => {
                if !shared_browser {
                    self.close_owner_locked(profile).await?;
                }
                return Err(error);
            }
        };
        let now = utc_now();
        let resource = owner.read().await;
        let mut metadata = resource.metadata.clone();
        if shared_browser {
            metadata["shared_browser"] = json!(true);
        }
        if let Some(generation) = actor_generation {
            metadata["actor_generation"] = json!(generation);
        }
        let session = Session {
            owner: Arc::clone(&owner),
            page: page.clone(),
            is_system_browser: resource.system_browser.is_some(),
            shared_browser,
            viewer: if actor_generation.is_some() {
                json!({"kind":"screencast","vnc":{"available":false,"error":"actor_page_only"}})
            } else {
                resource.viewer.clone()
            },
            url: url.into(),
            width,
            height,
            started_at: now.clone(),
            updated_at: now,
            seq: 0,
            strategy: resource.strategy.clone(),
            metadata,
            recover_closed_page: !shared_browser && matches!(mode, BrowserMode::Headless),
        };
        drop(resource);
        // Register before navigation: failed or cancelled initialization still has
        // an owner, and cleanup must not affect other Actor windows.
        self.bind_profile(key, profile).await?;
        self.sessions.lock().await.insert(key.into(), session);
        Ok((
            self.sessions.lock().await.get(key).map_or_else(idle, state),
            Some(page),
        ))
    }

    pub async fn info(&self, key: &str) -> Value {
        let snapshot = self.sessions.lock().await.get(key).map(|s| {
            (
                Arc::clone(&s.owner),
                s.page.target_id().clone(),
                s.recover_closed_page,
                s.metadata["actor_generation"].as_str().map(str::to_owned),
            )
        });
        let Some((owner, target_id, recover_closed_page, generation)) = snapshot else {
            return if self.closed_pages.lock().await.contains_key(key) {
                closed()
            } else {
                idle()
            };
        };
        let owner = owner.read().await;
        let handler_finished = owner.handler.is_finished();
        let user_closed_page = !handler_finished
            && !recover_closed_page
            && owner
                .browser
                .execute(GetTargetsParams::default())
                .await
                .is_ok_and(|targets| {
                    !targets
                        .result
                        .target_infos
                        .iter()
                        .any(|target| target.target_id == target_id)
                });
        drop(owner);
        if handler_finished {
            let message = match self.close(key).await {
                Ok(_) => "Browser surface process exited.".to_owned(),
                Err(error) => format!("Browser surface process exited; cleanup failed: {error}"),
            };
            return failed(&message);
        }
        if user_closed_page {
            let operation = self.key_operation(key).await;
            let _guard = operation.lock().await;
            if self
                .sessions
                .lock()
                .await
                .get(key)
                .is_some_and(|s| s.page.target_id() == &target_id)
            {
                if let Err(error) = self.close_key_locked(key).await {
                    return failed(&format!("Closed browser surface cleanup failed: {error}"));
                }
                if session_actor(key).is_some() {
                    self.closed_pages
                        .lock()
                        .await
                        .insert(key.to_owned(), generation);
                }
                return closed();
            }
            // A replacement that opened during inspection has its own lifecycle.
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
        let snapshot = self
            .sessions
            .lock()
            .await
            .get(key)
            .map(|s| (Arc::clone(&s.owner), s.page.target_id().clone()));
        let Some((owner, target_id)) = snapshot else {
            return false;
        };
        owner
            .read()
            .await
            .browser
            .execute(GetTargetsParams::default())
            .await
            .is_ok_and(|targets| {
                targets
                    .result
                    .target_infos
                    .iter()
                    .any(|target| target.target_id == target_id)
            })
    }

    pub async fn vnc_port(&self, key: &str) -> Result<u16> {
        let owner = self
            .sessions
            .lock()
            .await
            .get(key)
            .map(|s| Arc::clone(&s.owner))
            .context("browser surface is not active")?;
        owner
            .read()
            .await
            .system_browser
            .as_ref()
            .and_then(system_browser::SystemBrowserLaunch::vnc_port)
            .context("VNC viewer is not available for this browser surface")
    }

    pub async fn close(&self, key: &str) -> Result<bool> {
        let key_operation = self.key_operation(key).await;
        let _key_operation_guard = key_operation.lock().await;
        self.close_key_locked(key).await
    }

    // Caller holds the per-surface key operation lock.
    async fn close_key_locked(&self, key: &str) -> Result<bool> {
        let retired_marker = self.closed_pages.lock().await.remove(key).is_some();
        let Some((profile, operation)) = self.operation_for_key(key).await else {
            return Ok(retired_marker);
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
        Ok(was_closed || retired_marker)
    }

    pub async fn close_profile(&self, profile: &Path) -> Result<usize> {
        let profile = profile
            .canonicalize()
            .unwrap_or_else(|_| profile.to_owned());
        let operation = self.profile_operations.lock().await.get(&profile).cloned();
        let Some(operation) = operation else {
            return Ok(0);
        };
        let _guard = operation.lock().await;
        self.close_owner_locked(&profile).await
    }

    pub async fn shutdown_all(&self) -> Result<usize> {
        self.shutting_down.store(true, Ordering::Release);
        let mut profiles = self
            .key_profiles
            .lock()
            .await
            .values()
            .cloned()
            .collect::<HashSet<_>>();
        profiles.extend(self.owners.lock().await.keys().cloned());
        let results = join_all(profiles.into_iter().map(|profile| async move {
            let operation = self.profile_operations.lock().await.get(&profile).cloned();
            let Some(operation) = operation else {
                return Ok(0);
            };
            let _operation = operation.lock().await;
            self.close_owner_locked(&profile).await
        }))
        .await;
        let mut closed = 0;
        let mut first_error = None;
        for result in results {
            match result {
                Ok(count) => closed += count,
                Err(error) => {
                    tracing::warn!(%error, "failed to close browser owner during shutdown");
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

    /// Caller holds the profile operation lock, shared by all its page owners.
    async fn close_owner_locked(&self, profile: &Path) -> Result<usize> {
        let owner = self.owners.lock().await.get(profile).cloned();
        let Some(owner) = owner else {
            return Ok(0);
        };
        owner.write().await.stop().await?;
        let mut sessions = self.sessions.lock().await;
        let before = sessions.len();
        sessions.retain(|_, session| !Arc::ptr_eq(&session.owner, &owner));
        let closed = before - sessions.len();
        drop(sessions);
        self.key_profiles
            .lock()
            .await
            .retain(|_, path| path != profile);
        self.owners.lock().await.remove(profile);
        Ok(closed)
    }

    async fn close_locked(&self, key: &str) -> Result<bool> {
        let snapshot = self
            .sessions
            .lock()
            .await
            .get(key)
            .map(|s| (Arc::clone(&s.owner), s.page.clone(), s.shared_browser));
        let Some((owner, page, shared)) = snapshot else {
            return Ok(false);
        };
        if !shared || owner.read().await.handler.is_finished() {
            let profile = self
                .key_profiles
                .lock()
                .await
                .get(key)
                .cloned()
                .context("browser profile registration missing")?;
            self.close_owner_locked(&profile).await?;
        } else {
            if let Err(error) = page.clone().close().await {
                page_recovery::confirm_candidate_gone(
                    &owner.read().await.browser,
                    &page,
                    error.into(),
                )
                .await?;
            }
            self.sessions.lock().await.remove(key);
        }
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
        keys.extend(self.closed_pages.lock().await.keys().cloned());
        keys
    }
}

async fn reusable_page(browser: &Browser) -> Result<Option<Page>> {
    for page in browser.pages().await? {
        let Some(url) = candidate_page_url(browser, &page).await? else {
            continue;
        };
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
    json!({
        "active":true,"state":"ready","message":"Browser surface is ready.","strategy":session.strategy,
        "url":session.url,"width":session.width,"height":session.height,
        "started_at":session.started_at,"updated_at":session.updated_at,
        "last_frame_seq":session.seq,"last_frame_at":session.updated_at,"controller_attached":false,
        "metadata":session.metadata,
        "viewer":session.viewer
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
