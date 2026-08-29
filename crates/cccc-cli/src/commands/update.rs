use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::args::{ReleaseChannelArg, UpdateArgs};

#[cfg(not(windows))]
const UNIX_INSTALLER_URL: &str = "https://chesterra.github.io/cccc/install.sh";
#[cfg(windows)]
const WINDOWS_INSTALLER_URL: &str = "https://chesterra.github.io/cccc/install.ps1";
const INSTALL_MARKER: &str = ".cccc-standalone";
const INSTALL_MARKER_VERSION: &str = "standalone-v1";
const PIP_INSTALL_MARKER_VERSION: &str = "pip-v1";
#[cfg(any(windows, test))]
const WINDOWS_INSTALL_COMMAND: &str = concat!(
    "Wait-Process -Id $env:CCCC_UPDATE_PARENT_PID -ErrorAction SilentlyContinue; ",
    "[Net.ServicePointManager]::SecurityProtocol = ",
    "[Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; ",
    "Invoke-RestMethod -Uri $env:CCCC_INSTALLER_URL | Invoke-Expression",
);

pub async fn run(args: UpdateArgs) -> Result<()> {
    let executable = std::env::current_exe().context("could not resolve the CCCC executable")?;
    let install_dir = standalone_install_dir(&executable)?;
    let channel = effective_channel(args.channel);

    if args.check {
        println!("Current version: {}", crate::PRODUCT_VERSION);
        println!("Install directory: {}", install_dir.display());
        println!("Release channel: {}", channel_name(channel));
        println!("Installer: {}", installer_url());
        return Ok(());
    }

    let version = latest_channel_version(channel).await?;
    run_installer(&install_dir, &executable, Some(&version))
}

fn effective_channel(requested: Option<ReleaseChannelArg>) -> ReleaseChannelArg {
    requested.unwrap_or_else(|| {
        if crate::PRODUCT_VERSION.contains('-') {
            ReleaseChannelArg::Rc
        } else {
            ReleaseChannelArg::Stable
        }
    })
}

const fn channel_name(channel: ReleaseChannelArg) -> &'static str {
    match channel {
        ReleaseChannelArg::Stable => "stable",
        ReleaseChannelArg::Rc => "rc",
    }
}

async fn latest_channel_version(channel: ReleaseChannelArg) -> Result<String> {
    let repository =
        std::env::var("CCCC_GITHUB_REPOSITORY").unwrap_or_else(|_| "ChesterRa/cccc".into());
    if repository.split('/').count() != 2
        || repository.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
        })
    {
        bail!("CCCC_GITHUB_REPOSITORY must use the owner/repository form");
    }
    let releases = reqwest::Client::new()
        .get(format!(
            "https://api.github.com/repos/{repository}/releases?per_page=30"
        ))
        .header(reqwest::header::USER_AGENT, "cccc-standalone-updater")
        .send()
        .await
        .context("could not query GitHub releases for the RC channel")?
        .error_for_status()
        .context("GitHub rejected the RC release query")?
        .json::<Vec<serde_json::Value>>()
        .await
        .context("GitHub returned an invalid releases response")?;
    releases
        .iter()
        .find_map(|release| release_version(release, channel))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no published CCCC {} release was found",
                channel_name(channel)
            )
        })
}

fn release_version(release: &serde_json::Value, channel: ReleaseChannelArg) -> Option<String> {
    let prerelease = release["prerelease"].as_bool()?;
    if release["draft"].as_bool() == Some(true) || prerelease != (channel == ReleaseChannelArg::Rc)
    {
        return None;
    }
    let version = release["tag_name"].as_str()?.strip_prefix('v')?;
    (valid_release_version(version)
        && match channel {
            ReleaseChannelArg::Stable => !version.contains('-'),
            ReleaseChannelArg::Rc => version.contains('-'),
        })
    .then(|| version.to_owned())
}

fn valid_release_version(version: &str) -> bool {
    let core = version.split_once('-').map_or(version, |(core, _)| core);
    core.split('.').count() == 3
        && core
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn standalone_install_dir(executable: &Path) -> Result<PathBuf> {
    let install_dir = executable
        .parent()
        .context("CCCC executable has no parent directory")?;
    let marker = install_dir.join(INSTALL_MARKER);
    match std::fs::read_to_string(&marker) {
        Ok(value) if value.trim() == INSTALL_MARKER_VERSION => {}
        Ok(value) if value.trim() == PIP_INSTALL_MARKER_VERSION => bail!(
            "this CCCC executable is managed by pip; update it with python -m pip install --upgrade \"cccc-pair>=0.4.36\""
        ),
        Ok(_) => bail!(
            "this Rust executable is managed by another installation or has an unrecognized owner; update it through that installer"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => bail!(
            "this CCCC executable is not an owned standalone installation; update it through its package manager (for pip: python -m pip install --upgrade \"cccc-pair>=0.4.36\")"
        ),
        Err(error) => return Err(error).context(format!("could not read {}", marker.display())),
    }
    Ok(install_dir.to_path_buf())
}

#[cfg(not(windows))]
fn run_installer(install_dir: &Path, executable: &Path, version: Option<&str>) -> Result<()> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("curl -fsSL \"$CCCC_INSTALLER_URL\" | sh")
        .env("CCCC_INSTALLER_URL", UNIX_INSTALLER_URL)
        .env("CCCC_INSTALL_DIR", install_dir)
        .env("CCCC_TRUSTED_EXISTING_CLI", executable);
    if let Some(version) = version {
        command.env("CCCC_VERSION", version);
    }
    let status = command
        .status()
        .context("could not start the CCCC installer")?;
    if !status.success() {
        bail!("CCCC installer exited with {status}");
    }
    Ok(())
}

#[cfg(windows)]
fn run_installer(install_dir: &Path, executable: &Path, version: Option<&str>) -> Result<()> {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            WINDOWS_INSTALL_COMMAND,
        ])
        .env("CCCC_UPDATE_PARENT_PID", std::process::id().to_string())
        .env("CCCC_INSTALLER_URL", WINDOWS_INSTALLER_URL)
        .env("CCCC_INSTALL_DIR", install_dir)
        .env("CCCC_TRUSTED_EXISTING_CLI", executable)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(version) = version {
        command.env("CCCC_VERSION", version);
    }
    let child = command
        .spawn()
        .context("could not start the CCCC updater")?;
    println!("Started CCCC updater (process {}).", child.id());
    Ok(())
}

#[cfg(not(windows))]
fn installer_url() -> &'static str {
    UNIX_INSTALLER_URL
}

#[cfg(windows)]
fn installer_url() -> &'static str {
    WINDOWS_INSTALLER_URL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_self_update_requires_the_complete_ownership_marker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp
            .path()
            .join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
        std::fs::write(&executable, b"binary").expect("binary");
        let missing = standalone_install_dir(&executable).expect_err("missing marker");
        assert!(
            missing
                .to_string()
                .contains("python -m pip install --upgrade \"cccc-pair>=0.4.36\"")
        );

        std::fs::write(temp.path().join(INSTALL_MARKER), b"foreign-v1\n").expect("foreign marker");
        assert!(standalone_install_dir(&executable).is_err());

        std::fs::write(
            temp.path().join(INSTALL_MARKER),
            format!("{INSTALL_MARKER_VERSION}\n"),
        )
        .expect("marker");
        assert_eq!(
            standalone_install_dir(&executable).expect("standalone install"),
            temp.path()
        );

        std::fs::write(temp.path().join(INSTALL_MARKER), b"pip-v1\n").expect("pip marker");
        let pip_owned = standalone_install_dir(&executable)
            .expect_err("pip ownership must override a stale standalone marker");
        assert!(pip_owned.to_string().contains("managed by pip"));
    }

    #[test]
    fn update_channel_defaults_to_the_installed_release_family() {
        assert_eq!(
            effective_channel(None),
            if crate::PRODUCT_VERSION.contains('-') {
                ReleaseChannelArg::Rc
            } else {
                ReleaseChannelArg::Stable
            }
        );
        assert_eq!(
            effective_channel(Some(ReleaseChannelArg::Stable)),
            ReleaseChannelArg::Stable
        );
    }

    #[test]
    fn validates_release_versions_before_passing_them_to_the_installer() {
        assert!(valid_release_version("0.4.34-rc2"));
        assert!(valid_release_version("1.2.3"));
        assert!(!valid_release_version("latest"));
        assert!(!valid_release_version("1.2.3/../../escape"));
    }

    #[test]
    fn windows_updater_enables_tls_before_downloading_the_installer() {
        let tls = WINDOWS_INSTALL_COMMAND
            .find("[Net.SecurityProtocolType]::Tls12")
            .expect("Windows updater TLS bootstrap");
        let download = WINDOWS_INSTALL_COMMAND
            .find("Invoke-RestMethod")
            .expect("Windows updater download");
        assert!(tls < download);
    }

    #[test]
    fn release_selection_keeps_stable_and_prerelease_channels_separate() {
        let stable = serde_json::json!({
            "tag_name":"v1.2.3","prerelease":false,"draft":false
        });
        let rc = serde_json::json!({
            "tag_name":"v1.3.0-rc2","prerelease":true,"draft":false
        });
        assert_eq!(
            release_version(&stable, ReleaseChannelArg::Stable).as_deref(),
            Some("1.2.3")
        );
        assert!(release_version(&stable, ReleaseChannelArg::Rc).is_none());
        assert_eq!(
            release_version(&rc, ReleaseChannelArg::Rc).as_deref(),
            Some("1.3.0-rc2")
        );
    }
}
