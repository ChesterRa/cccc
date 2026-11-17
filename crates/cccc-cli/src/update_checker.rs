//! The host's update check, offered to the Web it serves: the same check
//! `cccc update --check` makes, as data instead of text.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use cccc_core::HomeLayout;
use cccc_web::{
    LastUpdate, UpdateCheck, UpdateCheckHook, UpdateMode, UpdateRejection, UpdateRequest,
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
}

impl UpdateEntry {
    pub(crate) fn new(checker: UpdateCheckHook, trigger: TriggerSlot, in_place: bool) -> Self {
        Self {
            checker,
            trigger,
            in_place,
        }
    }

    /// Whether this host carries an update out itself.
    pub(crate) fn in_place(&self) -> bool {
        self.in_place
    }

    pub(crate) async fn check(&self) -> UpdateCheck {
        self.checker.check().await
    }

    /// Resolves once the update is prepared and the Web has closed for it.
    /// Dropping this future before then gives the update up. The terminal
    /// interface offers restarting agents only.
    pub(crate) async fn start(&self, version: String) -> Result<String, UpdateRejection> {
        let trigger = self.trigger.current().ok_or_else(|| UpdateRejection {
            code: "update_unavailable".into(),
            message: "CCCC can update itself only while its Web is serving".into(),
        })?;
        trigger
            .start(UpdateRequest {
                version,
                mode: UpdateMode::Restart,
            })
            .await
    }
}

pub(crate) struct HostChecker {
    home: HomeLayout,
}

impl HostChecker {
    pub(crate) fn new(home: HomeLayout) -> Self {
        Self { home }
    }
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
        Box::pin(async {
            match update::check_running_program().await {
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
                },
            }
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
    let last: LastResult = cccc_core::fs::read_json(&home.root().join("update/last.json")).ok()?;
    Some(LastUpdate {
        transaction: last.transaction,
        from_version: last.from_version,
        to_version: last.to_version,
        mode: last.mode,
        result: last.result,
        stage: last.stage,
        reason: last.reason,
        finished_at: last.finished_at,
    })
}

#[cfg(test)]
#[path = "update_checker_tests.rs"]
mod tests;
