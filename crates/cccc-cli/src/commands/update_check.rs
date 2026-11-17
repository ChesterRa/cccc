use std::future::Future;
use std::io::Write;

use super::*;

/// The installation facts a check reads locally, without the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Installation {
    pub owner: InstallationOwner,
    pub channel: ReleaseChannelArg,
    pub platform_supported: bool,
    pub platform_requirement: &'static str,
}

/// What release discovery found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Latest {
    /// Offline: release discovery was not asked.
    NotChecked,
    /// Discovery failed, so whether an update exists is unknown.
    Unavailable(String),
    Found {
        version: String,
        status: Status,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    NoPackage,
    ChannelSwitch,
    Newer,
    Current,
    InstalledNewer,
}

impl Status {
    pub(crate) fn actionable(self) -> bool {
        matches!(self, Self::ChannelSwitch | Self::Newer)
    }
}

/// One check of this program, for every place CCCC shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckOutcome {
    pub current: String,
    pub installation: Installation,
    pub latest: Latest,
}

pub(crate) fn installation(
    executable: &Path,
    requested_channel: Option<ReleaseChannelArg>,
) -> Result<Installation> {
    let (platform_supported, platform_requirement) = platform_requirement(
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(target_env = "musl"),
    );
    Ok(Installation {
        owner: installation_owner(executable)?,
        channel: effective_channel(requested_channel),
        platform_supported,
        platform_requirement,
    })
}

/// Asks release discovery once, unless offline. A failed or unusable answer
/// leaves the update status unknown; it never reads as current.
pub(crate) async fn latest(
    current: &str,
    requested_channel: Option<ReleaseChannelArg>,
    platform_supported: bool,
    offline: bool,
    discovered: impl Future<Output = Result<String>>,
) -> Latest {
    if offline {
        return Latest::NotChecked;
    }
    let version = match discovered.await {
        Ok(version) => version,
        Err(error) => return Latest::Unavailable(format!("{error:#}")),
    };
    match status(current, &version, requested_channel, platform_supported) {
        Ok(status) => Latest::Found { version, status },
        Err(error) => Latest::Unavailable(format!("{error:#}")),
    }
}

fn status(
    current: &str,
    published: &str,
    requested_channel: Option<ReleaseChannelArg>,
    platform_supported: bool,
) -> Result<Status> {
    let installed = release_version(current)?;
    let published = release_version(published)?;
    let switching =
        requested_channel.is_some() && installed.pre.is_empty() != published.pre.is_empty();
    Ok(if !platform_supported {
        Status::NoPackage
    } else if switching {
        Status::ChannelSwitch
    } else {
        match published.cmp_precedence(&installed) {
            std::cmp::Ordering::Greater => Status::Newer,
            std::cmp::Ordering::Equal => Status::Current,
            std::cmp::Ordering::Less => Status::InstalledNewer,
        }
    })
}

pub(crate) async fn check(
    executable: &Path,
    current: &str,
    requested_channel: Option<ReleaseChannelArg>,
    offline: bool,
    discovered: impl Future<Output = Result<String>>,
) -> Result<CheckOutcome> {
    let installation = installation(executable, requested_channel)?;
    let latest = latest(
        current,
        requested_channel,
        installation.platform_supported,
        offline,
        discovered,
    )
    .await;
    Ok(CheckOutcome {
        current: current.to_owned(),
        installation,
        latest,
    })
}

/// The command that updates a pip installation to `latest`.
pub(crate) fn pip_command(channel: ReleaseChannelArg, latest: &str) -> String {
    let source = if channel == ReleaseChannelArg::Rc {
        " --pre --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/"
    } else {
        ""
    };
    format!("python -m pip install --upgrade{source} \"cccc-pair=={latest}\"")
}

pub(super) async fn report(
    output: &mut impl Write,
    executable: &Path,
    current: &str,
    args: &UpdateArgs,
    discovered: impl Future<Output = Result<String>>,
) -> Result<()> {
    let installation = installation(executable, args.channel)?;
    let channel = installation.channel;
    writeln!(output, "Current version: {current}")?;
    writeln!(output, "Executable: {}", executable.display())?;
    writeln!(
        output,
        "Install directory: {}",
        executable
            .parent()
            .context("CCCC executable has no parent directory")?
            .display()
    )?;
    writeln!(
        output,
        "Installation: {}",
        match installation.owner {
            InstallationOwner::Standalone => "website installer",
            InstallationOwner::Pip => "pip (manual package-manager update)",
            InstallationOwner::Unmanaged => "unmanaged / source build (no ownership marker)",
            InstallationOwner::Unrecognized => "unrecognized ownership marker",
        }
    )?;
    writeln!(output, "Release channel: {}", channel_name(channel))?;
    writeln!(
        output,
        "Build platform: {}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )?;
    writeln!(
        output,
        "Native package: {}",
        installation.platform_requirement
    )?;
    if installation.owner == InstallationOwner::Standalone {
        writeln!(output, "Installer: {}", installer_url())?;
    }

    let (latest, status) = match latest(
        current,
        args.channel,
        installation.platform_supported,
        args.offline,
        discovered,
    )
    .await
    {
        Latest::NotChecked => {
            writeln!(output, "Latest version: not checked (offline)")?;
            return Ok(());
        }
        Latest::Unavailable(reason) => {
            writeln!(
                output,
                "Latest version: unavailable; update status is unknown"
            )?;
            return Err(anyhow::anyhow!(reason))
                .context("update check failed; no installation or running service was changed");
        }
        Latest::Found { version, status } => (version, status),
    };
    writeln!(
        output,
        "Latest {channel} version: {latest}",
        channel = channel_name(channel)
    )?;
    writeln!(
        output,
        "Update status: {}",
        match status {
            Status::NoPackage => "no current native package for this build platform",
            Status::ChannelSwitch => "explicit channel switch available",
            Status::Newer => "newer release available",
            Status::Current => "already current on this channel",
            Status::InstalledNewer =>
                "installed version is newer than the published channel; automatic downgrade is refused",
        }
    )?;
    if status.actionable() {
        match installation.owner {
            InstallationOwner::Standalone => {
                writeln!(
                    output,
                    "Next step: cccc update --channel {}",
                    channel_name(channel)
                )?;
            }
            InstallationOwner::Pip => {
                writeln!(
                    output,
                    "Use the Python environment that owns this executable; stop CCCC before replacing it."
                )?;
                writeln!(output, "Next step: {}", pip_command(channel, &latest))?;
                writeln!(
                    output,
                    "An unavailable package must fail explicitly; check the pip index and platform instead of accepting an older version."
                )?;
            }
            InstallationOwner::Unmanaged | InstallationOwner::Unrecognized => {
                writeln!(
                    output,
                    "Next step: update through the package manager or source checkout that owns this executable."
                )?;
            }
        }
    }
    Ok(())
}

fn platform_requirement(os: &str, arch: &str, musl: bool) -> (bool, &'static str) {
    match (os, arch, musl) {
        ("linux", "x86_64", false) => (true, "Linux x86-64 requires glibc 2.28 or later"),
        ("macos", "aarch64", _) => (true, "Apple Silicon requires macOS 11 or later"),
        ("windows", "x86_64", _) => (true, "Windows x86-64"),
        ("macos", "x86_64", _) => (
            false,
            "Intel Mac support ended with v0.4.37; use that archived release",
        ),
        ("linux", _, true) => (
            false,
            "musl/Alpine is not supported by the published native packages",
        ),
        _ => (
            false,
            "no published native package for this platform; v0.4.35 is the last portable Python release",
        ),
    }
}

#[cfg(test)]
#[path = "update_check_tests.rs"]
mod tests;
