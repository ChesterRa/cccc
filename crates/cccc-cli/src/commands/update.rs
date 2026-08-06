use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::args::UpdateArgs;

#[cfg(not(windows))]
const UNIX_INSTALLER_URL: &str = "https://chesterra.github.io/cccc/install.sh";
#[cfg(windows)]
const WINDOWS_INSTALLER_URL: &str = "https://chesterra.github.io/cccc/install.ps1";
const INSTALL_MARKER: &str = ".cccc-standalone";
const INSTALL_MARKER_VERSION: &str = "standalone-v1";

pub fn run(args: UpdateArgs) -> Result<()> {
    let executable = std::env::current_exe().context("could not resolve the CCCC executable")?;
    let install_dir = standalone_install_dir(&executable)?;

    if args.check {
        println!("Current version: {}", crate::PRODUCT_VERSION);
        println!("Install directory: {}", install_dir.display());
        println!("Installer: {}", installer_url());
        return Ok(());
    }

    run_installer(&install_dir)
}

fn standalone_install_dir(executable: &Path) -> Result<PathBuf> {
    let install_dir = executable
        .parent()
        .context("CCCC executable has no parent directory")?;
    let owned_by_standalone_installer = std::fs::read_to_string(install_dir.join(INSTALL_MARKER))
        .is_ok_and(|value| value.trim() == INSTALL_MARKER_VERSION);
    if !owned_by_standalone_installer {
        bail!(
            "this Rust executable is managed by another installation; update it through that installer"
        );
    }
    Ok(install_dir.to_path_buf())
}

#[cfg(not(windows))]
fn run_installer(install_dir: &Path) -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg("curl -fsSL \"$CCCC_INSTALLER_URL\" | sh")
        .env("CCCC_INSTALLER_URL", UNIX_INSTALLER_URL)
        .env("CCCC_INSTALL_DIR", install_dir)
        .status()
        .context("could not start the CCCC installer")?;
    if !status.success() {
        bail!("CCCC installer exited with {status}");
    }
    Ok(())
}

#[cfg(windows)]
fn run_installer(install_dir: &Path) -> Result<()> {
    let child = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "Wait-Process -Id $env:CCCC_UPDATE_PARENT_PID -ErrorAction SilentlyContinue; Invoke-RestMethod -Uri $env:CCCC_INSTALLER_URL | Invoke-Expression",
        ])
        .env("CCCC_UPDATE_PARENT_PID", std::process::id().to_string())
        .env("CCCC_INSTALLER_URL", WINDOWS_INSTALLER_URL)
        .env("CCCC_INSTALL_DIR", install_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
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
    fn standalone_install_requires_the_installer_marker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp
            .path()
            .join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
        std::fs::write(&executable, b"binary").expect("binary");
        assert!(standalone_install_dir(&executable).is_err());

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
    }
}
