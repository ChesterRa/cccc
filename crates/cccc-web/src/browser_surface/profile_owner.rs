#[cfg(unix)]
use anyhow::bail;
use anyhow::{Context, Result};
use chromiumoxide::browser::Browser;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const OWNER_FILE: &str = ".cccc-browser-owner.lock";
/// Version 2 records the browser's native process identity (boot and start)
/// instead of the start time `ps` printed.
const OWNER_VERSION: u32 = 2;

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

    /// Records the browser this profile's next acquire may verify and stop.
    /// An identity that cannot be read is an error, never an empty record.
    pub(super) async fn record_pid(&mut self, browser_pid: u32) -> Result<()> {
        #[cfg(unix)]
        let process_start = process_identity(browser_pid)?.with_context(|| {
            format!("browser process {browser_pid} ended before it was recorded")
        })?;
        #[cfg(not(unix))]
        let process_start = String::new();
        self.write(browser_pid, process_start)
    }

    fn write(&mut self, browser_pid: u32, process_start: String) -> Result<()> {
        let owner = OwnerMetadata {
            version: OWNER_VERSION,
            browser_pid,
            process_start,
            profile: self.profile.to_string_lossy().into_owned(),
        };
        write_owner(&mut self.file, &owner)
    }

    /// After a launch failed, records the browser macOS opened for this
    /// profile while it still runs, so the next acquire verifies and stops it.
    /// Only a confirmed end records nothing; a browser that cannot be located
    /// or identified is an error.
    #[cfg(any(target_os = "macos", all(test, unix)))]
    pub(super) async fn record_unstopped_browser(
        &mut self,
        process: Option<&(u32, String)>,
    ) -> Result<()> {
        let found = match process {
            Some((pid, identity)) => Some((*pid, identity.clone())),
            None => locate_opened_browser(&self.profile).await?,
        };
        let Some((pid, identity)) = found else {
            return Ok(());
        };
        match process_identity(pid)? {
            Some(current) if current == identity => self.write(pid, identity),
            _ => Ok(()),
        }
    }

    pub(super) fn clear_owner(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_data()?;
        Ok(())
    }
}

#[cfg(any(target_os = "macos", all(test, unix)))]
pub(super) fn browser_pid_from_singleton(profile: &Path) -> Result<u32> {
    let target = std::fs::read_link(profile.join("SingletonLock"))?;
    let target = target.to_string_lossy();
    parse_singleton_target(&target)
        .map(|(_, pid)| pid)
        .with_context(|| format!("browser profile contains a malformed SingletonLock: {target}"))
}

/// The browser process the profile's SingletonLock names and its identity,
/// or `None` once that process has ended. A missing lock or a process that
/// does not use this profile leaves the opened browser unknown.
#[cfg(any(target_os = "macos", all(test, unix)))]
async fn locate_opened_browser(profile: &Path) -> Result<Option<(u32, String)>> {
    let pid = browser_pid_from_singleton(profile)
        .context("the browser opened for this profile cannot be located")?;
    let Some(snapshot) = process_snapshot(pid).await? else {
        return Ok(None);
    };
    if !snapshot
        .command
        .contains(profile.to_string_lossy().as_ref())
    {
        bail!("process {pid} named by the profile lock does not use this profile");
    }
    Ok(Some((pid, snapshot.start)))
}

/// Stops the browser macOS opened for `profile` and returns once its native
/// identity confirms it ended. Without a recorded process, the profile's
/// SingletonLock names it.
#[cfg(any(target_os = "macos", all(test, unix)))]
pub(super) async fn stop_opened_browser(
    profile: &Path,
    process: Option<&(u32, String)>,
) -> Result<()> {
    let found = match process {
        Some((pid, identity)) => Some((*pid, identity.clone())),
        None => locate_opened_browser(profile).await?,
    };
    if let Some((pid, identity)) = found {
        terminate_process(pid, &identity).await?;
    }
    Ok(())
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
    let output = cccc_runtime::background_tokio_command("hostname")
        .output()
        .await?;
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

/// The process's native identity, its boot and start, or `None` once it has
/// exited. Anything else that stops the read leaves the process unknown.
#[cfg(unix)]
pub(super) fn process_identity(pid: u32) -> Result<Option<String>> {
    // Kernels issue PIDs below 2^31, so a larger one names no process.
    if i32::try_from(pid).is_err() {
        return Ok(None);
    }
    match cccc_core::local_sessions::NativeHost::capture(pid) {
        Ok(host) => Ok(Some(format!("{}:{}", host.boot, host.started))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read the identity of process {pid}")),
    }
}

/// `None` only when the process has exited; `ps` supplies just its command
/// line, and a failed `ps` leaves a running process unknown, not gone.
#[cfg(unix)]
async fn process_snapshot(pid: u32) -> Result<Option<ProcessSnapshot>> {
    let Some(start) = process_identity(pid)? else {
        return Ok(None);
    };
    let process = cccc_runtime::background_tokio_command("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .await
        .with_context(|| format!("read the command line of process {pid}"))?;
    // The command line counts only if it belongs to the identity read first.
    match process_identity(pid)? {
        None => return Ok(None),
        Some(now) if now != start => bail!("process {pid} changed while it was read"),
        Some(_) => {}
    }
    if !process.status.success() || process.stdout.is_empty() {
        bail!("could not read the command line of process {pid}");
    }
    let command = String::from_utf8_lossy(&process.stdout).trim().to_owned();
    Ok(Some(ProcessSnapshot { command, start }))
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
    let status = cccc_runtime::background_tokio_command("kill")
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
        std::fs::remove_file(&stale_socket).expect("remove stale socket endpoint");
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

    /// Starts a stand-in browser whose command line names `profile` (an
    /// absolute, canonical path) and returns it with its pid and identity
    /// once `ps` shows that command for the identity.
    async fn stand_in_browser(profile: &Path) -> (tokio::process::Child, u32, String) {
        let script = profile.join("stand-in-browser.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\ntrap 'kill \"$child\" 2>/dev/null; exit 0' TERM\nsleep 30 &\nchild=$!\nwait \"$child\"\n",
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("script permissions");
        let child = tokio::process::Command::new(&script)
            .spawn()
            .expect("stand-in browser");
        let pid = child.id().expect("stand-in pid");
        let start = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(snapshot) = process_snapshot(pid).await.expect("process snapshot")
                    && snapshot
                        .command
                        .contains(profile.to_string_lossy().as_ref())
                {
                    break snapshot.start;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("stand-in command timeout");
        (child, pid, start)
    }

    async fn assert_verified_owner_recovered(profile: &Path, owner_host: &str) {
        std::fs::create_dir_all(profile).expect("profile");
        let canonical_profile = std::fs::canonicalize(profile).expect("canonical profile");
        let (mut child, pid, start) = stand_in_browser(&canonical_profile).await;
        symlink(format!("{owner_host}-{pid}"), profile.join("SingletonLock"))
            .expect("singleton lock");
        let owner = OwnerMetadata {
            version: OWNER_VERSION,
            browser_pid: pid,
            process_start: start,
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

    /// A browser macOS opened before a launch failed is recorded from the
    /// profile lock, and the next acquire verifies and stops it.
    #[tokio::test]
    async fn an_unstopped_opened_browser_is_recorded_for_the_next_acquire() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = std::fs::canonicalize(temp.path())
            .expect("canonical temp")
            .join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        let mut lease = ProfileLease::acquire(&profile).await.expect("fresh lease");
        let (mut child, pid, start) = stand_in_browser(&profile).await;
        let host = hostname().await.expect("hostname");
        symlink(format!("{host}-{pid}"), profile.join("SingletonLock")).expect("singleton lock");

        lease
            .record_unstopped_browser(None)
            .await
            .expect("record the located browser");
        let recorded = read_owner(&mut lease.file).expect("recorded owner");
        assert_eq!(recorded.browser_pid, pid);
        assert_eq!(recorded.process_start, start);
        assert_eq!(recorded.profile, profile.to_string_lossy());
        drop(lease);

        let _next = ProfileLease::acquire(&profile)
            .await
            .expect("the next acquire stops the recorded browser");
        tokio::time::timeout(std::time::Duration::from_secs(2), child.wait())
            .await
            .expect("stand-in exit timeout")
            .expect("stand-in exit");
        assert!(std::fs::symlink_metadata(profile.join("SingletonLock")).is_err());
    }

    /// Without a recorded process the profile lock names the opened browser.
    /// A missing lock, or a lock naming a process that does not use the
    /// profile, is an error and signals nothing; the recorded identity stops
    /// the browser, and stopping one that already ended needs nothing.
    #[tokio::test]
    async fn an_opened_browser_is_stopped_only_by_its_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = std::fs::canonicalize(temp.path())
            .expect("canonical temp")
            .join("profile");
        std::fs::create_dir_all(&profile).expect("profile");
        let (mut child, pid, start) = stand_in_browser(&profile).await;
        let lock = profile.join("SingletonLock");
        let host = hostname().await.expect("hostname");

        let missing = stop_opened_browser(&profile, None)
            .await
            .expect_err("a missing lock leaves the browser unknown");
        assert!(
            missing.to_string().contains("cannot be located"),
            "{missing:#}"
        );

        let mut foreign = tokio::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("foreign process");
        let foreign_pid = foreign.id().expect("foreign pid");
        symlink(format!("{host}-{foreign_pid}"), &lock).expect("foreign lock");
        let error = stop_opened_browser(&profile, None)
            .await
            .expect_err("a process outside the profile is not stopped");
        assert!(
            error.to_string().contains("does not use this profile"),
            "{error:#}"
        );
        assert!(foreign.try_wait().expect("foreign state").is_none());
        foreign.kill().await.expect("stop the foreign process");
        assert!(child.try_wait().expect("stand-in state").is_none());

        std::fs::remove_file(&lock).expect("remove the foreign lock");
        symlink(format!("{host}-{pid}"), &lock).expect("singleton lock");
        let opened = (pid, start);
        assert_eq!(
            locate_opened_browser(&profile).await.expect("locate"),
            Some(opened.clone())
        );
        stop_opened_browser(&profile, Some(&opened))
            .await
            .expect("stop by identity");
        tokio::time::timeout(std::time::Duration::from_secs(2), child.wait())
            .await
            .expect("stand-in exit timeout")
            .expect("stand-in exit");
        stop_opened_browser(&profile, Some(&opened))
            .await
            .expect("an ended browser needs nothing");
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
