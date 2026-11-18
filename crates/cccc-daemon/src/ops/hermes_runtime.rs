use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use cccc_contracts::{ActorRuntime, DaemonRequest};
use cccc_core::HomeLayout;
use cccc_core::fs::with_exclusive_lock;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::dispatch::{OpError, OpResult, object, string_arg};

mod config;
mod process;

#[cfg(all(test, unix))]
mod tests;

use config::{PersistentEntry, find_executable, hermes_home, persistent_entry};
use process::run;

pub(super) const SERVER: &str = "cccc";
pub(super) const PLACEHOLDERS: [(&str, &str); 3] = [
    ("CCCC_HOME", "${CCCC_HOME}"),
    ("CCCC_GROUP_ID", "${CCCC_GROUP_ID}"),
    ("CCCC_ACTOR_ID", "${CCCC_ACTOR_ID}"),
];

const ENTRY_KEY: &str = "mcp_servers.cccc";
const CLI_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "runtime_hermes_status" => Operation::new(Read, |home, _request| status(home)),
        "runtime_hermes_prepare" => Operation::new(Write, prepare),
        "runtime_hermes_mcp_test" => Operation::new(Read, mcp_test),
        _ => return None,
    })
}

/// Prepares a Hermes member start and returns the command to launch.
///
/// Without `-p`, Hermes rereads its sticky profile (`hermes profile use`) on
/// every command. The profile it selects now is added to the command, so the
/// entry check, the ACP session, first setup and every later reopening of the
/// member's terminal use the same one; the member's saved command is unchanged.
///
/// Hermes attaches every enabled `mcp_servers` entry of a profile to every
/// session in it, so a persistent `cccc` entry would hand every member one
/// CCCC identity. CCCC injects its server into each member's own session
/// instead: it removes the entry earlier CCCC versions wrote, and refuses to
/// start past any other `cccc` server rather than guess whose it is.
pub(super) fn prepare_member_start(
    home: &HomeLayout,
    command: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Vec<String>, OpError> {
    let mut command = if command.is_empty() {
        cccc_runtime::default_command(ActorRuntime::Hermes)
    } else {
        command.to_vec()
    };
    let named = crate::ops::codex_voice_analyst::hermes_profile(&command).map_err(OpError::io)?;
    let hermes = find_executable_in_env(&command[0], env, cwd).ok_or_else(cli_missing)?;
    let values = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect::<Vec<_>>();
    let cli = Cli {
        hermes: &hermes,
        profile: named.as_deref(),
        cwd: Some(cwd),
        env: &values,
    };
    let hermes_home = env
        .get("HERMES_HOME")
        .cloned()
        .or_else(|| std::env::var("HERMES_HOME").ok());
    let (pinned, path) = match cli.pin(hermes_home.as_deref())? {
        Some((profile, path)) => (Some(profile), Some(path)),
        None => (None, None),
    };
    if let Some(profile) = &pinned {
        command.splice(1..1, ["--profile".to_owned(), profile.clone()]);
    }
    Cli {
        profile: pinned.as_deref().or(cli.profile),
        ..cli
    }
    .retire(home, path)?;
    Ok(command)
}

/// What a check finds in the profile a [`Cli`] runs in.
enum Finding {
    Absent,
    /// The entry earlier CCCC versions wrote, in this config file.
    WrittenByCccc(PathBuf),
}

/// The Hermes CLI as a member, or the user's own shell, runs it.
#[derive(Clone, Copy)]
struct Cli<'a> {
    hermes: &'a Path,
    profile: Option<&'a str>,
    cwd: Option<&'a Path>,
    env: &'a [(&'a str, String)],
}

impl Cli<'_> {
    fn run(&self, args: &[&str], input: Option<&str>) -> Result<Value, OpError> {
        let mut argv = Vec::new();
        if let Some(profile) = self.profile {
            argv.extend(["--profile".to_owned(), profile.to_owned()]);
        }
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        run(self.hermes, &argv, self.cwd, input, self.env, CLI_TIMEOUT).map_err(OpError::io)
    }

    /// The config Hermes itself reads, a sticky `hermes profile use` included.
    fn config_path(&self) -> Result<PathBuf, OpError> {
        let output = self.run(&["config", "path"], None)?;
        output["stdout"]
            .as_str()
            .and_then(|stdout| stdout.lines().map(str::trim).rfind(|line| !line.is_empty()))
            .map(PathBuf::from)
            .filter(|path| output["returncode"] == 0 && path.is_absolute())
            .ok_or_else(|| profile_unavailable(&output))
    }

    /// The profile Hermes selects now, as `--profile` names it, with its config
    /// file. `None` when there is nothing to pin: the command names a profile,
    /// or HERMES_HOME names a profile directory, which Hermes uses as it is.
    fn pin(&self, hermes_home: Option<&str>) -> Result<Option<(String, PathBuf)>, OpError> {
        let profile_directory = hermes_home.is_some_and(|home| {
            Path::new(home)
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "profiles")
        });
        if self.profile.is_some() || profile_directory {
            return Ok(None);
        }
        let path = self.config_path()?;
        let profile = profile_of(&path);
        let resolved = Cli {
            profile: Some(&profile),
            ..*self
        }
        .config_path()?;
        if resolved != path {
            return Err(OpError::new(
                "hermes_profile_mismatch",
                format!(
                    "Hermes reported {}, but `hermes --profile {profile} config path` reports {}; CCCC changed nothing. If Hermes's active profile was being switched, start the member again.",
                    path.display(),
                    resolved.display()
                ),
            ));
        }
        Ok(Some((profile, path)))
    }

    /// The profile to name and the config file the user's own `hermes` selects now.
    fn select(&self) -> Result<(Option<String>, PathBuf), OpError> {
        let hermes_home = std::env::var("HERMES_HOME").ok();
        match self.pin(hermes_home.as_deref())? {
            Some((profile, path)) => Ok((Some(profile), path)),
            None => Ok((None, self.config_path()?)),
        }
    }

    /// Whether Hermes's effective config for this profile, managed layers
    /// included, defines a `cccc` server, enabled or not.
    fn effective_entry(&self) -> Result<bool, OpError> {
        let output = self.run(&["config", "get", ENTRY_KEY, "--json"], None)?;
        if output["returncode"] == 0 {
            return Ok(true);
        }
        if output["stderr"]
            .as_str()
            .is_some_and(|stderr| stderr.contains("Config key not set"))
        {
            return Ok(false);
        }
        Err(profile_unavailable(&output))
    }

    /// Only the user's own config file can show that CCCC wrote an entry;
    /// what Hermes reports as effective decides whether one exists at all.
    fn inspect(&self, path: Option<PathBuf>) -> Result<Finding, OpError> {
        if !self.effective_entry()? {
            return Ok(Finding::Absent);
        }
        let path = match path {
            Some(path) => path,
            None => self.config_path()?,
        };
        self.classify(&path)
    }

    fn classify(&self, path: &Path) -> Result<Finding, OpError> {
        match persistent_entry(path)? {
            PersistentEntry::WrittenByCccc => Ok(Finding::WrittenByCccc(path.to_path_buf())),
            PersistentEntry::Other => Err(entry_conflict(path, &self.removal())),
            PersistentEntry::Absent if self.effective_entry()? => Err(entry_elsewhere(path)),
            PersistentEntry::Absent => Ok(Finding::Absent),
        }
    }

    /// The command that removes the entry from this profile's config file.
    fn removal(&self) -> String {
        match self.profile {
            Some(profile) => format!("`hermes --profile {profile} config unset {ENTRY_KEY}`"),
            None => format!("`hermes config unset {ENTRY_KEY}` with the same HERMES_HOME"),
        }
    }

    fn retire(&self, home: &HomeLayout, path: Option<PathBuf>) -> Result<Value, OpError> {
        let Finding::WrittenByCccc(path) = self.inspect(path)? else {
            return Ok(json!({"persistent_entry":"absent"}));
        };
        let lock = home.daemon_dir().join("hermes-runtime-setup.lock");
        with_exclusive_lock(&lock, || Ok(self.remove(&path))).map_err(OpError::io)?
    }

    /// Remove the entry CCCC wrote from `path`, the config file of this profile.
    fn remove(&self, path: &Path) -> Result<Value, OpError> {
        // Another member's start may have removed it while this one waited.
        if matches!(self.classify(path)?, Finding::Absent) {
            return Ok(json!({"persistent_entry":"absent","config_path":path}));
        }
        // Only the key: `hermes mcp remove` also deletes OAuth data kept under
        // the server's name, which this stdio entry cannot be shown to own.
        let removed = self.run(&["config", "unset", ENTRY_KEY], None)?;
        // Read again rather than trust the exit status.
        match self.classify(path)? {
            Finding::Absent => Ok(json!({"persistent_entry":"removed","config_path":path})),
            Finding::WrittenByCccc(_) => Err(OpError::new(
                "hermes_mcp_cleanup_failed",
                format!(
                    "Hermes kept the 'cccc' MCP entry in {}: {}",
                    path.display(),
                    cli_error(&removed, "the removal did not take effect")
                ),
            )),
        }
    }
}

/// The profile name Hermes resolves to the config file at `path`.
fn profile_of(path: &Path) -> String {
    let home = path.parent();
    match (
        home.and_then(Path::file_name),
        home.and_then(Path::parent).and_then(Path::file_name),
    ) {
        (Some(name), Some(parent)) if parent == "profiles" => name.to_string_lossy().into_owned(),
        _ => "default".to_owned(),
    }
}

fn profile_unavailable(output: &Value) -> OpError {
    OpError::new(
        "hermes_profile_unavailable",
        cli_error(output, "Hermes did not report its configuration"),
    )
}

fn entry_conflict(path: &Path, removal: &str) -> OpError {
    OpError::new(
        "hermes_mcp_entry_conflict",
        format!(
            "{} has a 'cccc' MCP server, and CCCC cannot confirm who wrote it. When it is enabled, Hermes gives it to every session of this profile, so every member would act as the same CCCC identity. Remove it with {removal}, then start the member again.",
            path.display()
        ),
    )
}

fn entry_elsewhere(path: &Path) -> OpError {
    OpError::new(
        "hermes_mcp_entry_conflict",
        format!(
            "Hermes reads a 'cccc' MCP server for the profile of {} that is not a plain entry in that file, for example one from a managed configuration layer, and CCCC cannot confirm who wrote it. When it is enabled, Hermes gives it to every session of this profile, so every member would act as the same CCCC identity. Remove it where it is defined, then start the member again.",
            path.display()
        ),
    )
}

fn cli_error(output: &Value, fallback: &str) -> String {
    ["stderr", "stdout"]
        .iter()
        .find_map(|stream| {
            output[*stream]
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .unwrap_or(fallback)
        .chars()
        .take(600)
        .collect()
}

fn cli_missing() -> OpError {
    OpError::new(
        "hermes_cli_missing",
        "Hermes CLI is not installed or not in PATH",
    )
}

fn find_executable_in_env(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Option<PathBuf> {
    let inherited_path = std::env::var_os("PATH");
    let search_path = env
        .get("PATH")
        .map(std::ffi::OsStr::new)
        .or(inherited_path.as_deref());
    cccc_core::runtime_mcp::find_program_in(name, search_path, cwd)
}

fn status(home: &HomeLayout) -> OpResult {
    let hermes = find_executable("hermes");
    let version = hermes
        .as_ref()
        .and_then(|path| {
            run(
                path,
                &["--version".into()],
                None,
                None,
                &[],
                Duration::from_secs(10),
            )
            .ok()
        })
        .filter(|output| output["returncode"] == 0)
        .and_then(|output| {
            output["stdout"]
                .as_str()
                .map(|value| value.trim().lines().next().unwrap_or("").to_owned())
        })
        .unwrap_or_default();
    // The check a member start without `-p` makes; each issue is the error
    // code that start would fail with.
    let (selected, entry) = match hermes.as_deref() {
        None => (None, Err(cli_missing())),
        Some(hermes) => {
            let cli = Cli {
                hermes,
                profile: None,
                cwd: None,
                env: &[],
            };
            match cli.select() {
                Ok((profile, path)) => {
                    let entry = Cli {
                        profile: profile.as_deref(),
                        ..cli
                    }
                    .inspect(Some(path.clone()));
                    (Some(path), entry)
                }
                Err(error) => (None, Err(error)),
            }
        }
    };
    let config_path_verified = selected.is_some();
    // Unverified, this is only where the default profile keeps its file.
    let config_path = selected.unwrap_or_else(|| hermes_home().join("config.yaml"));
    let (persistent_entry, error) = match entry {
        Ok(Finding::Absent) => ("absent", None),
        // Removed when a member next starts, or by prepare.
        Ok(Finding::WrittenByCccc(_)) => ("cccc_written", None),
        Err(error) => {
            let entry = match error.code.as_str() {
                "hermes_mcp_entry_conflict" => "conflict",
                "hermes_config_unreadable" | "hermes_config_invalid" => "unreadable",
                _ => "unknown",
            };
            (entry, Some(error))
        }
    };
    let issues = Vec::from_iter(error.as_ref().map(|error| error.code.clone()));
    object(json!({
        "runtime":"hermes",
        "launch_ready":issues.is_empty(),
        "hermes_cli":{"available":hermes.is_some(),"path":hermes,"version":version},
        "config_path":config_path,
        "config_path_verified":config_path_verified,
        "config_exists":config_path.exists(),
        "mcp":{
            "mode":"injected_per_session",
            "persistent_entry":persistent_entry,
            "error":error.map(|error| error.message)
        },
        "issues":issues,
        "commands":{
            "prepare":"cccc runtime hermes prepare",
            "setup":"hermes model",
            "launch":cccc_runtime::default_command(ActorRuntime::Hermes).join(" ")
        },
        "cccc_home":home.root()
    }))
}

/// Members get CCCC's server in their own sessions. Prepare only removes,
/// from the profile Hermes selects, the persistent entry earlier versions
/// wrote, as a member start would, and reports what it found.
fn prepare(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let hermes = find_executable("hermes").ok_or_else(cli_missing)?;
    let cwd = string_arg(request, "cwd").map(PathBuf::from);
    let cli = Cli {
        hermes: &hermes,
        profile: None,
        cwd: cwd.as_deref(),
        env: &[],
    };
    let (profile, path) = cli.select()?;
    let cleanup = Cli {
        profile: profile.as_deref(),
        ..cli
    }
    .retire(home, Some(path))?;
    object(json!({
        "ok":true,"mcp":"injected_per_session","cleanup":cleanup,"status":status(home)?
    }))
}

/// No saved `cccc` server exists to test: each member's session gets its own.
fn mcp_test(_home: &HomeLayout, _request: &DaemonRequest) -> OpResult {
    object(json!({
        "ok":true,"applicable":false,"mcp":"injected_per_session",
        "message":"CCCC gives each Hermes member its own CCCC server in its session, so there is no saved server to test. Start the member and send it a message to check its tools."
    }))
}
