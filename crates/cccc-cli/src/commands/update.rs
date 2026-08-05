use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::args::UpdateArgs;

const CARGO: &str = "cargo";
const PACKAGE: &str = "cccc";
const UPDATE_ARGS: &[&str] = &["install", PACKAGE, "--force", "--locked"];
const SEARCH_ARGS: &[&str] = &["search", PACKAGE, "--registry", "crates-io", "--limit", "1"];

#[derive(Debug, PartialEq, Eq)]
struct UpdatePlan {
    program: &'static str,
    args: &'static [&'static str],
}

impl UpdatePlan {
    fn crates_io() -> Self {
        Self {
            program: CARGO,
            args: UPDATE_ARGS,
        }
    }

    fn display(&self) -> String {
        std::iter::once(self.program)
            .chain(self.args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Update the Rust distribution. Returns `true` when a replacement was installed.
pub fn run(args: UpdateArgs, product_version: &str, crate_version: &str) -> Result<bool> {
    let executable = std::env::current_exe().context("could not locate the running CCCC binary")?;
    let cargo_install = is_cargo_install(&executable);
    let latest = latest_crate_version();
    println!("Current CCCC product version: {product_version}");
    println!("Current crates.io package version: {crate_version}");
    match &latest {
        Ok(version) => println!("Latest crates.io package version: {version}"),
        Err(error) => eprintln!("Could not check the latest crates.io version: {error}"),
    }
    println!(
        "Update source: {}",
        if cargo_install {
            "crates.io"
        } else {
            "GitHub Rust release"
        }
    );
    if cargo_install {
        println!("Command: {}", UpdatePlan::crates_io().display());
    } else {
        println!("Installer: signed-by-tag release assets with SHA256SUMS verification");
    }

    if args.check {
        return Ok(false);
    }
    let latest = latest.context("could not resolve the Rust release version")?;
    match compare_versions(&latest, crate_version)? {
        Ordering::Less => {
            println!("CCCC Rust is newer than the latest crates.io release; no update applied.");
            return Ok(false);
        }
        Ordering::Equal => {
            println!("CCCC Rust is already up to date.");
            return Ok(false);
        }
        Ordering::Greater => {}
    }

    if cargo_install {
        install_with_cargo()?;
    } else {
        install_release(&executable, &latest)?;
    }

    println!("CCCC Rust update installed successfully.");
    Ok(true)
}

fn compare_versions(latest: &str, current: &str) -> Result<Ordering> {
    let latest = semver::Version::parse(latest)
        .with_context(|| format!("invalid crates.io version `{latest}`"))?;
    let current = semver::Version::parse(current)
        .with_context(|| format!("invalid installed crate version `{current}`"))?;
    Ok(latest.cmp(&current))
}

fn install_with_cargo() -> Result<()> {
    let plan = UpdatePlan::crates_io();
    let status = Command::new(plan.program)
        .args(plan.args)
        .status()
        .with_context(|| "could not run Cargo; install Rust/Cargo and retry `cccc update`")?;
    if !status.success() {
        bail!("Cargo update failed with {status}");
    }
    Ok(())
}

fn install_release(executable: &Path, version: &str) -> Result<()> {
    let install_dir = executable
        .parent()
        .filter(|path| path.is_absolute())
        .context("CCCC executable has no absolute parent directory")?;
    let extension = if cfg!(windows) { "ps1" } else { "sh" };
    let url = format!(
        "https://raw.githubusercontent.com/ChesterRa/cccc/rust-v{version}/scripts/install.{extension}"
    );
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .get(&url)
        .header(reqwest::header::USER_AGENT, "cccc-rust-updater")
        .send()
        .with_context(|| format!("could not download the tagged Rust installer: {url}"))?;
    if !response.status().is_success() {
        bail!("tagged Rust installer returned HTTP {}", response.status());
    }
    let script = response
        .text()
        .context("could not read the Rust installer")?;
    let script_path = temporary_script(extension);
    std::fs::write(&script_path, script).context("could not stage the Rust installer")?;
    let mut command = if cfg!(windows) {
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        command.arg(&script_path);
        command
    } else {
        let mut command = Command::new("sh");
        command.arg(&script_path);
        command
    };
    let status = command
        .env("CCCC_VERSION", version)
        .env("CCCC_RELEASE_TAG_PREFIX", "rust-v")
        .env("CCCC_INSTALL_DIR", install_dir)
        .env("CCCC_NO_MODIFY_PATH", "1")
        .status()
        .context("could not run the tagged Rust installer")?;
    let _ = std::fs::remove_file(&script_path);
    if !status.success() {
        bail!("GitHub Rust release update failed with {status}");
    }
    Ok(())
}

fn temporary_script(extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cccc-update-{}-{}.{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("main"),
        extension
    ))
}

fn is_cargo_install(executable: &Path) -> bool {
    executable
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "bin")
        && executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".cargo")
}

fn latest_crate_version() -> Result<String> {
    let output = Command::new(CARGO)
        .args(SEARCH_ARGS)
        .output()
        .with_context(|| "could not run Cargo to check crates.io")?;
    if !output.status.success() {
        bail!(
            "cargo search failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_search_version(&String::from_utf8_lossy(&output.stdout))
        .context("cargo search did not return the cccc package")
}

fn parse_search_version(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let version = line.strip_prefix("cccc = \"")?.split('"').next()?;
        (!version.trim().is_empty()).then(|| version.trim().to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crates_io_plan_reinstalls_the_public_crate_from_its_lockfile() {
        let plan = UpdatePlan::crates_io();
        assert_eq!(plan.program, "cargo");
        assert_eq!(plan.args, ["install", "cccc", "--force", "--locked"]);
        assert_eq!(plan.display(), "cargo install cccc --force --locked");
    }

    #[test]
    fn parses_exact_crate_search_result() {
        assert_eq!(
            parse_search_version("cccc = \"0.4.33\" # package"),
            Some("0.4.33".into())
        );
        assert_eq!(parse_search_version("other = \"1.0.0\""), None);
    }

    #[test]
    fn detects_cargo_and_release_install_locations() {
        assert!(is_cargo_install(Path::new("/tmp/user/.cargo/bin/cccc")));
        assert!(!is_cargo_install(Path::new("/tmp/user/.local/bin/cccc")));
    }

    #[test]
    fn update_comparison_never_downgrades() {
        assert_eq!(
            compare_versions("0.0.5", "0.4.33").expect("valid versions"),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("0.4.33", "0.4.33").expect("valid versions"),
            Ordering::Equal
        );
        assert_eq!(
            compare_versions("0.4.34", "0.4.33").expect("valid versions"),
            Ordering::Greater
        );
        assert!(compare_versions("latest", "0.4.33").is_err());
    }
}
