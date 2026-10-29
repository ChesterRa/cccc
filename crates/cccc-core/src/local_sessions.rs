//! Native integration metadata in CCCC_HOME. Written only from public runtime
//! lifecycle callbacks; it contains no transcript, credentials or native history.
use crate::{HomeLayout, fs};
use cccc_contracts::local_session::{LocalSessionRuntime, LocalSessionSource};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

/// Global native configuration can also be loaded by CCCC-owned Actors and
/// assistants. It must not replace their existing identity or receive path.
pub fn cccc_owned_environment() -> bool {
    [
        "CCCC_ACTOR_ID",
        "CCCC_SECRETARY_TASK_TOKEN",
        "CCCC_SECRETARY_TASK_TOKEN_FILE",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
        || std::env::var("CCCC_MCP_TOOL_PROFILE").is_ok_and(|value| value == "secretary-task")
}

/// PowerShell treats ASCII and smart single quotes as string delimiters. Each
/// must be doubled even inside an ASCII-delimited literal; encoding the outer
/// script does not prevent an embedded path from becoming executable code.
pub fn powershell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if matches!(character, '\'' | '\u{2018}'..='\u{201b}') {
            quoted.push(character);
        }
        quoted.push(character);
    }
    quoted.push('\'');
    quoted
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeHost {
    pub pid: u32,
    pub started: String,
    /// Linux start ticks need a boot UUID. macOS/Windows use an absolute native
    /// creation timestamp and a platform namespace instead of uptime ticks.
    pub boot: String,
}

impl NativeHost {
    #[cfg(target_os = "linux")]
    pub fn capture(pid: u32) -> io::Result<Self> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let fields = stat
            .rsplit_once(") ")
            .ok_or_else(|| io::Error::other("Invalid native host identity"))?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields
            .first()
            .is_some_and(|state| matches!(*state, "Z" | "X"))
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Native host has exited",
            ));
        }
        let started = fields
            .get(19)
            .ok_or_else(|| io::Error::other("Missing native host start time"))?
            .to_string();
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .to_owned();
        Ok(Self { pid, started, boot })
    }

    #[cfg(target_os = "macos")]
    pub fn capture(pid: u32) -> io::Result<Self> {
        let info = macos_process(pid)?;
        Ok(Self {
            pid,
            started: (u128::from(info.pbi_start_tvsec) * 1_000_000
                + u128::from(info.pbi_start_tvusec))
            .to_string(),
            boot: "macos-bsd-start".into(),
        })
    }

    #[cfg(windows)]
    pub fn capture(pid: u32) -> io::Result<Self> {
        Ok(Self {
            pid,
            started: cccc_windows_process::creation_time(pid)?.to_string(),
            boot: "windows-filetime".into(),
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub fn capture(_pid: u32) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Native session integration requires Linux, macOS or Windows",
        ))
    }

    /// Resolve an actual native parent, not a shell's emulated PPID. Recheck
    /// the child and reject a reused parent PID whose creation is newer.
    pub fn parent(pid: u32) -> io::Result<Self> {
        let child = Self::capture(pid)?;
        let parent_pid = native_parent_pid(pid)?;
        if parent_pid <= 1 || parent_pid == pid {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Native parent is unavailable",
            ));
        }
        let parent = Self::capture(parent_pid)?;
        if parent.boot != child.boot
            || parent.started.parse::<u128>().map_err(io::Error::other)?
                > child.started.parse::<u128>().map_err(io::Error::other)?
            || Self::capture(pid)? != child
            || native_parent_pid(pid)? != parent_pid
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Native parent identity changed",
            ));
        }
        Ok(parent)
    }

    pub fn alive(&self) -> bool {
        Self::capture(self.pid).is_ok_and(|current| current == *self)
    }
    /// Positive exit/replacement evidence only; inaccessible metadata is
    /// inconclusive. `!alive()` cannot substitute for this recovery boundary.
    pub fn has_exited(&self) -> bool {
        exit_evidence(self, Self::capture(self.pid))
    }
    pub fn key(&self) -> String {
        format!("{}:{}:{}", self.boot, self.pid, self.started)
    }
}

fn exit_evidence(host: &NativeHost, observed: io::Result<NativeHost>) -> bool {
    match observed {
        Ok(current) => current != *host,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

#[cfg(target_os = "linux")]
fn native_parent_pid(pid: u32) -> io::Result<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))?
        .rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().nth(1))
        .ok_or_else(|| io::Error::other("Missing native parent identity"))?
        .parse()
        .map_err(io::Error::other)
}

#[cfg(target_os = "macos")]
fn native_parent_pid(pid: u32) -> io::Result<u32> {
    Ok(macos_process(pid)?.pbi_ppid)
}

#[cfg(target_os = "macos")]
fn macos_process(pid: u32) -> io::Result<libproc::bsd_info::BSDInfo> {
    use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
    let pid = i32::try_from(pid).map_err(io::Error::other)?;
    let info = libproc::proc_pid::pidinfo::<libproc::bsd_info::BSDInfo>(pid, 0).map_err(|_| {
        // libproc returns a formatted string, not an errno. Never infer exit
        // from English text; use the native existence check for that case.
        match kill(Pid::from_raw(pid), None) {
            Err(Errno::ESRCH) => io::Error::new(io::ErrorKind::NotFound, "Native host has exited"),
            Err(Errno::EPERM) => io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Native host is inaccessible",
            ),
            _ => io::Error::other("Native host identity is unavailable"),
        }
    })?;
    if info.pbi_status == nix::libc::SZOMB {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Native host has exited",
        ));
    }
    Ok(info)
}

#[cfg(windows)]
fn native_parent_pid(pid: u32) -> io::Result<u32> {
    cccc_windows_process::parent_pid(pid)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn native_parent_pid(_pid: u32) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Native parent lookup is unavailable",
    ))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetireReason {
    SessionEnd,
    IntegrationDisabled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeSession {
    pub source_id: String,
    pub runtime: LocalSessionRuntime,
    pub native_session_id: String,
    pub host: NativeHost,
    pub cwd: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub revision: String,
    #[serde(default)]
    pub codex_home: Option<PathBuf>,
    #[serde(default)]
    pub codex_can_receive: bool,
    #[serde(default)]
    pub codex_program: Option<PathBuf>,
    #[serde(default)]
    pub claude_config_dir: Option<PathBuf>,
    #[serde(default)]
    pub pi_agent_dir: Option<PathBuf>,
    #[serde(default)]
    pub opencode_config_dir: Option<PathBuf>,
    /// OpenCode shares one MCP process across native sessions. Match its
    /// actual parent to the backend captured by the original TUI extension.
    #[serde(default)]
    pub opencode_server: Option<NativeHost>,
    /// Retained recovery metadata is not a live lifecycle identity.
    #[serde(default)]
    pub retired: Option<RetireReason>,
}

impl NativeSession {
    /// Captured Claude/Pi conversation metadata for recovery and diagnostics.
    /// This match alone grants no authority: the daemon must also fence the
    /// lifecycle, prove the old host exited, and preserve membership access.
    pub fn same_native_conversation(&self, other: &Self) -> bool {
        if self.runtime != other.runtime
            || self.native_session_id.is_empty()
            || self.native_session_id != other.native_session_id
        {
            return false;
        }
        let directories = match self.runtime {
            LocalSessionRuntime::Claude => (
                self.claude_config_dir.as_ref(),
                other.claude_config_dir.as_ref(),
            ),
            LocalSessionRuntime::Pi => (self.pi_agent_dir.as_ref(), other.pi_agent_dir.as_ref()),
            _ => return false,
        };
        match directories {
            (Some(left), Some(right)) => match (left.canonicalize(), right.canonicalize()) {
                (Ok(left), Ok(right)) => left == right,
                _ => false,
            },
            _ => false,
        }
    }

    pub fn host_alive(&self) -> bool {
        self.host.alive()
            && (self.runtime != LocalSessionRuntime::Opencode
                || self.opencode_server.as_ref().is_some_and(NativeHost::alive))
    }
    pub fn source(&self, connected: bool) -> LocalSessionSource {
        LocalSessionSource {
            source_id: self.source_id.clone(),
            runtime: self.runtime,
            native_session_id: self.native_session_id.clone(),
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            origin: self.origin.clone(),
            connected,
        }
    }
}

fn path(home: &HomeLayout, source_id: &str) -> io::Result<PathBuf> {
    let (runtime, key) = source_id
        .split_once(':')
        .ok_or_else(|| io::Error::other("Invalid native source identifier"))?;
    if !matches!(runtime, "codex" | "claude" | "pi" | "opencode")
        || key.is_empty()
        || key.len() > 256
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_:".contains(&c))
    {
        return Err(io::Error::other("Invalid native source identifier"));
    }
    Ok(home
        .root()
        .join("state/local_sessions")
        .join(format!("{}.json", source_id.replace(':', "%3A"))))
}

/// Read the last lifecycle record, including ended sources, for recovery only.
/// Authorization and delivery must use `read` and check the host is alive,
/// never authorize from this retained metadata.
pub fn read_record(home: &HomeLayout, source_id: &str) -> io::Result<NativeSession> {
    let session: NativeSession = fs::read_json(&path(home, source_id)?)?;
    if session.source_id != source_id {
        return Err(io::Error::other("Native session identity mismatch"));
    }
    Ok(session)
}

/// An explicit disable also fences callbacks still loaded in existing hosts,
/// including callbacks for a source that has never been registered before.
pub fn integration_disabled(home: &HomeLayout, runtime: LocalSessionRuntime) -> io::Result<bool> {
    let path = home
        .root()
        .join("state/local_sessions")
        .join(format!("{}.disabled", runtime.as_str()));
    match fs::read_json(&path) {
        Ok(disabled) => Ok(disabled),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn set_integration_disabled(
    home: &HomeLayout,
    runtime: LocalSessionRuntime,
    disabled: bool,
) -> io::Result<()> {
    home.initialize().map_err(io::Error::other)?;
    fs::with_exclusive_lock(&home.root().join("state/local_sessions.lock"), || {
        let path = home
            .root()
            .join("state/local_sessions")
            .join(format!("{}.disabled", runtime.as_str()));
        if disabled {
            // Close authorization before retiring records or invoking slow
            // provider configuration. Failed cleanup must remain disabled.
            fs::write_json_committed(&path, &true)?;
        }
        if disabled || integration_disabled(home, runtime)? {
            let (sessions, errors) = scan_records(home)?;
            for mut session in sessions
                .into_iter()
                .filter(|session| session.runtime == runtime)
            {
                if session.retired != Some(RetireReason::IntegrationDisabled) {
                    session.retired = Some(RetireReason::IntegrationDisabled);
                    session.codex_can_receive = false;
                    fs::write_json_committed(&self::path(home, &session.source_id)?, &session)?;
                }
            }
            if errors
                .iter()
                .any(|error| error.starts_with(&format!("{}:", runtime.as_str())))
            {
                return Err(io::Error::other(
                    "Native integration is disabled, but some lifecycle records are unreadable; repair them before enabling it again",
                ));
            }
        }
        if !disabled {
            // Finish any previously interrupted retirement before reopening
            // registration. Enabling alone never revives a retained identity.
            fs::write_json_committed(&path, &false)?;
        }
        Ok(())
    })
}

pub fn read(home: &HomeLayout, source_id: &str) -> io::Result<NativeSession> {
    let session = read_record(home, source_id)?;
    if session.retired.is_some() || integration_disabled(home, session.runtime)? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Native lifecycle identity has ended or its integration is disabled",
        ));
    }
    Ok(session)
}

pub fn validate_source(
    home: &HomeLayout,
    source_id: &str,
    session_revision: &str,
) -> io::Result<NativeSession> {
    let session = read(home, source_id)?;
    if session.revision != session_revision || !session.host_alive() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "The captured native session authority is no longer valid",
        ));
    }
    Ok(session)
}

pub fn validate_binding(
    home: &HomeLayout,
    binding: &cccc_contracts::local_session::LocalSessionBinding,
) -> io::Result<()> {
    let group = crate::GroupStore::new(home.clone())?.load(&binding.group_id)?;
    let actor = crate::actors::find(&group, &binding.actor_id)
        .ok_or_else(|| io::Error::other("Native membership is unavailable"))?;
    let session = validate_source(home, &binding.source_id, &binding.session_revision)?;
    if !actor.enabled
        || actor.external_session_id != binding.source_id
        || crate::actors::generation_identity(actor) != binding.actor_generation
        || !matches!(
            (actor.runtime, session.runtime),
            (
                cccc_contracts::ActorRuntime::Codex,
                LocalSessionRuntime::Codex
            ) | (
                cccc_contracts::ActorRuntime::Claude,
                LocalSessionRuntime::Claude
            ) | (cccc_contracts::ActorRuntime::Pi, LocalSessionRuntime::Pi)
                | (
                    cccc_contracts::ActorRuntime::Opencode,
                    LocalSessionRuntime::Opencode
                )
        )
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "The captured native session authority is no longer valid",
        ));
    }
    Ok(())
}

pub fn write(home: &HomeLayout, session: NativeSession) -> io::Result<NativeSession> {
    let ready = session.codex_can_receive;
    write_with_codex_readiness(home, session, Some(ready))
}

/// Unknown capability preserves evidence only within this exact live endpoint.
/// Merge under the lifecycle lock so a concurrent negative check is not lost.
pub fn write_with_codex_readiness(
    home: &HomeLayout,
    mut session: NativeSession,
    ready: Option<bool>,
) -> io::Result<NativeSession> {
    home.initialize().map_err(io::Error::other)?;
    fs::with_exclusive_lock(&home.root().join("state/local_sessions.lock"), || {
        if integration_disabled(home, session.runtime)? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Native integration is explicitly disabled",
            ));
        }
        session.codex_can_receive = ready.unwrap_or(false);
        if session.runtime == LocalSessionRuntime::Opencode
            && let Ok(previous) = read(home, &session.source_id)
            && previous.host != session.host
            && previous.host.alive()
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "This OpenCode session is already connected through another live terminal",
            ));
        }
        if let Ok(previous) = read(home, &session.source_id)
            && previous.native_session_id == session.native_session_id
            && previous.host == session.host
            // One registration per extension incarnation. A reloaded TUI
            // must not inherit authority from its previous receive bridge.
            && session.runtime != LocalSessionRuntime::Opencode
        {
            session.revision.clone_from(&previous.revision);
            if ready.is_none()
                && previous.cwd == session.cwd
                && previous.codex_home == session.codex_home
                && previous.codex_program == session.codex_program
            {
                session.codex_can_receive = previous.codex_can_receive;
            }
            if session.origin.is_none()
                && previous.cwd == session.cwd
                && previous.codex_home == session.codex_home
                && previous.codex_program == session.codex_program
            {
                session.origin.clone_from(&previous.origin);
            }
            if session == previous {
                return Ok(session);
            }
        }
        fs::write_json_committed(&path(home, &session.source_id)?, &session)?;
        Ok(session)
    })
}

pub fn retire(
    home: &HomeLayout,
    source_id: &str,
    native_id: &str,
    host: &NativeHost,
    reason: RetireReason,
) -> io::Result<()> {
    retire_revision(home, source_id, native_id, host, None, reason)
}

/// An extension receiver ending after a reload must not retire its replacement.
pub fn retire_revision(
    home: &HomeLayout,
    source_id: &str,
    native_id: &str,
    host: &NativeHost,
    revision: Option<&str>,
    reason: RetireReason,
) -> io::Result<()> {
    fs::with_exclusive_lock(&home.root().join("state/local_sessions.lock"), || {
        let session = match read_record(home, source_id) {
            Ok(session) => session,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        // SessionEnd can arrive after another conversation starts on this host,
        // or after this Codex thread resumes on a replacement host. Explicit
        // disable also covers ended records and cannot be undone by a late End.
        if session.native_session_id == native_id
            && session.host == *host
            && revision.is_none_or(|revision| session.revision == revision)
            && session.retired != Some(reason)
            && session.retired != Some(RetireReason::IntegrationDisabled)
        {
            let mut session = session;
            session.retired = Some(reason);
            session.codex_can_receive = false;
            fs::write_json_committed(&path(home, source_id)?, &session)?;
        }
        Ok(())
    })
}

/// Capability evidence belongs to the exact native endpoint checked. Never
/// revive retired identity, and do not qualify a host that has since exited.
pub fn set_codex_receive_ready(
    home: &HomeLayout,
    expected: &NativeSession,
    ready: bool,
) -> io::Result<()> {
    fs::with_exclusive_lock(&home.root().join("state/local_sessions.lock"), || {
        let mut session = match read(home, &expected.source_id) {
            Ok(session) => session,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if session.runtime == LocalSessionRuntime::Codex
            && session.runtime == expected.runtime
            && session.native_session_id == expected.native_session_id
            && session.host == expected.host
            && session.revision == expected.revision
            && session.codex_home == expected.codex_home
            && session.codex_program == expected.codex_program
            && session.cwd == expected.cwd
            && session.codex_can_receive != ready
            && (!ready || session.host.alive())
        {
            session.codex_can_receive = ready;
            fs::write_json_committed(&path(home, &session.source_id)?, &session)?;
        }
        Ok(())
    })
}

/// Enumerate retained records for explicit administration, not authorization.
pub fn scan_records(home: &HomeLayout) -> io::Result<(Vec<NativeSession>, Vec<String>)> {
    let directory = home.root().join("state/local_sessions");
    let files = match std::fs::read_dir(directory) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((vec![], vec![])),
        Err(error) => return Err(error),
    };
    let mut sessions = Vec::new();
    let mut errors = Vec::new();
    for entry in files {
        let entry = entry?;
        let file = entry.path();
        if file.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Some(id) = file.file_stem().and_then(|id| id.to_str()) else {
            continue;
        };
        let id = id.replace("%3A", ":");
        match read_record(home, &id) {
            Ok(session) => sessions.push(session),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => errors.push(format!("{id}: {error}")),
        }
    }
    Ok((sessions, errors))
}

pub fn scan(home: &HomeLayout) -> io::Result<(Vec<NativeSession>, Vec<String>)> {
    let (mut sessions, errors) = scan_records(home)?;
    let codex_disabled = integration_disabled(home, LocalSessionRuntime::Codex)?;
    let claude_disabled = integration_disabled(home, LocalSessionRuntime::Claude)?;
    let pi_disabled = integration_disabled(home, LocalSessionRuntime::Pi)?;
    let opencode_disabled = integration_disabled(home, LocalSessionRuntime::Opencode)?;
    sessions.retain(|session| {
        session.retired.is_none()
            && session.host_alive()
            && !match session.runtime {
                LocalSessionRuntime::Codex => codex_disabled,
                LocalSessionRuntime::Claude => claude_disabled,
                LocalSessionRuntime::Pi => pi_disabled,
                LocalSessionRuntime::Opencode => opencode_disabled,
            }
    });
    Ok((sessions, errors))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod tests {
    #[test]
    fn permission_and_unknown_process_errors_do_not_establish_exit() {
        use super::*;
        let host = NativeHost::capture(std::process::id()).expect("host");
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
            io::ErrorKind::Interrupted,
        ] {
            assert!(!exit_evidence(&host, Err(io::Error::from(kind))));
        }
        assert!(!exit_evidence(&host, Ok(host.clone())));
        assert!(exit_evidence(
            &host,
            Err(io::Error::from(io::ErrorKind::NotFound))
        ));
    }
    use super::*;

    #[test]
    fn pi_reload_cleanup_is_fenced_even_when_the_native_session_id_is_unchanged() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let host = NativeHost::capture(std::process::id()).expect("host");
        let source_id = LocalSessionRuntime::Pi.source_id(&host.key());
        let session: NativeSession = serde_json::from_value(serde_json::json!({
            "source_id":source_id,"runtime":"pi","native_session_id":"same-conversation",
            "host":host,"cwd":temp.path(),"title":"Pi fixture","revision":"before-reload"
        }))
        .expect("Pi lifecycle");
        let before = write(&home, session.clone()).expect("register");
        retire_revision(
            &home,
            &source_id,
            &session.native_session_id,
            &host,
            Some(&before.revision),
            RetireReason::SessionEnd,
        )
        .expect("unload");
        let mut after = session.clone();
        after.revision = "after-reload".into();
        let after = write(&home, after).expect("reloaded extension");
        retire_revision(
            &home,
            &source_id,
            &session.native_session_id,
            &host,
            Some(&before.revision),
            RetireReason::SessionEnd,
        )
        .expect("late old cleanup");
        assert_eq!(
            read(&home, &source_id).expect("replacement retained"),
            after
        );
        set_integration_disabled(&home, LocalSessionRuntime::Pi, true).expect("disable");
        assert!(
            write(&home, session).is_err(),
            "loaded extension cannot undo disable"
        );
    }

    #[test]
    fn powershell_path_literals_escape_ascii_and_smart_quote_delimiters() {
        for (path, literal) in [
            (
                "repo'; Write-Output injected; #",
                "'repo''; Write-Output injected; #'",
            ),
            (
                "repo‘; Write-Output injected; #",
                "'repo‘‘; Write-Output injected; #'",
            ),
            (
                "repo’; Write-Output injected; #",
                "'repo’’; Write-Output injected; #'",
            ),
            (
                "repo‚; Write-Output injected; #",
                "'repo‚‚; Write-Output injected; #'",
            ),
            (
                "repo‛; Write-Output injected; #",
                "'repo‛‛; Write-Output injected; #'",
            ),
        ] {
            assert_eq!(powershell_quote(path), literal);
        }
    }

    #[test]
    fn host_identity_and_native_parent_follow_process_lifetime() {
        use std::process::{Command, Stdio};
        if std::env::var_os("CCCC_TEST_NATIVE_HOST_CHILD").is_some() {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            return;
        }
        let current = NativeHost::capture(std::process::id()).expect("native host");
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "local_sessions::tests::host_identity_and_native_parent_follow_process_lifetime",
            ])
            .env("CCCC_TEST_NATIVE_HOST_CHILD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .expect("owned child");
        let identity = NativeHost::capture(child.id());
        let parent = NativeHost::parent(child.id());
        let alive = identity.as_ref().is_ok_and(NativeHost::alive);
        child.kill().expect("stop owned child");
        child.wait().expect("reap owned child");
        let identity = identity.expect("child identity");
        assert!(alive);
        assert_eq!(parent.expect("native parent"), current);
        assert!(
            !identity.alive(),
            "exited child must not remain a live identity"
        );
        let mut stale = current.clone();
        stale.started.push('0');
        assert!(
            !stale.alive(),
            "a reused PID with another birth time must not match"
        );
        assert!(current.alive());
    }

    #[test]
    fn a_late_end_cannot_remove_the_new_conversation_and_reused_hosts_are_rejected() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let host = NativeHost::capture(std::process::id()).expect("host");
        let mut session = NativeSession {
            source_id: format!("claude:{}", host.key()),
            runtime: LocalSessionRuntime::Claude,
            native_session_id: "first".into(),
            host: host.clone(),
            cwd: "/tmp".into(),
            title: "Claude".into(),
            origin: None,
            revision: "one".into(),
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: Some("/fixture/claude".into()),
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: None,
            retired: None,
        };
        write(&home, session.clone()).expect("write");
        session.native_session_id = "second".into();
        session.revision = "two".into();
        write(&home, session.clone()).expect("clear");
        retire(
            &home,
            &session.source_id,
            "first",
            &host,
            RetireReason::SessionEnd,
        )
        .expect("late end");
        assert_eq!(
            read(&home, &session.source_id)
                .expect("current")
                .native_session_id,
            "second"
        );
        assert!(session.host.alive());
        session.host.started.push('0');
        assert!(!session.host.alive());
        let mut dead = session.clone();
        dead.source_id = "codex:dead-fixture".into();
        dead.runtime = LocalSessionRuntime::Codex;
        write(&home, dead.clone()).expect("dead metadata");
        assert_eq!(scan(&home).expect("scan").0.len(), 1);
        assert_eq!(
            read_record(&home, &dead.source_id).expect("crash recovery"),
            dead
        );
        retire(
            &home,
            &session.source_id,
            "second",
            &host,
            RetireReason::SessionEnd,
        )
        .expect("end");
        assert!(read(&home, &session.source_id).is_err());
        let ended = read_record(&home, &session.source_id).expect("recovery after End");
        assert_eq!(ended.native_session_id, "second");
        assert_eq!(ended.claude_config_dir, Some("/fixture/claude".into()));
        assert_eq!(ended.retired, Some(RetireReason::SessionEnd));
        assert!(scan(&home).expect("no ended sources").0.is_empty());
    }

    #[test]
    fn a_late_end_cannot_remove_a_codex_thread_resumed_on_another_host() {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let host = NativeHost::capture(std::process::id()).expect("host");
        let mut session = NativeSession {
            source_id: "codex:same-thread".into(),
            runtime: LocalSessionRuntime::Codex,
            native_session_id: "same-thread".into(),
            host: host.clone(),
            cwd: "/tmp".into(),
            title: "Codex".into(),
            origin: None,
            revision: "one".into(),
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: None,
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: None,
            retired: None,
        };
        write(&home, session.clone()).expect("first host");
        session.host.started.push('0');
        session.revision = "two".into();
        let current = write(&home, session.clone()).expect("resumed thread");
        retire(
            &home,
            &session.source_id,
            &session.native_session_id,
            &host,
            RetireReason::SessionEnd,
        )
        .expect("late end");
        assert_eq!(
            read(&home, &session.source_id).expect("replacement"),
            current
        );
        retire(
            &home,
            &session.source_id,
            &session.native_session_id,
            &session.host,
            RetireReason::SessionEnd,
        )
        .expect("replacement end");
        assert_eq!(
            read(&home, &session.source_id).expect_err("retired").kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn stale_codex_capability_evidence_cannot_disconnect_another_incarnation_or_endpoint() {
        for changed in [
            "host",
            "native_id",
            "revision",
            "codex_home",
            "program",
            "cwd",
        ] {
            let temp = tempfile::tempdir().expect("temp");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let original = NativeSession {
                source_id: "codex:thread-fixture".into(),
                runtime: LocalSessionRuntime::Codex,
                native_session_id: "thread-fixture".into(),
                host: NativeHost::capture(std::process::id()).expect("host"),
                cwd: "/fixture/project".into(),
                title: "Codex".into(),
                origin: None,
                revision: "one".into(),
                codex_home: Some("/fixture/codex-home".into()),
                codex_can_receive: true,
                codex_program: Some("/fixture/codex".into()),
                claude_config_dir: None,
                pi_agent_dir: None,
                opencode_config_dir: None,
                opencode_server: None,
                retired: None,
            };
            write(&home, original.clone()).expect("original");
            let mut replacement = original.clone();
            match changed {
                "host" => replacement.host.started.push('0'),
                "native_id" => replacement.native_session_id.push_str("-replacement"),
                "revision" => {
                    retire(
                        &home,
                        &original.source_id,
                        &original.native_session_id,
                        &original.host,
                        RetireReason::SessionEnd,
                    )
                    .expect("previous incarnation ended");
                    replacement.revision = "two".into();
                }
                "codex_home" => replacement.codex_home = Some("/fixture/new-home".into()),
                "program" => replacement.codex_program = Some("/fixture/new-codex".into()),
                "cwd" => replacement.cwd = "/fixture/new-project".into(),
                _ => unreachable!(),
            }
            let replacement = write(&home, replacement).expect("replacement");
            set_codex_receive_ready(&home, &original, false).expect("stale negative capability");
            assert_eq!(
                read(&home, &original.source_id).expect("current"),
                replacement,
                "changed {changed}"
            );
            set_codex_receive_ready(&home, &replacement, false)
                .expect("current negative capability");
            let mut disconnected = replacement.clone();
            disconnected.codex_can_receive = false;
            assert_eq!(
                read(&home, &original.source_id).expect("identity retained"),
                disconnected
            );
            set_codex_receive_ready(&home, &replacement, false).expect("idempotent disconnect");
            set_codex_receive_ready(&home, &original, true).expect("stale positive capability");
            assert_eq!(
                read(&home, &original.source_id).expect("still retained"),
                disconnected
            );
        }
    }

    #[test]
    fn unknown_codex_readiness_does_not_cross_a_lifecycle_or_endpoint_boundary() {
        for changed in [
            "same",
            "host",
            "native_id",
            "retired",
            "home",
            "program",
            "cwd",
        ] {
            let temp = tempfile::tempdir().expect("temp");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let original = NativeSession {
                source_id: "codex:thread-fixture".into(),
                runtime: LocalSessionRuntime::Codex,
                native_session_id: "thread-fixture".into(),
                host: NativeHost::capture(std::process::id()).expect("host"),
                cwd: "/fixture/project".into(),
                title: "Codex".into(),
                origin: None,
                revision: "one".into(),
                codex_home: Some("/fixture/codex-home".into()),
                codex_can_receive: true,
                codex_program: Some("/fixture/codex".into()),
                claude_config_dir: None,
                pi_agent_dir: None,
                opencode_config_dir: None,
                opencode_server: None,
                retired: None,
            };
            write(&home, original.clone()).expect("positive evidence");
            let mut callback = original.clone();
            callback.revision = "new-callback".into();
            callback.codex_can_receive = false;
            match changed {
                "same" => callback.title = "Updated title".into(),
                "host" => callback.host.started.push('0'),
                "native_id" => callback.native_session_id.push_str("-replacement"),
                "retired" => retire(
                    &home,
                    &original.source_id,
                    &original.native_session_id,
                    &original.host,
                    RetireReason::SessionEnd,
                )
                .expect("end"),
                "home" => callback.codex_home = Some("/fixture/another-home".into()),
                "program" => callback.codex_program = Some("/fixture/another-codex".into()),
                "cwd" => callback.cwd = "/fixture/another-project".into(),
                _ => unreachable!(),
            }
            let current = write_with_codex_readiness(&home, callback.clone(), None)
                .expect("unknown callback evidence");
            assert_eq!(current.codex_can_receive, changed == "same", "{changed}");
            if changed == "same" {
                assert_eq!(current.revision, original.revision);
                set_codex_receive_ready(&home, &current, false).expect("concurrent negative");
                assert!(
                    !write_with_codex_readiness(&home, callback, None)
                        .expect("unknown cannot overwrite current negative")
                        .codex_can_receive
                );
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod opencode_tests {
    use super::*;
    fn session() -> NativeSession {
        let host = NativeHost::capture(std::process::id()).expect("host");
        NativeSession {
            source_id: "opencode:ses_fixture".into(),
            runtime: LocalSessionRuntime::Opencode,
            native_session_id: "ses_fixture".into(),
            host: host.clone(),
            cwd: "/tmp".into(),
            title: "OpenCode".into(),
            origin: None,
            revision: "first".into(),
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: None,
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: Some(host),
            retired: None,
        }
    }
    #[test]
    fn reload_and_backend_loss_fence_original_opencode_authority() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("layout");
        let first = write(&home, session()).expect("initial registration");
        let mut replacement = first.clone();
        replacement.revision = "reloaded".into();
        let replacement = write(&home, replacement).expect("reload");
        assert_ne!(first.revision, replacement.revision);
        retire_revision(
            &home,
            &first.source_id,
            &first.native_session_id,
            &first.host,
            Some(&first.revision),
            RetireReason::SessionEnd,
        )
        .expect("late cleanup");
        assert_eq!(
            read(&home, &first.source_id).expect("replacement"),
            replacement
        );
        assert!(validate_source(&home, &first.source_id, &first.revision).is_err());
        validate_source(&home, &replacement.source_id, &replacement.revision)
            .expect("current authority");
        let mut absent = replacement.clone();
        absent.opencode_server.as_mut().expect("backend").pid = u32::MAX;
        let absent = write(&home, absent).expect("backend replaced");
        assert!(validate_source(&home, &absent.source_id, &absent.revision).is_err());
        assert!(scan(&home).expect("directory").0.is_empty());
    }
    #[test]
    fn a_second_live_terminal_cannot_steal_an_opencode_source() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("layout");
        let mut child = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .expect("fixture host");
        let mut first = session();
        first.host = NativeHost::capture(child.id()).expect("fixture fingerprint");
        let result = write(&home, first).and_then(|first| write(&home, session()).map(|_| first));
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(
            result.expect_err("live ownership must be preserved").kind(),
            io::ErrorKind::AlreadyExists
        );
        write(&home, session()).expect("explicit reopen after original host exit");
    }
}
