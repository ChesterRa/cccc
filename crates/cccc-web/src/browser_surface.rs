mod chatgpt_dom;
mod frame;
pub(crate) mod held;
mod input_events;
mod interaction;
mod navigation;
mod owner;
mod page_recovery;
mod profile_owner;
mod prompt_submission;
mod proxy;
mod resources;
mod system_browser;

#[cfg(test)]
mod chrome_test_guard;
#[cfg(test)]
pub(crate) use chrome_test_guard::chrome_test_guard;

pub use interaction::{serve_socket, serve_vnc_socket};
pub(crate) use prompt_submission::{
    BOUND_CONVERSATION_ERROR_MARKER, PromptSubmissionOutcome, conversation_target_matches,
    is_chatgpt_url, normalized_chatgpt_conversation_url,
};

use anyhow::{Context, Result, bail};
use cccc_contracts::utc_now;
use cccc_daemon::session_owner::browser::{
    BrowserActorIdentity, BrowserLaunchSpec, BrowserMode as HeldMode, BrowserPage,
};
use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::target::{
    CloseTargetParams, CreateTargetParams, GetTargetsParams,
};
use futures_util::future::join_all;
use navigation::goto_dom_content_loaded;
use owner::BrowserOwner;
mod pairing;
use page_recovery::{candidate_page_url, close_internal_pages, is_internal_page};
pub(crate) use pairing::{PairingPage, pairing_prompt};
use resources::{HeldResource, OwnerResources};
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

// Browser ownership includes the provider: changing runtime must retire the old
// window and its manual-close marker without changing the Actor's task identity.
pub(crate) fn actor_identity(actor: &cccc_contracts::Actor) -> String {
    format!(
        "{:?}:{}",
        actor.runtime,
        cccc_core::actors::generation_identity(actor)
    )
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
    /// Set where the session owner keeps shared browsers for this Web.
    resources: std::sync::OnceLock<OwnerResources>,
}

#[derive(Debug)]
pub(crate) struct BrowserSurfaceContentChanged;

impl std::fmt::Display for BrowserSurfaceContentChanged {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the shared browser session has a different initial URL")
    }
}

impl std::error::Error for BrowserSurfaceContentChanged {}

#[derive(Clone)]
pub(super) struct Session {
    owner: Arc<RwLock<BrowserOwner>>,
    pub(super) page: Page,
    is_system_browser: bool,
    shared_browser: bool,
    viewer: Value,
    /// Presentation's published address; ordinary navigation changes `url` instead.
    initial_url: Option<String>,
    url: String,
    pub(super) width: u32,
    pub(super) height: u32,
    started_at: String,
    pub(super) updated_at: String,
    seq: u64,
    strategy: String,
    metadata: Value,
    recover_closed_page: bool,
    /// The member a window belongs to, as the session owner records it.
    actor: Option<BrowserActorIdentity>,
}

/// A browser the session owner keeps, with the pages this Web's surfaces
/// used in it, so that this Web takes them back instead of opening new ones.
struct PreservedBrowser {
    websocket_url: String,
    shared: bool,
    strategy: String,
    metadata: Value,
    viewer: Value,
    sessions: Vec<BrowserPage>,
}

/// What a surface keeps across a Web restart besides its page. The session
/// owner stores it as given and hands it to the next Web.
#[derive(serde::Serialize, serde::Deserialize)]
struct KeptSurface {
    url: String,
    initial_url: Option<String>,
    width: u32,
    height: u32,
    is_system_browser: bool,
    shared_browser: bool,
    viewer: Value,
    metadata: Value,
    started_at: String,
    strategy: String,
    recover_closed_page: bool,
}

fn kept_page(key: &str, session: &Session) -> BrowserPage {
    BrowserPage {
        key: key.to_owned(),
        actor: session.actor.clone(),
        target_id: session.page.target_id().inner().clone(),
        state: json!(KeptSurface {
            url: session.url.clone(),
            initial_url: session.initial_url.clone(),
            width: session.width,
            height: session.height,
            is_system_browser: session.is_system_browser,
            shared_browser: session.shared_browser,
            viewer: session.viewer.clone(),
            metadata: session.metadata.clone(),
            started_at: session.started_at.clone(),
            strategy: session.strategy.clone(),
            recover_closed_page: session.recover_closed_page,
        }),
    }
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
    track_initial_url: bool,
    mode: BrowserMode,
    shared_browser: bool,
    actor_identity: Option<&'a str>,
    actor: Option<&'a BrowserActorIdentity>,
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
                    s.metadata["actor_identity"].as_str().map(str::to_owned),
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
                                a.runtime.is_web_model() && actor_identity(a) == *old
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

    #[cfg(test)]
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
            track_initial_url: false,
            mode: BrowserMode::Headless,
            shared_browser: false,
            actor_identity: None,
            actor: None,
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
            track_initial_url: true,
            mode: BrowserMode::Headless,
            shared_browser: false,
            actor_identity: None,
            actor: None,
        })
        .await
    }

    /// Opens a Presentation page, or reuses its current session without replacing another card.
    pub async fn open_presentation(
        &self,
        key: &str,
        profile: &Path,
        url: &str,
        width: u32,
        height: u32,
        reuse_existing: bool,
    ) -> Result<Value> {
        self.open_with(OpenRequest {
            key,
            profile,
            url,
            width,
            height,
            storage_state: None,
            reuse_existing,
            track_initial_url: true,
            mode: BrowserMode::Headless,
            shared_browser: false,
            actor_identity: None,
            actor: None,
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
            track_initial_url: false,
            mode: BrowserMode::System { background: false },
            shared_browser: true,
            actor_identity: None,
            actor: None,
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
        page.bring_to_front()
            .await
            .context("show shared login window")?;
        if is_internal_page(&page.url().await?.unwrap_or_default()) {
            navigation::start_navigation(&page, url).await?;
        }
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
        actor_created_at: &str,
    ) -> Result<Value> {
        // A kept window counts as its member kept, so the session owner
        // records which member, down to its creation, the window belongs to.
        let actor = session_actor(key)
            .filter(|_| !actor_created_at.is_empty())
            .map(|(group_id, actor_id)| BrowserActorIdentity {
                group_id: group_id.to_owned(),
                actor_id: actor_id.to_owned(),
                actor_created_at: actor_created_at.to_owned(),
            });
        self.open_with(OpenRequest {
            key,
            profile,
            url,
            width: dimensions.0,
            height: dimensions.1,
            storage_state: None,
            reuse_existing: true,
            track_initial_url: false,
            mode: BrowserMode::System { background: false },
            shared_browser: true,
            actor_identity: Some(generation),
            actor: actor.as_ref(),
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
            track_initial_url: false,
            mode: BrowserMode::Headless,
            shared_browser: false,
            actor_identity: None,
            actor: None,
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
            track_initial_url: false,
            mode: BrowserMode::System { background: true },
            shared_browser: false,
            actor_identity: None,
            actor: None,
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
            track_initial_url,
            mode,
            shared_browser,
            actor_identity,
            actor,
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
                track_initial_url,
                mode,
                shared_browser,
                actor_identity,
                actor,
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
        // Shared windows must be viewable and closable while the site loads.
        // Login starts navigation in its caller; Actor delivery checks composer
        // and target readiness separately before claiming or sending work.
        if let Some(page) = page.filter(|_| !shared_browser || actor_identity.is_some()) {
            let opened = async {
                if shared_browser {
                    navigation::start_navigation(&page, url)
                        .await
                        .context("open browser page")?;
                } else {
                    goto_dom_content_loaded(&page, url)
                        .await
                        .context("open browser page")?;
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
            track_initial_url,
            mode,
            shared_browser,
            actor_identity,
            actor,
        } = request;
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("browser surfaces are shutting down");
        }
        // The session owner may keep this profile's shared browser from an
        // earlier Web, or from this one after its connection ended, with that
        // Web's pages: take them all back first, so a surface reuses its own
        // page instead of opening another.
        let held = self.resources.get().filter(|_| shared_browser);
        let mut kept_no_page = false;
        if let Some(resources) = held
            && self.needs_connection_locked(profile).await
        {
            kept_no_page = self
                .take_held_locked(resources, profile, held_spec(profile, width, height, mode)?)
                .await?;
        }
        if reuse_existing {
            let existing = self.sessions.lock().await.get(key).map(|s| {
                (
                    Arc::clone(&s.owner),
                    state(s),
                    s.page.target_id().clone(),
                    s.initial_url.clone(),
                )
            });
            if let Some((owner, state, target_id, initial_url)) = existing {
                let owner = owner.read().await;
                if !owner.handler.is_finished()
                    && actor_identity.is_none_or(|g| state["metadata"]["actor_identity"] == g)
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
                        if track_initial_url && initial_url.as_deref() != Some(url) {
                            return Err(BrowserSurfaceContentChanged.into());
                        }
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
                // A browser this Web runs stops here; from a kept one this Web
                // only lets go, and connects to it again below.
                self.close_owner_locked(profile).await?;
                None
            } else {
                Some(owner)
            }
        } else {
            None
        };
        let (owner, new_owner) = match (owner, held) {
            (Some(owner), _) => (owner, kept_no_page),
            (None, Some(resources)) => {
                let kept_no_page = self
                    .take_held_locked(resources, profile, held_spec(profile, width, height, mode)?)
                    .await?;
                let owner = self
                    .owners
                    .lock()
                    .await
                    .get(profile)
                    .cloned()
                    .context("the kept browser was not taken back")?;
                (owner, kept_no_page)
            }
            (None, None) => {
                let owner = Arc::new(RwLock::new(
                    BrowserOwner::launch(
                        profile,
                        width,
                        height,
                        mode,
                        shared_browser,
                        storage_state,
                        proxy::BrowserProxy::from_env()?,
                    )
                    .await?,
                ));
                self.owners
                    .lock()
                    .await
                    .insert(profile.to_owned(), Arc::clone(&owner));
                (owner, true)
            }
        };
        let initialized = async {
            let owner = owner.read().await;
            if shared_browser {
                let params = CreateTargetParams::builder()
                    .url("about:blank")
                    .new_window(true)
                    .background(actor_identity.is_some())
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
                let viewport = match frame::viewport_size(&page).await {
                    Ok(viewport) => viewport,
                    Err(error) => {
                        let _ = page.close().await;
                        return Err(error);
                    }
                };
                Ok((page, viewport))
            } else {
                let page = match reusable_page(&owner.browser).await? {
                    Some(page) => Ok(page),
                    None => owner
                        .browser
                        .new_page("about:blank")
                        .await
                        .context("create browser page"),
                }?;
                Ok((page, (width, height)))
            }
        }
        .await;
        let (page, (width, height)) = match initialized {
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
        if let Some(generation) = actor_identity {
            metadata["actor_identity"] = json!(generation);
        }
        let session = Session {
            owner: Arc::clone(&owner),
            page: page.clone(),
            is_system_browser: resource.is_system_browser(),
            shared_browser,
            viewer: if actor_identity.is_some() {
                json!({"kind":"screencast","vnc":{"available":false,"error":"actor_page_only"}})
            } else {
                resource.viewer.clone()
            },
            initial_url: track_initial_url.then(|| url.into()),
            url: url.into(),
            width,
            height,
            started_at: now.clone(),
            updated_at: now,
            seq: 0,
            strategy: resource.strategy.clone(),
            metadata,
            recover_closed_page: !shared_browser && matches!(mode, BrowserMode::Headless),
            actor: actor.cloned(),
        };
        drop(resource);
        // A kept browser's owner records the page before any surface uses it.
        if let Err(error) = self
            .publish_pages_locked(&owner, Some((key, &session)))
            .await
        {
            let closed = owner
                .read()
                .await
                .browser
                .execute(CloseTargetParams::new(page.target_id().clone()))
                .await;
            if let Err(close_error) = closed {
                tracing::warn!(%close_error, "an unrecorded page stays open in the kept browser");
            }
            return Err(error.context("record the new page with the session owner"));
        }
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
                s.metadata["actor_identity"].as_str().map(str::to_owned),
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
        let kept = owner.held.is_some();
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
            // A kept browser may still run with this page, and only the
            // session owner can tell: a status read reports the lost
            // connection and changes nothing. Opening again reconnects.
            if kept {
                return disconnected();
            }
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
            .vnc_port()
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

    /// Opens no surface from now on. Open ones keep working until they are
    /// closed or detached, so work already under way can finish.
    pub fn stop_opening(&self) {
        self.shutting_down.store(true, Ordering::Release);
    }

    pub fn stopping(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    /// Caller holds the profile operation lock. Every kept page is found or
    /// proven gone before any is registered, so a failure takes back nothing.
    async fn adopt_locked(
        &self,
        profile: &Path,
        preserved: PreservedBrowser,
        held: HeldResource,
    ) -> Result<usize> {
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("browser surfaces are shutting down");
        }
        if self.owners.lock().await.contains_key(profile) {
            bail!(
                "browser profile is already managed by this Web: {}",
                profile.display()
            );
        }
        let mut owner = BrowserOwner::adopt(&preserved, held).await?;
        let mut restored = Vec::new();
        for kept in preserved.sessions {
            // A surface open elsewhere under this key is the current one.
            if self.sessions.lock().await.contains_key(&kept.key) {
                continue;
            }
            let found = match serde_json::from_value::<KeptSurface>(kept.state) {
                Ok(surface) => find_page(&owner.browser, &kept.target_id)
                    .await
                    .map(|page| page.map(|page| (surface, page))),
                Err(_) => Err(anyhow::anyhow!("a kept page's surface could not be read")),
            };
            match found {
                Ok(Some((surface, page))) => restored.push((kept.key, kept.actor, surface, page)),
                Ok(None) => {}
                Err(error) => {
                    owner.detach().await;
                    return Err(error);
                }
            }
        }
        let owner = Arc::new(RwLock::new(owner));
        let now = utc_now();
        let mut count = 0;
        for (key, actor, surface, page) in restored {
            // A key bound to another profile is opening or open there, so its
            // page here is stale. Same lock order as bind_profile.
            let mut key_profiles = self.key_profiles.lock().await;
            if key_profiles.get(&key).is_some_and(|bound| bound != profile) {
                continue;
            }
            let mut sessions = self.sessions.lock().await;
            if sessions.contains_key(&key) {
                continue;
            }
            key_profiles.insert(key.clone(), profile.to_owned());
            sessions.insert(
                key,
                Session {
                    owner: Arc::clone(&owner),
                    page,
                    is_system_browser: surface.is_system_browser,
                    shared_browser: surface.shared_browser,
                    viewer: surface.viewer,
                    initial_url: surface.initial_url,
                    url: surface.url,
                    width: surface.width,
                    height: surface.height,
                    started_at: surface.started_at,
                    updated_at: now.clone(),
                    seq: 0,
                    strategy: surface.strategy,
                    metadata: surface.metadata,
                    recover_closed_page: surface.recover_closed_page,
                    actor,
                },
            );
            count += 1;
        }
        self.owners.lock().await.insert(profile.to_owned(), owner);
        Ok(count)
    }

    /// Whether this Web needs a connection to the browser the session owner
    /// keeps for this profile: it has none yet, or its connection ended. An
    /// ended connection does not show that the browser ended; the owner
    /// tells, and starts another only then.
    async fn needs_connection_locked(&self, profile: &Path) -> bool {
        let owner = self.owners.lock().await.get(profile).cloned();
        match owner {
            None => true,
            Some(owner) => {
                let owner = owner.read().await;
                owner.held.is_some() && owner.handler.is_finished()
            }
        }
    }

    /// Caller holds the profile operation lock, and this Web has no live
    /// connection to the browser kept for this profile. Connects to the
    /// browser the session owner keeps, which the owner starts when it keeps
    /// none or the kept one ended, and takes back every page of this Web
    /// recorded with it. An ended connection is let go only once the owner
    /// answered, so a failed request leaves it as it was. Returns whether the
    /// owner recorded no page, as for a new browser.
    async fn take_held_locked(
        &self,
        resources: &OwnerResources,
        profile: &Path,
        spec: BrowserLaunchSpec,
    ) -> Result<bool> {
        let (resource, connection) = resources.ensure(&spec).await?;
        let ended = self.owners.lock().await.get(profile).cloned();
        if let Some(ended) = ended {
            ended.write().await.detach().await;
            self.forget_owner_locked(profile, &ended).await;
        }
        let kept_no_page = resource.sessions.is_empty();
        let held = HeldResource {
            vnc_port: connection.vnc_port,
            resource_id: resource.resource_id,
            instance: resource.instance,
            spec: resource.spec,
        };
        let browser = PreservedBrowser {
            websocket_url: connection.websocket_url,
            shared: true,
            strategy: connection.strategy,
            metadata: connection.metadata,
            viewer: connection.viewer,
            sessions: resource.sessions,
        };
        self.adopt_locked(profile, browser, held).await?;
        // Recording what was taken back tells the owner this Web drives those
        // pages again, and drops pages that no longer exist.
        if !kept_no_page {
            let owner = self
                .owners
                .lock()
                .await
                .get(profile)
                .cloned()
                .context("the kept browser was not taken back")?;
            if let Err(error) = self.publish_pages_locked(&owner, None).await {
                tracing::warn!(error = %format!("{error:#}"), "the session owner did not record the pages taken back");
            }
        }
        Ok(kept_no_page)
    }

    /// Caller holds the profile operation lock. Records with the session
    /// owner which pages of its browser this Web's surfaces use, `opening`
    /// among them. Browsers this Web runs itself record nothing.
    async fn publish_pages_locked(
        &self,
        owner: &Arc<RwLock<BrowserOwner>>,
        opening: Option<(&str, &Session)>,
    ) -> Result<()> {
        let Some(held) = owner.read().await.held.clone() else {
            return Ok(());
        };
        let resources = self
            .resources
            .get()
            .context("a kept browser has no session owner")?;
        let mut pages = self
            .sessions
            .lock()
            .await
            .iter()
            .filter(|(key, session)| {
                Arc::ptr_eq(&session.owner, owner)
                    && opening.is_none_or(|(opening, _)| opening != key.as_str())
            })
            .map(|(key, session)| kept_page(key, session))
            .collect::<Vec<_>>();
        pages.extend(opening.map(|(key, session)| kept_page(key, session)));
        resources.set_pages(&held, pages).await
    }

    /// From now on the session owner keeps the browsers that surfaces share,
    /// so that they outlive this Web. Set it before any surface opens.
    pub(crate) fn hold_shared_browsers(&self, client: cccc_client::DaemonClient) {
        let _ = self.resources.set(OwnerResources::new(client));
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
            let owner = self.owners.lock().await.get(&profile).cloned();
            let held = match &owner {
                Some(owner) => owner.read().await.held.is_some(),
                None => false,
            };
            match owner.filter(|_| held) {
                // The session owner keeps it running for the next Web.
                Some(owner) => {
                    self.release_held_locked(&profile, &owner).await;
                    Ok(0)
                }
                None => self.close_owner_locked(&profile).await,
            }
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
    /// Stops a browser this Web runs. Only the session owner stops a browser
    /// it keeps: this Web lets go of its connection, and the pages recorded
    /// with the owner stay for the next connection.
    async fn close_owner_locked(&self, profile: &Path) -> Result<usize> {
        let owner = self.owners.lock().await.get(profile).cloned();
        let Some(owner) = owner else {
            return Ok(0);
        };
        let mut resource = owner.write().await;
        if resource.held.is_some() {
            resource.detach().await;
        } else {
            resource.stop().await?;
        }
        drop(resource);
        Ok(self.forget_owner_locked(profile, &owner).await)
    }

    /// Caller holds the profile operation lock. Leaves a kept browser and its
    /// pages running for the next Web, with their latest addresses recorded,
    /// and drops only this Web's connection.
    async fn release_held_locked(&self, profile: &Path, owner: &Arc<RwLock<BrowserOwner>>) {
        if let Err(error) = self.publish_pages_locked(owner, None).await {
            tracing::warn!(error = %format!("{error:#}"), "the session owner keeps earlier page addresses");
        }
        owner.write().await.detach().await;
        self.forget_owner_locked(profile, owner).await;
    }

    /// Caller holds the profile operation lock. Removes an owner and its
    /// surfaces from this Web and returns how many surfaces it had.
    async fn forget_owner_locked(
        &self,
        profile: &Path,
        owner: &Arc<RwLock<BrowserOwner>>,
    ) -> usize {
        let mut sessions = self.sessions.lock().await;
        let before = sessions.len();
        sessions.retain(|_, session| !Arc::ptr_eq(&session.owner, owner));
        let removed = before - sessions.len();
        drop(sessions);
        self.key_profiles
            .lock()
            .await
            .retain(|_, path| path != profile);
        self.owners.lock().await.remove(profile);
        removed
    }

    /// Caller holds the profile operation lock.
    async fn close_locked(&self, key: &str) -> Result<bool> {
        let Some((mut owner, mut page, shared)) = self.surface_page(key).await else {
            return Ok(false);
        };
        let ended = owner.read().await.handler.is_finished();
        let held = owner.read().await.held.clone();
        if shared
            && ended
            && let Some(held) = held
            && let Some(resources) = self.resources.get()
        {
            // Its connection ended, not necessarily its browser: connect
            // again and close only this surface's page.
            let profile = self.surface_profile(key).await?;
            self.take_held_locked(resources, &profile, held.spec)
                .await?;
            let Some((connected, taken_back, _)) = self.surface_page(key).await else {
                // Its page ended with the browser it was in.
                return Ok(true);
            };
            owner = connected;
            page = taken_back;
        }
        if !shared || owner.read().await.handler.is_finished() {
            let profile = self.surface_profile(key).await?;
            self.close_owner_locked(&profile).await?;
        } else {
            // Close the owned target through the browser: Page.close can reject
            // a still-existing window while its renderer changes during navigation.
            let resource = owner.read().await;
            if let Err(error) = resource
                .browser
                .execute(CloseTargetParams::new(page.target_id().clone()))
                .await
            {
                // A user may already have closed this target. Completion is
                // established by its absence in a live browser, not error text.
                let targets = resource
                    .browser
                    .execute(GetTargetsParams::default())
                    .await?;
                if targets
                    .result
                    .target_infos
                    .iter()
                    .any(|target| &target.target_id == page.target_id())
                {
                    return Err(error.into());
                }
            }
            drop(resource);
            self.sessions.lock().await.remove(key);
            // The next Web takes back only pages that are still recorded.
            if let Err(error) = self.publish_pages_locked(&owner, None).await {
                tracing::warn!(error = %format!("{error:#}"), "the session owner still records a closed page");
            }
        }
        Ok(true)
    }

    async fn surface_page(&self, key: &str) -> Option<(Arc<RwLock<BrowserOwner>>, Page, bool)> {
        self.sessions
            .lock()
            .await
            .get(key)
            .map(|s| (Arc::clone(&s.owner), s.page.clone(), s.shared_browser))
    }

    async fn surface_profile(&self, key: &str) -> Result<PathBuf> {
        self.key_profiles
            .lock()
            .await
            .get(key)
            .cloned()
            .context("browser profile registration missing")
    }

    async fn register_profile(&self, key: &str, profile: &Path) -> Result<Arc<Mutex<()>>> {
        self.bind_profile(key, profile).await?;
        Ok(self.profile_operation(profile).await)
    }

    async fn profile_operation(&self, profile: &Path) -> Arc<Mutex<()>> {
        let mut operations = self.profile_operations.lock().await;
        Arc::clone(
            operations
                .entry(profile.to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
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

/// A new connection learns existing targets asynchronously; a target that
/// is absent from the browser's own list is gone, not undiscovered.
async fn find_page(browser: &Browser, target_id: &str) -> Result<Option<Page>> {
    use chromiumoxide::cdp::browser_protocol::target::TargetId;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match browser.get_page(TargetId::new(target_id)).await {
            Ok(page) => return Ok(Some(page)),
            Err(chromiumoxide::error::CdpError::NotFound) => {}
            Err(error) => return Err(error).context("find preserved browser page"),
        }
        let targets = browser
            .execute(GetTargetsParams::default())
            .await
            .context("list preserved browser targets")?;
        if !targets
            .result
            .target_infos
            .iter()
            .any(|target| target.target_id.inner() == target_id)
        {
            return Ok(None);
        }
        if std::time::Instant::now() >= deadline {
            bail!("preserved browser page {target_id} was not attached in time");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
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
    let mut value = json!({
        "active":true,"state":"ready","message":"Browser surface is ready.","strategy":session.strategy,
        "url":session.url,"width":session.width,"height":session.height,
        "started_at":session.started_at,"updated_at":session.updated_at,
        "last_frame_seq":session.seq,"last_frame_at":session.updated_at,"controller_attached":false,
        "metadata":session.metadata,
        "viewer":session.viewer
    });
    if let Some(initial_url) = &session.initial_url {
        value["initial_url"] = json!(initial_url);
    }
    value
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

fn disconnected() -> Value {
    let message = "The connection to the browser ended. Opening the surface again reconnects it.";
    json!({
        "active":false,"state":"failed","message":message,
        "error":{"code":"browser_surface_disconnected","message":message},
        "width":0,"height":0,"last_frame_seq":0,"controller_attached":false,
        "viewer":{"kind":"screencast","vnc":{"available":false,"error":"Browser connection ended"}}
    })
}

/// What a shared surface asks the session owner to keep for this profile.
fn held_spec(
    profile: &Path,
    width: u32,
    height: u32,
    mode: BrowserMode,
) -> Result<BrowserLaunchSpec> {
    Ok(BrowserLaunchSpec {
        profile: profile.to_owned(),
        mode: match mode {
            BrowserMode::Headless => HeldMode::Headless,
            BrowserMode::System { background } => HeldMode::System { background },
        },
        width,
        height,
        proxy: proxy::BrowserProxy::from_env()?
            .as_ref()
            .map(proxy::BrowserProxy::to_owner),
    })
}

#[cfg(test)]
mod browser_surface_tests;
