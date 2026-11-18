//! The host's update check, offered to the Web it serves: the same check
//! `cccc update --check` makes, as data instead of text, and on a host that
//! can restart into it, whether its program has been rebuilt since it started.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use cccc_core::HomeLayout;
use cccc_core::update_image::FileIdentity;
use cccc_web::{
    LastUpdate, LocalBuild, UpdateCheck, UpdateCheckHook, UpdateRejection, UpdateRequest,
    UpdateStatus, UpdateTrigger,
};

use crate::commands::update::{self, CheckOutcome, InstallationOwner, Latest, Status};
use crate::update_host::LastResult;

/// The serving Web's in-process update entry; empty while no Web serves.
#[derive(Clone, Default)]
pub(crate) struct TriggerSlot(Arc<Mutex<Option<UpdateTrigger>>>);

impl TriggerSlot {
    /// Keeps the entry each serve hands out.
    pub(crate) fn callback(&self) -> cccc_web::TriggerCallback {
        let slot = self.clone();
        Arc::new(move |trigger| *slot.lock() = Some(trigger))
    }

    /// That Web has closed: an update started now is not carried out.
    pub(crate) fn clear(&self) {
        *self.lock() = None;
    }

    fn current(&self) -> Option<UpdateTrigger> {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<UpdateTrigger>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the host's terminal interface offers for updates: the same check as
/// the Web's, and, on a host that can replace itself, the same handoff.
#[derive(Clone)]
pub(crate) struct UpdateEntry {
    checker: UpdateCheckHook,
    trigger: TriggerSlot,
    in_place: bool,
    local_restart: bool,
    release_keep: bool,
}

impl UpdateEntry {
    pub(crate) fn new(
        checker: UpdateCheckHook,
        trigger: TriggerSlot,
        in_place: bool,
        local_restart: bool,
        release_keep: bool,
    ) -> Self {
        Self {
            checker,
            trigger,
            in_place,
            local_restart,
            release_keep,
        }
    }

    /// Whether this host carries an update out itself.
    pub(crate) fn in_place(&self) -> bool {
        self.in_place
    }

    /// Whether this host restarts into a rebuilt program at its launch path.
    pub(crate) fn local_restart(&self) -> bool {
        self.local_restart
    }

    /// Whether this host offers to keep agents running through a release update.
    pub(crate) fn release_keep(&self) -> bool {
        self.release_keep
    }

    pub(crate) async fn check(&self) -> UpdateCheck {
        self.checker.check().await
    }

    /// Resolves once the update is prepared and the Web has closed for it.
    /// Dropping this future before then gives the update up.
    pub(crate) async fn start(&self, request: UpdateRequest) -> Result<String, UpdateRejection> {
        let trigger = self.trigger.current().ok_or_else(|| UpdateRejection {
            code: "update_unavailable".into(),
            message: "CCCC can update itself only while its Web is serving".into(),
        })?;
        trigger.start(request).await
    }
}

pub(crate) struct HostChecker {
    home: HomeLayout,
    /// Only a host that can restart into a rebuilt program has one.
    launch: Option<LaunchProgram>,
}

impl HostChecker {
    pub(crate) fn new(home: HomeLayout, launch: Option<LaunchProgram>) -> Self {
        Self { home, launch }
    }
}

/// The program this host was started from, as it was then. A rebuild replaces
/// the file at that path while this process keeps running the old one, so the
/// path is read once, at start.
#[derive(Clone, Debug)]
pub(crate) struct LaunchProgram {
    pub(crate) path: PathBuf,
    pub(crate) identity: FileIdentity,
}

impl LaunchProgram {
    /// The running program: the path it was started from, and the file this
    /// process actually runs, read from its own image rather than from the
    /// path, in case a build replaced the file meanwhile. Writing a new program
    /// at the path changes that file even when the build's source id stays the
    /// same, as after a Web-only rebuild. Unknown where the running image
    /// cannot be read, or when the path no longer names a file.
    pub(crate) fn current() -> Option<Self> {
        let path = std::env::current_exe().ok()?;
        let identity = FileIdentity::running().ok()?;
        path.is_file().then_some(Self { path, identity })
    }
}

/// What is now at the launch path, when it is no longer the program this host
/// was started from. An unchanged file is not run; one that cannot be read is
/// reported as such, never as nothing new. Whether agents could keep running
/// through a restart into it is the daemon's own first look; it decides again
/// when it is asked to stop.
async fn local_build(home: HomeLayout, launch: Option<LaunchProgram>) -> Option<LocalBuild> {
    let launch = launch?;
    let unreadable = |error: std::io::Error| LocalBuild::Unreadable {
        reason: format!("{}: {error}", launch.path.display()),
    };
    let identity = match FileIdentity::read(&launch.path) {
        Ok(identity) if identity == launch.identity => return None,
        Ok(identity) => identity,
        Err(error) => return Some(unreadable(error)),
    };
    let token = match identity.token() {
        Ok(token) => token,
        Err(error) => return Some(unreadable(error)),
    };
    let check = crate::update_host::self_check(&launch.path).await;
    // What answered must be the file read before it, not one a build was
    // still writing.
    if FileIdentity::read(&launch.path).ok().as_ref() != Some(&identity) {
        return Some(LocalBuild::Unreadable {
            reason: "it changed while it was being read; check again".into(),
        });
    }
    Some(match check {
        Ok(check) if !check.accepts_local_restart() => LocalBuild::Incompatible {
            version: check.version,
        },
        Ok(check) => {
            let mut keep =
                cccc_daemon::session_owner::preservation_check(&home, &check.capabilities).await;
            crate::update_host::name_refusals(&home, &mut keep);
            LocalBuild::rebuilt(
                check.version,
                check.build.source_id,
                identity.written,
                token,
                keep,
            )
        }
        Err(error) => LocalBuild::Unreadable {
            reason: format!("{error:#}"),
        },
    })
}

impl cccc_web::UpdateChecker for HostChecker {
    fn status(&self) -> UpdateStatus {
        UpdateStatus {
            current: crate::PRODUCT_VERSION.into(),
            installation: update::running_installation()
                .map(|installation| owner_name(installation.owner))
                .unwrap_or("unrecognized")
                .into(),
            last: last_update(&self.home),
        }
    }

    fn check(&self) -> Pin<Box<dyn Future<Output = UpdateCheck> + Send>> {
        let home = self.home.clone();
        let launch = self.launch.clone();
        Box::pin(async move {
            // Independent answers: a failed release check still shows a rebuild.
            let (release, local) =
                tokio::join!(update::check_running_program(), local_build(home, launch));
            let mut check = match release {
                Ok(outcome) => describe(outcome),
                Err(error) => UpdateCheck {
                    current: crate::PRODUCT_VERSION.into(),
                    channel: String::new(),
                    installation: "unrecognized".into(),
                    latest: None,
                    status: None,
                    failure: Some(format!("{error:#}")),
                    release_notes_url: None,
                    command: None,
                    local: None,
                },
            };
            check.local = local;
            check
        })
    }
}

fn describe(outcome: CheckOutcome) -> UpdateCheck {
    let installation = &outcome.installation;
    let channel = update::channel_name(installation.channel);
    let (latest, status, failure) = match &outcome.latest {
        Latest::Found { version, status } => (Some(version.clone()), Some(*status), None),
        Latest::Unavailable(reason) => (None, None, Some(reason.clone())),
        Latest::NotChecked => (None, None, Some("release discovery was not asked".into())),
    };
    let command = match (&latest, status) {
        (Some(latest), Some(status)) if status.actionable() => match installation.owner {
            InstallationOwner::Standalone => Some(format!("cccc update --channel {channel}")),
            InstallationOwner::Pip => Some(update::pip_command(installation.channel, latest)),
            InstallationOwner::Unmanaged | InstallationOwner::Unrecognized => None,
        },
        _ => None,
    };
    UpdateCheck {
        current: outcome.current,
        channel: channel.into(),
        installation: owner_name(installation.owner).into(),
        release_notes_url: latest.as_deref().map(release_notes_url),
        latest,
        status: status.map(|status| status_name(status).into()),
        failure,
        command,
        local: None,
    }
}

fn owner_name(owner: InstallationOwner) -> &'static str {
    match owner {
        InstallationOwner::Standalone => "standalone",
        InstallationOwner::Pip => "pip",
        InstallationOwner::Unmanaged => "unmanaged",
        InstallationOwner::Unrecognized => "unrecognized",
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::NoPackage => "no_package",
        Status::ChannelSwitch => "channel_switch",
        Status::Newer => "newer",
        Status::Current => "current",
        Status::InstalledNewer => "installed_newer",
    }
}

/// The release page of `version`, in the repository releases come from.
fn release_notes_url(version: &str) -> String {
    let repository =
        std::env::var("CCCC_GITHUB_REPOSITORY").unwrap_or_else(|_| "ChesterRa/cccc".into());
    let prefix = std::env::var("CCCC_RELEASE_TAG_PREFIX").unwrap_or_else(|_| "v".into());
    format!("https://github.com/{repository}/releases/tag/{prefix}{version}")
}

/// The latest attended update's recorded result. An unreadable record is
/// left out rather than guessed.
fn last_update(home: &HomeLayout) -> Option<LastUpdate> {
    let mut last: LastResult =
        cccc_core::fs::read_json(&home.root().join("update/last.json")).ok()?;
    // Recorded while this host looked for its agents, and nothing looks now:
    // the look was cut short, so the agents it lists stay unconfirmed.
    if last.agents.state == "observing" && !crate::update_host::observing_agents() {
        last.agents.state = "unconfirmed".into();
    }
    Some(LastUpdate {
        transaction: last.transaction,
        from_version: last.from_version,
        to_version: last.to_version,
        mode: last.mode,
        result: last.result,
        stage: last.stage,
        reason: last.reason,
        finished_at: last.finished_at,
        source: last.source,
        token: last.token,
        source_id: last.source_id,
        agents: last.agents,
    })
}

#[cfg(test)]
#[path = "update_checker_tests.rs"]
mod tests;
