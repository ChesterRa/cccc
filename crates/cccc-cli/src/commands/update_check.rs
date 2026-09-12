use std::future::Future;
use std::io::Write;

use super::*;

pub(super) async fn report(
    output: &mut impl Write,
    executable: &Path,
    current: &str,
    args: &UpdateArgs,
    latest: impl Future<Output = Result<String>>,
) -> Result<()> {
    let owner = installation_owner(executable)?;
    let channel = effective_channel(args.channel);
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
        match owner {
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
    let (supported, requirement) = platform_requirement(
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(target_env = "musl"),
    );
    writeln!(output, "Native package: {requirement}")?;
    if owner == InstallationOwner::Standalone {
        writeln!(output, "Installer: {}", installer_url())?;
    }
    if args.offline {
        writeln!(output, "Latest version: not checked (offline)")?;
        return Ok(());
    }

    let latest = match latest.await {
        Ok(version) => version,
        Err(error) => {
            writeln!(
                output,
                "Latest version: unavailable; update status is unknown"
            )?;
            return Err(error)
                .context("update check failed; no installation or running service was changed");
        }
    };
    writeln!(
        output,
        "Latest {channel} version: {latest}",
        channel = channel_name(channel)
    )?;
    let installed = release_version(current)?;
    let published = release_version(&latest)?;
    let switching = args.channel.is_some() && installed.pre.is_empty() != published.pre.is_empty();
    let actionable = if !supported {
        writeln!(
            output,
            "Update status: no current native package for this build platform"
        )?;
        false
    } else if switching {
        writeln!(output, "Update status: explicit channel switch available")?;
        true
    } else {
        match published.cmp_precedence(&installed) {
            std::cmp::Ordering::Greater => {
                writeln!(output, "Update status: newer release available")?;
                true
            }
            std::cmp::Ordering::Equal => {
                writeln!(output, "Update status: already current on this channel")?;
                false
            }
            std::cmp::Ordering::Less => {
                writeln!(
                    output,
                    "Update status: installed version is newer than the published channel; automatic downgrade is refused"
                )?;
                false
            }
        }
    };
    if actionable {
        match owner {
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
                let source = if channel == ReleaseChannelArg::Rc {
                    " --pre --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/"
                } else {
                    ""
                };
                writeln!(
                    output,
                    "Next step: python -m pip install --upgrade{source} \"cccc-pair=={latest}\""
                )?;
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
