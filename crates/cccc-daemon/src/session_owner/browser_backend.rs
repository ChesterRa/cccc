//! Resource reservation shares the owner admission boundary. Slow launch and
//! stop work runs outside it; a lost controller never drops the held browser.

use super::browser::{
    BrowserLaunchSpec, BrowserLauncher, BrowserPage, BrowserProcess, BrowserResource,
    BrowserResourceState,
};
use super::protocol::{Failure, Mutation};
use cccc_core::local_sessions::NativeHost;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_BROWSERS: usize = 64;
const MAX_SESSION_BYTES: usize = 128 * 1024;
const MAX_SETTING_BYTES: usize = 16 * 1024;

pub(super) struct BrowserStopReport {
    pub stopped: Vec<super::receipt::AgentIdentity>,
    pub unconfirmed: Vec<super::receipt::AgentIdentity>,
    pub error: Option<Failure>,
}

#[derive(Default)]
pub(super) struct Browsers {
    owner_id: String,
    instance: String,
    launcher: Option<Arc<dyn BrowserLauncher>>,
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    closed: bool,
    control_epoch: u64,
    entries: BTreeMap<PathBuf, Entry>,
}

struct Entry {
    id: String,
    spec: BrowserLaunchSpec,
    process: Option<Box<dyn BrowserProcess>>,
    native: Option<NativeHost>,
    sessions: Vec<BrowserPage>,
    pages_epoch: u64,
    busy: bool,
    issue: Option<String>,
}

impl Browsers {
    pub(super) fn new(owner_id: String, instance: String) -> Self {
        Self {
            owner_id,
            instance,
            ..Self::default()
        }
    }

    pub(super) fn set_launcher(&mut self, launcher: Option<Arc<dyn BrowserLauncher>>) {
        self.launcher = launcher;
    }

    fn state(&self) -> Result<MutexGuard<'_, State>, Failure> {
        self.state
            .lock()
            .map_err(|_| failed("browser registry lock failed"))
    }

    pub(super) fn validate(&self, mutation: &Mutation) -> Result<(), Failure> {
        let state = self.state()?;
        match mutation {
            Mutation::BrowserEnsure { spec } => {
                if state.closed {
                    return Err(failed("the browser holder is stopping"));
                }
                if !spec.profile.is_absolute()
                    || spec.width == 0
                    || spec.height == 0
                    || serde_json::to_vec(spec)
                        .map_err(|_| failed("could not encode browser settings"))?
                        .len()
                        > MAX_SETTING_BYTES
                {
                    return Err(Failure::new(
                        "invalid_input",
                        "browser profile and dimensions are invalid",
                    ));
                }
                if let Some(entry) = state.entries.get(&spec.profile) {
                    if !entry.spec.same_browser(spec) {
                        return Err(Failure::new(
                            "browser_settings_changed",
                            "the retained browser uses different launch settings; close it explicitly before changing them",
                        ));
                    }
                    require_idle(entry)?;
                } else if self.launcher.is_none() {
                    return Err(Failure::new(
                        "browser_not_supported",
                        "this holder has no browser launcher",
                    ));
                } else if state.entries.len() >= MAX_BROWSERS {
                    return Err(Failure::new(
                        "owner_capacity",
                        "browser resource capacity is full",
                    ));
                }
            }
            Mutation::BrowserPagesSet {
                resource_id,
                instance,
                sessions,
            } => {
                require_idle(self.entry(&state, resource_id, instance)?)?;
                let mut keys = BTreeSet::new();
                let mut targets = BTreeSet::new();
                if serde_json::to_vec(sessions)
                    .map_err(|_| failed("could not encode browser sessions"))?
                    .len()
                    > MAX_SESSION_BYTES
                    || sessions.iter().any(|page| {
                        page.key.is_empty()
                            || page.target_id.is_empty()
                            || !keys.insert(&page.key)
                            || !targets.insert(&page.target_id)
                            || page.actor.as_ref().is_some_and(|actor| {
                                actor.group_id.is_empty()
                                    || actor.actor_id.is_empty()
                                    || actor.actor_created_at.is_empty()
                            })
                    })
                {
                    return Err(Failure::new(
                        "invalid_input",
                        "browser session mappings are invalid or exceed the resource limit",
                    ));
                }
            }
            Mutation::BrowserClose {
                resource_id,
                instance,
            } => {
                let entry = self.entry(&state, resource_id, instance)?;
                if entry.busy {
                    return Err(failed("a browser operation is still in progress"));
                }
            }
            _ => return Err(failed("unexpected browser mutation")),
        }
        Ok(())
    }

    fn entry<'a>(&self, state: &'a State, id: &str, instance: &str) -> Result<&'a Entry, Failure> {
        if instance != self.instance {
            return Err(Failure::new(
                "browser_instance_changed",
                "the browser holder instance changed",
            ));
        }
        state
            .entries
            .values()
            .find(|entry| entry.id == id)
            .ok_or_else(|| {
                Failure::new("browser_not_owned", "the browser resource is not held here")
            })
    }

    /// Called while admission is locked, before any asynchronous worker starts.
    pub(super) fn reserve(&self, mutation: &Mutation) -> Result<(), Failure> {
        let mut state = self.state()?;
        match mutation {
            Mutation::BrowserEnsure { spec } => {
                state
                    .entries
                    .entry(spec.profile.clone())
                    .or_insert_with(|| Entry {
                        id: uuid::Uuid::new_v4().simple().to_string(),
                        spec: spec.clone(),
                        process: None,
                        native: None,
                        sessions: Vec::new(),
                        pages_epoch: 0,
                        busy: true,
                        issue: None,
                    });
            }
            Mutation::BrowserPagesSet { resource_id, .. }
            | Mutation::BrowserClose { resource_id, .. } => {
                let entry = state
                    .entries
                    .values_mut()
                    .find(|entry| entry.id == *resource_id)
                    .ok_or_else(|| failed("browser reservation disappeared"))?;
                entry.busy = true;
            }
            _ => return Err(failed("unexpected browser reservation")),
        }
        Ok(())
    }

    pub(super) fn prevent_starts(&self) -> Result<(), Failure> {
        self.state()?.closed = true;
        Ok(())
    }

    pub(super) fn control_acquired(&self, epoch: u64) -> Result<(), Failure> {
        self.state()?.control_epoch = epoch;
        Ok(())
    }

    pub(super) fn snapshots(&self) -> Result<Vec<super::protocol::BrowserSnapshot>, Failure> {
        let state = self.state()?;
        Ok(state
            .entries
            .values()
            .map(|entry| super::protocol::BrowserSnapshot {
                resource: self.observation(entry),
                reattached: state.control_epoch != 0 && entry.pages_epoch == state.control_epoch,
            })
            .collect())
    }

    pub(super) fn ensure(&self, spec: BrowserLaunchSpec) -> Result<BrowserResource, Failure> {
        {
            let mut state = self.state()?;
            let entry = state
                .entries
                .get(&spec.profile)
                .ok_or_else(|| failed("browser reservation is missing"))?;
            if entry.process.is_some() {
                return self.resource(entry);
            }
            if state.closed {
                state.entries.remove(&spec.profile);
                self.changed.notify_all();
                return Err(failed("browser launch was withdrawn before startup"));
            }
        }
        let launcher = self
            .launcher
            .as_ref()
            .ok_or_else(|| failed("browser launcher is missing"))?;
        // Browser launch owns its cleanup: Err means no resource remains.
        // Failed cleanup returns a process with initialization_error instead.
        let launched = tokio::runtime::Handle::current().block_on(launcher.launch(spec.clone()));
        let mut state = self.state()?;
        match launched {
            Ok(process) => {
                let native = NativeHost::capture(process.pid());
                let issue = process
                    .initialization_error()
                    .map(str::to_owned)
                    .or_else(|| {
                        native.as_ref().err().map(|error| {
                            format!("browser process identity is unconfirmed: {error}")
                        })
                    })
                    .or_else(|| {
                        (!serde_json::to_vec(process.connection())
                            .is_ok_and(|bytes| bytes.len() <= MAX_SETTING_BYTES))
                        .then(|| "browser connection metadata exceeds the resource limit".into())
                    });
                let entry = state
                    .entries
                    .get_mut(&spec.profile)
                    .ok_or_else(|| failed("browser reservation disappeared after launch"))?;
                entry.process = Some(process);
                entry.native = native.ok();
                entry.issue = issue;
                entry.busy = false;
                self.changed.notify_all();
                self.resource(entry)
            }
            Err(error) => {
                state.entries.remove(&spec.profile);
                self.changed.notify_all();
                Err(Failure::new("browser_launch_failed", error.to_string()))
            }
        }
    }

    fn resource(&self, entry: &Entry) -> Result<BrowserResource, Failure> {
        let resource = self.observation(entry);
        if resource.state != BrowserResourceState::Ready {
            return Err(failed(
                resource
                    .error
                    .unwrap_or_else(|| "the browser is not ready".into()),
            ));
        }
        Ok(resource)
    }

    fn observation(&self, entry: &Entry) -> BrowserResource {
        let state = if entry.busy {
            BrowserResourceState::Pending
        } else if let Some(native) = &entry.native {
            match NativeHost::capture(native.pid) {
                Ok(current) if current == *native && entry.issue.is_none() => {
                    BrowserResourceState::Ready
                }
                Ok(current) if current != *native => BrowserResourceState::Exited,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    BrowserResourceState::Exited
                }
                _ => BrowserResourceState::Unconfirmed,
            }
        } else {
            BrowserResourceState::Unconfirmed
        };
        BrowserResource {
            resource_id: entry.id.clone(),
            owner_id: self.owner_id.clone(),
            instance: self.instance.clone(),
            spec: entry.spec.clone(),
            connection: (state == BrowserResourceState::Ready)
                .then(|| {
                    entry
                        .process
                        .as_ref()
                        .map(|process| process.connection().clone())
                })
                .flatten(),
            state,
            error: entry.issue.clone().or_else(|| {
                (state != BrowserResourceState::Ready)
                    .then(|| "browser observation is not ready".into())
            }),
            sessions: entry.sessions.clone(),
        }
    }

    pub(super) fn list(&self) -> Result<Vec<BrowserResource>, Failure> {
        Ok(self
            .state()?
            .entries
            .values()
            .map(|entry| self.observation(entry))
            .collect())
    }

    pub(super) fn set_pages(
        &self,
        id: &str,
        sessions: Vec<BrowserPage>,
        epoch: u64,
    ) -> Result<BrowserResource, Failure> {
        let mut state = self.state()?;
        let entry = state
            .entries
            .values_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| failed("browser reservation is missing"))?;
        entry.busy = false;
        self.changed.notify_all();
        if epoch == 0 || epoch < entry.pages_epoch {
            return Err(Failure::new(
                "stale_epoch",
                "an older page publication cannot replace a newer one",
            ));
        }
        entry.sessions = sessions;
        entry.pages_epoch = epoch;
        self.resource(entry)
    }

    pub(super) fn close(&self, id: &str, timeout: Duration) -> Result<(), Failure> {
        let (profile, mut process, native) = {
            let mut state = self.state()?;
            let (profile, entry) = state
                .entries
                .iter_mut()
                .find(|(_, entry)| entry.id == id)
                .ok_or_else(|| failed("browser reservation is missing"))?;
            (
                profile.clone(),
                entry
                    .process
                    .take()
                    .ok_or_else(|| failed("browser launch is still in progress"))?,
                entry.native.clone(),
            )
        };
        let result = tokio::runtime::Handle::current().block_on(async {
            tokio::time::timeout(timeout, process.stop())
                .await
                .map_err(|_| failed("browser stop timed out; its handle is still held"))?
                .map_err(|error| failed(error.to_string()))?;
            if native.as_ref().is_some_and(|native| !native.has_exited()) {
                return Err(failed(
                    "browser stop returned without positive process exit evidence",
                ));
            }
            Ok(())
        });
        let mut state = self.state()?;
        if result.is_ok() {
            state.entries.remove(&profile);
        } else if let Some(entry) = state.entries.get_mut(&profile) {
            entry.process = Some(process);
            entry.busy = false;
        }
        self.changed.notify_all();
        result
    }

    pub(super) fn is_empty(&self) -> Result<bool, Failure> {
        Ok(self.state()?.entries.is_empty())
    }

    pub(super) fn stop_all(&self, timeout: Duration) -> Result<BrowserStopReport, Failure> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state()?;
        while state.entries.values().any(|entry| entry.busy) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(failed("browser work did not settle before stopping"));
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| failed("browser registry wait failed"))?
                .0;
        }
        let ids = state
            .entries
            .values_mut()
            .map(|entry| {
                entry.busy = true;
                let observed = super::protocol::BrowserSnapshot {
                    resource: self.observation(entry),
                    reattached: false,
                };
                (entry.id.clone(), observed.identities().collect::<Vec<_>>())
            })
            .collect::<Vec<_>>();
        drop(state);
        let mut errors = Vec::new();
        let mut report = BrowserStopReport {
            stopped: Vec::new(),
            unconfirmed: Vec::new(),
            error: None,
        };
        for (id, identities) in ids {
            if let Err(error) = self.close(&id, deadline.saturating_duration_since(Instant::now()))
            {
                errors.push(error.to_string());
                report.unconfirmed.extend(identities);
            } else {
                report.stopped.extend(identities);
            }
        }
        if !errors.is_empty() {
            report.error = Some(failed(errors.join("; ")));
        }
        Ok(report)
    }
}

fn require_idle(entry: &Entry) -> Result<(), Failure> {
    if entry.busy {
        return Err(failed("a browser operation is still in progress"));
    }
    if let Some(issue) = &entry.issue {
        return Err(failed(issue.clone()));
    }
    Ok(())
}

fn failed(message: impl Into<String>) -> Failure {
    Failure::new("browser_unconfirmed", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_owner::browser::{BrowserConnection, BrowserMode};
    use futures_util::future::BoxFuture;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Launcher {
        starts: Arc<AtomicUsize>,
        fail_stop: Arc<AtomicUsize>,
        issue: bool,
    }

    impl BrowserLauncher for Launcher {
        fn launch(
            &self,
            _spec: BrowserLaunchSpec,
        ) -> BoxFuture<'_, anyhow::Result<Box<dyn BrowserProcess>>> {
            Box::pin(async move {
                let first = self.starts.fetch_add(1, Ordering::SeqCst) == 0;
                let child = cccc_runtime::background_command("/bin/sleep")
                    .arg("60")
                    .spawn()?;
                Ok(Box::new(Process {
                    child,
                    fail_stop: self.fail_stop.clone(),
                    issue: self.issue && first,
                    connection: BrowserConnection {
                        websocket_url: "ws://127.0.0.1:9/fixture".into(),
                        display: None,
                        vnc_port: None,
                        strategy: "fixture".into(),
                        metadata: serde_json::Value::Null,
                        viewer: serde_json::Value::Null,
                    },
                }) as Box<dyn BrowserProcess>)
            })
        }
    }

    struct Process {
        child: std::process::Child,
        connection: BrowserConnection,
        fail_stop: Arc<AtomicUsize>,
        issue: bool,
    }

    impl BrowserProcess for Process {
        fn pid(&self) -> u32 {
            self.child.id()
        }
        fn connection(&self) -> &BrowserConnection {
            &self.connection
        }
        fn initialization_error(&self) -> Option<&str> {
            self.issue.then_some("injected initialization failure")
        }
        fn stop(&mut self) -> BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async move {
                if self
                    .fail_stop
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                        count.checked_sub(1)
                    })
                    .is_ok()
                {
                    anyhow::bail!("injected stop failure");
                }
                if self.child.try_wait()?.is_none() {
                    self.child.kill()?;
                    self.child.wait()?;
                }
                Ok(())
            })
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn fixture(
        issue: bool,
        failures: usize,
    ) -> (
        tempfile::TempDir,
        Browsers,
        BrowserLaunchSpec,
        Arc<AtomicUsize>,
    ) {
        let temp = tempfile::tempdir().expect("isolated profile");
        let starts = Arc::new(AtomicUsize::new(0));
        let mut browsers = Browsers::new("owner".into(), "instance".into());
        browsers.set_launcher(Some(Arc::new(Launcher {
            starts: starts.clone(),
            fail_stop: Arc::new(AtomicUsize::new(failures)),
            issue,
        })));
        let spec = BrowserLaunchSpec {
            profile: temp.path().to_path_buf(),
            mode: BrowserMode::Headless,
            width: 800,
            height: 600,
            proxy: None,
        };
        (temp, browsers, spec, starts)
    }

    fn ensure(browsers: &Browsers, spec: &BrowserLaunchSpec) -> Result<BrowserResource, Failure> {
        let mutation = Mutation::BrowserEnsure { spec: spec.clone() };
        browsers.validate(&mutation)?;
        browsers.reserve(&mutation)?;
        browsers.ensure(spec.clone())
    }

    fn member_page(target: &str) -> BrowserPage {
        BrowserPage {
            key: "web-model::group::member".into(),
            actor: Some(super::super::browser::BrowserActorIdentity {
                group_id: "group".into(),
                actor_id: "member".into(),
                actor_created_at: "created".into(),
            }),
            target_id: target.into(),
            state: serde_json::Value::Null,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn only_a_current_epoch_page_publication_proves_web_reattachment() {
        tokio::task::spawn_blocking(|| {
            let (_temp, browsers, spec, _) = fixture(false, 0);
            let resource = ensure(&browsers, &spec).expect("held browser");
            browsers.control_acquired(1).expect("first controller");
            browsers
                .set_pages(&resource.resource_id, vec![member_page("original")], 1)
                .expect("first Web mapping");
            let first = browsers.snapshots().expect("first observation").remove(0);
            let expected = first.identities().next().expect("member identity");
            assert!(first.reattached);
            browsers.control_acquired(2).expect("next daemon");
            let retained = browsers.snapshots().expect("retained mapping").remove(0);
            assert!(
                !retained.reattached,
                "holder takeover is not a Web connection"
            );
            assert_eq!(retained.identities().next(), Some(expected.clone()));
            // A worker already admitted by the old stream may finish, but it
            // can never claim to be a publication from the new controller.
            browsers
                .set_pages(&resource.resource_id, vec![member_page("original")], 1)
                .expect("old admitted completion");
            assert!(!browsers.snapshots().expect("old completion")[0].reattached);
            browsers
                .set_pages(&resource.resource_id, vec![member_page("original")], 2)
                .expect("new Web adopted all pages");
            assert!(browsers.snapshots().expect("new publication")[0].reattached);
            assert!(
                browsers
                    .set_pages(&resource.resource_id, vec![member_page("stale")], 1)
                    .is_err()
            );
            assert_eq!(
                browsers.snapshots().expect("new mapping stays")[0]
                    .identities()
                    .next(),
                Some(expected.clone())
            );
            browsers
                .set_pages(&resource.resource_id, vec![member_page("replacement")], 2)
                .expect("new member window");
            assert_ne!(
                browsers.snapshots().expect("new target")[0]
                    .identities()
                    .next()
                    .expect("identity")
                    .generation,
                expected.generation
            );
            let stopped = browsers
                .stop_all(Duration::from_secs(2))
                .expect("stop browser");
            assert!(stopped.error.is_none());
        })
        .await
        .expect("blocking fixture");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stop_all_names_only_browser_members_whose_resources_really_stopped() {
        tokio::task::spawn_blocking(|| {
            let (_temp, browsers, spec, _) = fixture(false, 1);
            let resource = ensure(&browsers, &spec).expect("held browser");
            browsers
                .set_pages(&resource.resource_id, vec![member_page("target")], 1)
                .expect("actual mapping");
            let expected = browsers.snapshots().expect("before stop")[0]
                .identities()
                .next()
                .expect("member");
            browsers.prevent_starts().expect("ordinary stop");
            let failed = browsers
                .stop_all(Duration::from_secs(2))
                .expect("attempt all stops");
            assert!(failed.error.is_some());
            assert!(failed.stopped.is_empty());
            assert_eq!(failed.unconfirmed, vec![expected.clone()]);
            assert!(!browsers.is_empty().expect("failed handle retained"));
            let confirmed = browsers
                .stop_all(Duration::from_secs(2))
                .expect("explicit stop retry");
            assert!(confirmed.error.is_none());
            assert_eq!(confirmed.stopped, vec![expected]);
            assert!(confirmed.unconfirmed.is_empty());
            assert!(browsers.is_empty().expect("resources gone"));
        })
        .await
        .expect("blocking fixture");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn one_profile_keeps_one_process_and_rejects_stale_page_and_close_requests() {
        tokio::task::spawn_blocking(|| {
            let (_temp, browsers, spec, starts) = fixture(false, 0);
            let first = ensure(&browsers, &spec).expect("launch");
            let pid = browsers.state().expect("registry").entries[&spec.profile]
                .native
                .as_ref()
                .expect("native identity")
                .clone();
            assert_eq!(ensure(&browsers, &spec).expect("same resource"), first);
            let mut another_viewport = spec.clone();
            another_viewport.width = 1366;
            another_viewport.height = 900;
            assert_eq!(
                ensure(&browsers, &another_viewport).expect("first launch retains its viewport"),
                first
            );
            assert_eq!(starts.load(Ordering::SeqCst), 1);
            let stale = Mutation::BrowserClose {
                resource_id: first.resource_id.clone(),
                instance: "old-instance".into(),
            };
            assert!(browsers.validate(&stale).is_err());
            assert!(pid.alive());
            let pages = vec![BrowserPage {
                key: "session".into(),
                actor: None,
                target_id: "target".into(),
                state: serde_json::json!({"url":"fixture","web_private":{"field":7}}),
            }];
            let mutation = Mutation::BrowserPagesSet {
                resource_id: first.resource_id.clone(),
                instance: first.instance.clone(),
                sessions: pages.clone(),
            };
            browsers.validate(&mutation).expect("valid instance");
            browsers.reserve(&mutation).expect("reserve metadata");
            assert_eq!(
                browsers
                    .set_pages(&first.resource_id, pages.clone(), 1)
                    .expect("replace mapping")
                    .sessions,
                pages
            );
            let close = Mutation::BrowserClose {
                resource_id: first.resource_id.clone(),
                instance: first.instance.clone(),
            };
            browsers.validate(&close).expect("close instance");
            browsers.reserve(&close).expect("reserve close");
            browsers
                .close(&first.resource_id, Duration::from_secs(2))
                .expect("close");
            assert!(pid.has_exited());
            assert!(browsers.is_empty().expect("registry"));
        })
        .await
        .expect("blocking fixture");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn failed_stop_and_failed_initialization_keep_the_handle_for_stop_all() {
        tokio::task::spawn_blocking(|| {
            let (_temp, browsers, spec, starts) = fixture(true, 1);
            assert!(ensure(&browsers, &spec).is_err());
            assert!(
                !browsers
                    .is_empty()
                    .expect("held despite initialization failure")
            );
            let listed = browsers.list().expect("explicit per-resource observation");
            assert_eq!(listed.len(), 1, "unknown is not an empty ready set");
            assert_eq!(listed[0].state, BrowserResourceState::Unconfirmed);
            browsers.prevent_starts().expect("stop fence");
            assert!(
                browsers
                    .stop_all(Duration::from_secs(2))
                    .expect("stop attempted")
                    .error
                    .is_some()
            );
            assert!(!browsers.is_empty().expect("failed stop retains handle"));
            assert_eq!(starts.load(Ordering::SeqCst), 1);
            browsers
                .stop_all(Duration::from_secs(2))
                .expect("retry held stop");
            assert!(browsers.is_empty().expect("stopped"));
        })
        .await
        .expect("blocking fixture");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stop_all_withdraws_an_admitted_launch_before_it_starts() {
        tokio::task::spawn_blocking(|| {
            let (_temp, browsers, spec, starts) = fixture(false, 0);
            let mutation = Mutation::BrowserEnsure { spec: spec.clone() };
            browsers.validate(&mutation).expect("validate");
            browsers.reserve(&mutation).expect("reserve before worker");
            browsers.prevent_starts().expect("stop selected");
            assert!(browsers.ensure(spec).is_err());
            browsers
                .stop_all(Duration::from_millis(10))
                .expect("nothing started");
            assert_eq!(starts.load(Ordering::SeqCst), 0);
            assert!(browsers.is_empty().expect("withdrawn reservation"));
        })
        .await
        .expect("blocking fixture");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_holder_with_only_a_browser_cannot_retire_until_stop_all_cleans_it() {
        tokio::task::spawn_blocking(|| {
            let (temp, _unused, spec, starts) = fixture(false, 0);
            let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
            let mut backend = super::super::backend::Backend::with_owner(
                home.clone(),
                "owner".into(),
                "instance".into(),
            );
            backend.set_browser_launcher(Some(Arc::new(Launcher {
                starts,
                fail_stop: Arc::new(AtomicUsize::new(0)),
                issue: false,
            })));
            let launch = Mutation::BrowserEnsure { spec };
            backend.validate(&launch).expect("validate launch");
            backend.reserve(&launch).expect("reserve launch");
            backend.execute(&home, launch).expect("launch held browser");
            assert!(!backend.is_empty().expect("browser counts as a resource"));
            assert!(backend.retire().is_err());
            backend
                .validate(&Mutation::StopAll)
                .expect("validate stop all");
            backend
                .reserve(&Mutation::StopAll)
                .expect("fence browser starts");
            let result = backend
                .execute(&home, Mutation::StopAll)
                .expect("stop resources");
            assert_eq!(result["browsers_stopped"], true);
            assert!(backend.is_empty().expect("resources stopped"));
            backend.retire().expect("empty owner may retire");
        })
        .await
        .expect("blocking fixture");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_unknown_profile_does_not_hide_a_different_healthy_profile() {
        tokio::task::spawn_blocking(|| {
            let (temp, browsers, spec, starts) = fixture(true, 0);
            assert!(ensure(&browsers, &spec).is_err());
            let mut second = spec.clone();
            second.profile = temp.path().join("second");
            std::fs::create_dir(&second.profile).expect("second profile");
            let ready = ensure(&browsers, &second).expect("unrelated healthy profile");
            assert_eq!(ready.state, BrowserResourceState::Ready);
            let listed = browsers.list().expect("complete observations");
            assert_eq!(listed.len(), 2);
            assert!(listed.iter().any(|resource| resource.state
                == BrowserResourceState::Unconfirmed
                && resource.connection.is_none()));
            assert!(
                listed
                    .iter()
                    .any(|resource| resource.state == BrowserResourceState::Ready
                        && resource.resource_id == ready.resource_id)
            );
            assert_eq!(starts.load(Ordering::SeqCst), 2);
            browsers.prevent_starts().expect("stop fence");
            browsers
                .stop_all(Duration::from_secs(2))
                .expect("stop both");
        })
        .await
        .expect("blocking fixture");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_positive_exit_is_reported_separately_and_its_handle_can_be_closed() {
        tokio::task::spawn_blocking(|| {
            let (_temp, browsers, spec, _starts) = fixture(false, 0);
            let resource = ensure(&browsers, &spec).expect("held process");
            let native = browsers.state().expect("registry").entries[&spec.profile]
                .native
                .clone()
                .expect("native child");
            // The fixture still owns its unreaped Child, so the PID cannot be reused.
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(native.pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            )
            .expect("simulate Chrome exiting");
            let deadline = Instant::now() + Duration::from_secs(2);
            while !native.has_exited() {
                assert!(Instant::now() < deadline, "fixture did not exit");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                browsers.list().expect("positive exit")[0].state,
                BrowserResourceState::Exited
            );
            let close = Mutation::BrowserClose {
                resource_id: resource.resource_id.clone(),
                instance: resource.instance,
            };
            browsers.validate(&close).expect("close exited resource");
            browsers.reserve(&close).expect("reserve cleanup");
            browsers
                .close(&resource.resource_id, Duration::from_secs(2))
                .expect("release exited process and auxiliaries");
            assert!(browsers.is_empty().expect("clean registry"));
        })
        .await
        .expect("blocking fixture");
    }
}
