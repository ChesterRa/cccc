use anyhow::{Context, Result, bail};
use chromiumoxide::browser::Browser;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const OWNER_FILE: &str = ".cccc-browser-owner.lock";
const OWNER_VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize)]
struct OwnerMetadata {
    version: u32,
    browser_pid: u32,
    process_start: String,
    profile: String,
}

#[derive(Debug)]
pub(super) struct ProfileLease {
    file: File,
    profile: PathBuf,
}

impl ProfileLease {
    pub(super) async fn acquire(profile: &Path) -> Result<Self> {
        let profile = std::fs::canonicalize(profile)
            .with_context(|| format!("canonicalize browser profile {}", profile.display()))?;
        let path = profile.join(OWNER_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open browser profile owner file {}", path.display()))?;
        FileExt::try_lock_exclusive(&file).with_context(|| {
            format!(
                "browser profile is already managed by another CCCC Web process: {}",
                profile.display()
            )
        })?;
        let previous_owner = read_owner(&mut file);
        recover_chromium_profile(&profile, previous_owner.as_ref()).await?;
        Ok(Self { file, profile })
    }

    pub(super) async fn record_browser(&mut self, browser: &mut Browser) -> Result<()> {
        let browser_pid = browser
            .get_mut_child()
            .and_then(|child| child.as_mut_inner().id())
            .context("launched Chromium process has no PID")?;
        self.record_pid(browser_pid).await
    }

    pub(super) async fn record_pid(&mut self, browser_pid: u32) -> Result<()> {
        let process_start = process_start(browser_pid).await.unwrap_or_default();
        let owner = OwnerMetadata {
            version: OWNER_VERSION,
            browser_pid,
            process_start,
            profile: self.profile.to_string_lossy().into_owned(),
        };
        write_owner(&mut self.file, &owner)
    }

    pub(super) fn clear_owner(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_data()?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub(super) fn browser_pid_from_singleton(profile: &Path) -> Result<u32> {
    let target = std::fs::read_link(profile.join("SingletonLock"))?;
    let target = target.to_string_lossy();
    parse_singleton_target(&target)
        .map(|(_, pid)| pid)
        .with_context(|| format!("browser profile contains a malformed SingletonLock: {target}"))
}

#[cfg(target_os = "macos")]
pub(super) async fn terminate_browser_for_profile(profile: &Path) -> Result<bool> {
    let Ok(pid) = browser_pid_from_singleton(profile) else {
        return Ok(false);
    };
    let Some(snapshot) = process_snapshot(pid).await? else {
        return Ok(false);
    };
    if !snapshot
        .command
        .contains(profile.to_string_lossy().as_ref())
    {
        bail!("refusing to terminate Chromium process {pid}: profile does not match");
    }
    terminate_process(pid, &snapshot.start).await?;
    Ok(true)
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn read_owner(file: &mut File) -> Option<OwnerMetadata> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    (!bytes.is_empty())
        .then(|| serde_json::from_slice(&bytes).ok())
        .flatten()
}

fn write_owner(file: &mut File, owner: &OwnerMetadata) -> Result<()> {
    let mut bytes = serde_json::to_vec(owner)?;
    bytes.push(b'\n');
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.sync_data()?;
    Ok(())
}

#[cfg(unix)]
async fn recover_chromium_profile(
    profile: &Path,
    previous_owner: Option<&OwnerMetadata>,
) -> Result<()> {
    let singleton_lock = profile.join("SingletonLock");
    let Ok(metadata) = std::fs::symlink_metadata(&singleton_lock) else {
        return Ok(());
    };
    if !metadata.file_type().is_symlink() {
        bail!(
            "browser profile contains an unrecognized SingletonLock: {}",
            singleton_lock.display()
        );
    }
    let target = std::fs::read_link(&singleton_lock)?;
    let target = target.to_string_lossy();
    let (owner_host, owner_pid) = parse_singleton_target(&target).with_context(|| {
        format!("browser profile contains a malformed SingletonLock target: {target}")
    })?;
    let local_host = hostname().await?;
    let snapshot = process_snapshot(owner_pid).await?;
    if let Some(snapshot) = snapshot.as_ref() {
        if !owner_matches(previous_owner, profile, owner_pid, snapshot) {
            if owner_host != local_host {
                bail!("browser profile is locked by host {owner_host}");
            }
            bail!(
                "browser profile is owned by a live or unverifiable Chromium process (pid {owner_pid})"
            );
        }
        terminate_process(owner_pid, &snapshot.start).await?;
    } else if owner_host != local_host && !has_stale_local_singleton_socket(profile)? {
        bail!("browser profile is locked by host {owner_host}");
    }

    remove_singleton_artifacts(profile)
}

#[cfg(unix)]
fn owner_matches(
    previous_owner: Option<&OwnerMetadata>,
    profile: &Path,
    owner_pid: u32,
    snapshot: &ProcessSnapshot,
) -> bool {
    previous_owner.is_some_and(|owner| {
        owner.version == OWNER_VERSION
            && owner.browser_pid == owner_pid
            && owner.profile == profile.to_string_lossy()
            && !owner.process_start.is_empty()
            && owner.process_start == snapshot.start
            && snapshot
                .command
                .contains(profile.to_string_lossy().as_ref())
    })
}

#[cfg(unix)]
fn has_stale_local_singleton_socket(profile: &Path) -> Result<bool> {
    let singleton_socket = profile.join("SingletonSocket");
    let Ok(metadata) = std::fs::symlink_metadata(&singleton_socket) else {
        return Ok(false);
    };
    if !metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let target = std::fs::read_link(singleton_socket)?;
    if !target.is_absolute() || !target.starts_with(std::env::temp_dir()) {
        return Ok(false);
    }
    match std::os::unix::net::UnixStream::connect(target) {
        Ok(_) => Ok(false),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

#[cfg(not(unix))]
async fn recover_chromium_profile(
    _profile: &Path,
    _previous_owner: Option<&OwnerMetadata>,
) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn parse_singleton_target(target: &str) -> Option<(&str, u32)> {
    let (host, pid) = target.rsplit_once('-')?;
    (!host.is_empty()).then_some((host, pid.parse::<u32>().ok()?))
}

#[cfg(unix)]
async fn hostname() -> Result<String> {
    let output = tokio::process::Command::new("hostname").output().await?;
    if !output.status.success() {
        bail!("failed to resolve local hostname");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(unix)]
struct ProcessSnapshot {
    command: String,
    start: String,
}

#[cfg(unix)]
async fn process_snapshot(pid: u32) -> Result<Option<ProcessSnapshot>> {
    let process = tokio::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat=", "-o", "command="])
        .output()
        .await?;
    if !process.status.success() || process.stdout.is_empty() {
        return Ok(None);
    }
    let output = String::from_utf8_lossy(&process.stdout);
    let mut fields = output.trim().splitn(2, char::is_whitespace);
    let state = fields.next().unwrap_or_default();
    if state.starts_with('Z') {
        return Ok(None);
    }
    let command = fields.next().unwrap_or_default().trim().to_owned();
    let start = process_start(pid).await.unwrap_or_default();
    Ok(Some(ProcessSnapshot { command, start }))
}

#[cfg(unix)]
async fn process_start(pid: u32) -> Result<String> {
    let output = tokio::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .await?;
    if !output.status.success() {
        bail!("process {pid} is not running");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(not(unix))]
async fn process_start(_pid: u32) -> Result<String> {
    Ok(String::new())
}

#[cfg(unix)]
async fn terminate_process(pid: u32, expected_start: &str) -> Result<()> {
    if !signal_if_same_process(pid, expected_start, "-TERM").await? {
        return Ok(());
    }
    for _ in 0..40 {
        if !is_same_process(pid, expected_start).await? {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    if !signal_if_same_process(pid, expected_start, "-KILL").await? {
        return Ok(());
    }
    for _ in 0..40 {
        if !is_same_process(pid, expected_start).await? {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    bail!("orphaned Chromium process {pid} did not exit")
}

#[cfg(unix)]
async fn signal_if_same_process(pid: u32, expected_start: &str, signal: &str) -> Result<bool> {
    if !is_same_process(pid, expected_start).await? {
        return Ok(false);
    }
    let status = tokio::process::Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .await?;
    if !status.success() {
        if !is_same_process(pid, expected_start).await? {
            return Ok(false);
        }
        bail!("failed to send {signal} to orphaned Chromium process {pid}");
    }
    Ok(true)
}

#[cfg(unix)]
async fn is_same_process(pid: u32, expected_start: &str) -> Result<bool> {
    Ok(process_snapshot(pid)
        .await?
        .is_some_and(|snapshot| snapshot.start == expected_start))
}

#[cfg(unix)]
fn remove_singleton_artifacts(profile: &Path) -> Result<()> {
    for name in [
        "SingletonLock",
        "SingletonCookie",
        "SingletonSocket",
        "DevToolsActivePort",
    ] {
        let path = profile.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("remove stale browser artifact {}", path.display()));
            }
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;

    #[tokio::test]
    async fn removes_artifacts_for_dead_local_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        let host = hostname().await.expect("hostname");
        symlink(format!("{host}-4294967295"), profile.join("SingletonLock"))
            .expect("singleton lock");
        symlink("cookie", profile.join("SingletonCookie")).expect("singleton cookie");
        std::fs::write(profile.join("DevToolsActivePort"), "1234").expect("devtools port");

        let _lease = ProfileLease::acquire(&profile)
            .await
            .expect("profile lease");

        assert!(std::fs::symlink_metadata(profile.join("SingletonLock")).is_err());
        assert!(std::fs::symlink_metadata(profile.join("SingletonCookie")).is_err());
        assert!(std::fs::symlink_metadata(profile.join("DevToolsActivePort")).is_err());
    }

    #[tokio::test]
    async fn refuses_to_remove_lock_for_unverified_live_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        let host = hostname().await.expect("hostname");
        symlink(
            format!("{host}-{}", std::process::id()),
            profile.join("SingletonLock"),
        )
        .expect("singleton lock");

        let error = ProfileLease::acquire(&profile)
            .await
            .expect_err("live owner must be rejected");

        assert!(error.to_string().contains("live or unverifiable"));
        assert!(std::fs::symlink_metadata(profile.join("SingletonLock")).is_ok());
    }

    #[tokio::test]
    async fn prevents_two_managers_from_owning_one_profile() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        let _first = ProfileLease::acquire(&profile).await.expect("first lease");

        let error = ProfileLease::acquire(&profile)
            .await
            .expect_err("second lease must fail");

        assert!(error.to_string().contains("already managed"));
    }

    #[tokio::test]
    async fn refuses_foreign_host_lock_without_removing_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        symlink("another-host-4294967295", profile.join("SingletonLock")).expect("singleton lock");

        let error = ProfileLease::acquire(&profile)
            .await
            .expect_err("foreign lock must fail");

        assert!(error.to_string().contains("locked by host"));
        assert!(std::fs::symlink_metadata(profile.join("SingletonLock")).is_ok());
    }

    #[tokio::test]
    async fn removes_dead_local_lock_after_hostname_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        symlink(
            "previous-local-host-4294967295",
            profile.join("SingletonLock"),
        )
        .expect("singleton lock");
        let socket_id = uuid::Uuid::new_v4().simple().to_string();
        let stale_socket = std::env::temp_dir().join(format!(".cccc-s-{}", &socket_id[..12]));
        let listener =
            std::os::unix::net::UnixListener::bind(&stale_socket).expect("singleton listener");
        drop(listener);
        symlink(&stale_socket, profile.join("SingletonSocket")).expect("singleton socket");

        let _lease = ProfileLease::acquire(&profile)
            .await
            .expect("profile lease");

        assert!(std::fs::symlink_metadata(profile.join("SingletonLock")).is_err());
        assert!(std::fs::symlink_metadata(profile.join("SingletonSocket")).is_err());
    }

    #[tokio::test]
    async fn refuses_foreign_host_lock_with_live_local_socket() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        symlink("another-host-4294967295", profile.join("SingletonLock")).expect("singleton lock");
        let socket_id = uuid::Uuid::new_v4().simple().to_string();
        let live_socket = std::env::temp_dir().join(format!(".cccc-l-{}", &socket_id[..12]));
        let _listener =
            std::os::unix::net::UnixListener::bind(&live_socket).expect("singleton listener");
        symlink(&live_socket, profile.join("SingletonSocket")).expect("singleton socket");

        let error = ProfileLease::acquire(&profile)
            .await
            .expect_err("live foreign lock must fail");

        assert!(error.to_string().contains("locked by host"));
        assert!(std::fs::symlink_metadata(profile.join("SingletonLock")).is_ok());
    }

    #[tokio::test]
    async fn refuses_malformed_lock_without_removing_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        symlink("not-a-valid-owner", profile.join("SingletonLock")).expect("singleton lock");

        let error = ProfileLease::acquire(&profile)
            .await
            .expect_err("malformed lock must fail");

        assert!(error.to_string().contains("malformed"));
        assert!(std::fs::symlink_metadata(profile.join("SingletonLock")).is_ok());
    }

    #[tokio::test]
    async fn verified_live_owner_is_terminated_before_recovery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        let host = hostname().await.expect("hostname");
        assert_verified_owner_recovered(&profile, &host).await;
    }

    #[tokio::test]
    async fn verified_live_owner_is_recovered_after_hostname_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        assert_verified_owner_recovered(&profile, "previous-local-host").await;
    }

    async fn assert_verified_owner_recovered(profile: &Path, owner_host: &str) {
        std::fs::create_dir_all(profile).expect("profile");
        let script = profile.join("orphan-browser.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\ntrap 'kill \"$child\" 2>/dev/null; exit 0' TERM\nsleep 30 &\nchild=$!\nwait \"$child\"\n",
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("script permissions");
        let canonical_profile = std::fs::canonicalize(profile).expect("canonical profile");
        let mut child = tokio::process::Command::new(canonical_profile.join("orphan-browser.sh"))
            .spawn()
            .expect("orphan process");
        let pid = child.id().expect("orphan pid");
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(snapshot) = process_snapshot(pid).await.expect("process snapshot") {
                    if snapshot
                        .command
                        .contains(canonical_profile.to_string_lossy().as_ref())
                    {
                        break snapshot;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("orphan command timeout");
        symlink(format!("{owner_host}-{pid}"), profile.join("SingletonLock"))
            .expect("singleton lock");
        let owner = OwnerMetadata {
            version: OWNER_VERSION,
            browser_pid: pid,
            process_start: snapshot.start,
            profile: canonical_profile.to_string_lossy().into_owned(),
        };
        let mut owner_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(profile.join(OWNER_FILE))
            .expect("owner file");
        write_owner(&mut owner_file, &owner).expect("owner metadata");

        let _lease = ProfileLease::acquire(profile)
            .await
            .expect("recover verified owner");

        tokio::time::timeout(std::time::Duration::from_secs(2), child.wait())
            .await
            .expect("owner exit timeout")
            .expect("owner exit");
        assert!(std::fs::symlink_metadata(profile.join("SingletonLock")).is_err());
    }

    #[tokio::test]
    async fn changed_process_identity_is_never_signaled() {
        let mut child = tokio::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("process");
        let pid = child.id().expect("pid");

        terminate_process(pid, "different process identity")
            .await
            .expect("identity mismatch is not an error");

        assert!(child.try_wait().expect("try wait").is_none());
        child.kill().await.expect("cleanup process");
    }
}
