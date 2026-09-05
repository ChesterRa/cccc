use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::args::{ReleaseChannelArg, UpdateArgs};

const RELEASE_INDEX_URL: &str = "https://chesterra.github.io/cccc/releases.json";
const RELEASE_INDEX_MAX_BYTES: usize = 16 * 1024;

#[cfg(test)]
#[path = "update_release_tests.rs"]
mod release_tests;

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
    if !should_install(crate::PRODUCT_VERSION, &version, args.channel)? {
        println!(
            "CCCC {} is already current on the {} channel.",
            crate::PRODUCT_VERSION,
            channel_name(channel)
        );
        return Ok(());
    }
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
        || repository.split('/').any(str::is_empty)
        || repository.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
        })
    {
        bail!("CCCC_GITHUB_REPOSITORY must use the owner/repository form");
    }
    let index_url = std::env::var("CCCC_RELEASE_INDEX_URL").ok();
    let url = release_index_url(&repository, index_url.as_deref())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    fetch_channel_version(&client, &url, &repository, channel).await
}

fn release_index_url(repository: &str, configured: Option<&str>) -> Result<reqwest::Url> {
    let url = match configured {
        Some(value) => value,
        None if repository.eq_ignore_ascii_case("ChesterRa/cccc") => RELEASE_INDEX_URL,
        None => bail!(
            "set CCCC_RELEASE_INDEX_URL to the fork's HTTPS release index when using CCCC_GITHUB_REPOSITORY"
        ),
    };
    let url = reqwest::Url::parse(url).context("invalid CCCC_RELEASE_INDEX_URL")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        bail!("CCCC_RELEASE_INDEX_URL must be HTTPS without embedded credentials or a fragment");
    }
    Ok(url)
}

async fn fetch_channel_version(
    client: &reqwest::Client,
    url: &reqwest::Url,
    repository: &str,
    channel: ReleaseChannelArg,
) -> Result<String> {
    let mut response = client
        .get(url.clone())
        .header(reqwest::header::USER_AGENT, "cccc-standalone-updater")
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .context(
            "could not fetch the published CCCC release index; the installation was not changed",
        )?
        .error_for_status()
        .context(
            "the CCCC release index is unavailable; retry after Pages publication completes",
        )?;
    if !response.status().is_success() {
        bail!("the CCCC release index returned an unexpected redirect");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("could not read the CCCC release index")?
    {
        if chunk.len() > RELEASE_INDEX_MAX_BYTES - bytes.len() {
            bail!("the CCCC release index exceeds {RELEASE_INDEX_MAX_BYTES} bytes");
        }
        bytes.extend_from_slice(&chunk);
    }
    let index: serde_json::Value =
        serde_json::from_slice(&bytes).context("invalid CCCC release index JSON")?;
    index_channel_version(&index, repository, channel)
}

fn index_channel_version(
    index: &serde_json::Value,
    repository: &str,
    channel: ReleaseChannelArg,
) -> Result<String> {
    if index["schema_version"].as_u64() != Some(1) {
        bail!("unsupported CCCC release index schema");
    }
    if !index["repository"]
        .as_str()
        .is_some_and(|value| value.eq_ignore_ascii_case(repository))
    {
        bail!("the CCCC release index belongs to a different repository");
    }
    let version = index["channels"][channel_name(channel)]
        .as_str()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no complete CCCC {} release is published in the index",
                channel_name(channel)
            )
        })?;
    let parsed = release_version(version)?;
    if parsed.pre.is_empty() != (channel == ReleaseChannelArg::Stable) {
        bail!("the CCCC release index mixes stable and prerelease channels");
    }
    Ok(version.to_owned())
}

fn release_version(value: &str) -> Result<semver::Version> {
    let mut version = semver::Version::parse(value).context("invalid CCCC release version")?;
    for phase in ["alpha", "beta", "rc"] {
        if let Some(number) = version.pre.as_str().strip_prefix(phase)
            && !number.is_empty()
            && number.bytes().all(|byte| byte.is_ascii_digit())
            && (number == "0" || !number.starts_with('0'))
        {
            version.pre = semver::Prerelease::new(&format!("{phase}.{number}"))?;
            break;
        }
    }
    Ok(version)
}

fn should_install(
    current: &str,
    target: &str,
    requested_channel: Option<ReleaseChannelArg>,
) -> Result<bool> {
    let current = release_version(current)?;
    let target = release_version(target)?;
    let explicit_switch = requested_channel
        .is_some_and(|channel| (channel == ReleaseChannelArg::Stable) != current.pre.is_empty());
    match target.cmp_precedence(&current) {
        std::cmp::Ordering::Less if !explicit_switch => bail!(
            "published version {target} is older than installed {current}; refusing a downgrade (the release index may be stale)"
        ),
        std::cmp::Ordering::Equal => Ok(false),
        _ => Ok(true),
    }
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
        assert!(release_version("0.4.34-rc2").is_ok());
        assert!(release_version("1.2.3").is_ok());
        assert!(release_version("latest").is_err());
        assert!(release_version("1.2.3/../../escape").is_err());
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
        let index = serde_json::json!({
            "schema_version":1,"repository":"ChesterRa/cccc",
            "channels":{"stable":"1.2.3","rc":"1.3.0-rc2"}
        });
        assert_eq!(
            index_channel_version(&index, "ChesterRa/cccc", ReleaseChannelArg::Stable)
                .expect("stable"),
            "1.2.3"
        );
        assert_eq!(
            index_channel_version(&index, "ChesterRa/cccc", ReleaseChannelArg::Rc).expect("rc"),
            "1.3.0-rc2"
        );
    }
}
