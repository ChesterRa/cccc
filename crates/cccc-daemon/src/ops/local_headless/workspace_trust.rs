//! Claude Code refuses a detached `--bg` launch in a workspace whose trust prompt was never
//! accepted. Instead of failing the start, show Claude's own trust prompt in the Actor's
//! terminal, and start the managed session once Claude accepts the workspace.

use super::supervisor::{Key, attach_managed, launch_managed};
use super::workspace_trust_recovery::{self, Recovery};
use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use cccc_core::{GroupDoc, HomeLayout};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const POLL: Duration = Duration::from_secs(1);

pub(super) fn refused(actor: &Actor, error: &io::Error) -> bool {
    actor.runtime == ActorRuntime::Claude
        && super::super::codex_voice_analyst::untrusted_claude_workspace(error).is_some()
}

/// Opens the configured Claude command interactively in the Actor's terminal, where Claude
/// asks the operator to trust the workspace. Only Claude records that decision.
pub(super) fn prompt(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    key: Key,
    cwd: PathBuf,
    refusal: io::Error,
) -> io::Result<()> {
    let unavailable = |error: &dyn std::fmt::Display| {
        io::Error::new(
            refusal.kind(),
            format!("{refusal} The trust prompt could not be opened: {error}"),
        )
    };
    let history =
        super::super::actor_runtime::terminal_history::config(home, &group.group_id, &actor.id)
            .map_err(|error| unavailable(&error))?;
    let recovery = Recovery::register(key.clone()).map_err(|error| unavailable(&error))?;
    // Capture before the prompt can record approval, even if the watcher starts late.
    let records = trust_records(&actor.env);
    let seen = signature(&records);
    let terminal = cccc_runtime::start_with_history(
        cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command: cccc_runtime::resolve_command_executable(
                &super::provider_cli::base_command(actor),
                &actor.env,
            ),
            cwd: cwd.clone(),
            env: actor.env.clone(),
            cols: 120,
            rows: 40,
        },
        history,
    )
    .map_err(|error| unavailable(&error))?;
    let watch = Watch {
        home: home.clone(),
        group: group.clone(),
        actor: actor.clone(),
        key,
        cwd,
        pid: terminal.pid,
        records,
        seen,
        recovery,
    };
    if let Err(error) = std::thread::Builder::new()
        .name("claude-workspace-trust".into())
        .spawn(move || watch.run())
    {
        let _ = cccc_runtime::stop(&group.group_id, &actor.id);
        return Err(unavailable(&error));
    }
    Ok(())
}

struct Watch {
    home: HomeLayout,
    group: GroupDoc,
    actor: Actor,
    key: Key,
    cwd: PathBuf,
    pid: Option<u32>,
    records: Vec<PathBuf>,
    seen: Vec<Option<(SystemTime, u64)>>,
    recovery: Recovery,
}

impl Watch {
    fn run(self) {
        wait_for_trust(
            POLL,
            &self.records,
            self.seen.clone(),
            || self.prompt_open(),
            || self.launch(),
        );
    }

    /// Declining the prompt exits Claude; stopping or replacing the Actor replaces this PTY.
    fn prompt_open(&self) -> bool {
        !self.recovery.cancelled()
            && cccc_runtime::status(&self.key.0, &self.key.1)
                .is_ok_and(|status| status.running && status.pid == self.pid)
    }

    /// Returns true once no further retry is useful.
    fn launch(&self) -> bool {
        workspace_trust_recovery::recover(
            &self.home,
            &self.recovery,
            || self.prompt_open(),
            || launch_managed(&self.home, &self.group, &self.actor, &self.cwd),
            |app| {
                let _ = cccc_runtime::stop(&self.key.0, &self.key.1);
                attach_managed(
                    &self.home,
                    &self.group,
                    &self.actor,
                    self.key.clone(),
                    self.cwd.clone(),
                    app,
                )?;
                super::super::actor_delivery::dispatch_group_unread(&self.home, &self.group);
                Ok(())
            },
            |app| super::block_on_managed(app.stop(app.generation())),
        )
        .unwrap_or_else(|error| {
            tracing::warn!(%error, group_id = %self.key.0, actor_id = %self.key.1,
                "managed Claude trust recovery failed");
            true
        })
    }
}

/// Retries the launch only after Claude's configuration changed, until a retry finishes or
/// the prompt closes.
fn wait_for_trust(
    poll: Duration,
    records: &[PathBuf],
    mut seen: Vec<Option<(SystemTime, u64)>>,
    prompt_open: impl Fn() -> bool,
    mut launch: impl FnMut() -> bool,
) {
    loop {
        std::thread::sleep(poll);
        if !prompt_open() {
            return;
        }
        let current = signature(records);
        if std::mem::replace(&mut seen, current.clone()) != current && launch() {
            return;
        }
    }
}

/// Where Claude may record workspace trust: its global config file, beside a configured
/// `CLAUDE_CONFIG_DIR` or in the home directory. Every candidate is watched.
fn trust_records(env: &BTreeMap<String, String>) -> Vec<PathBuf> {
    trust_records_with(env, |name| std::env::var(name).ok())
}

fn trust_records_with(
    env: &BTreeMap<String, String>,
    process: impl Fn(&str) -> Option<String>,
) -> Vec<PathBuf> {
    let mut records = Vec::new();
    for (value, file) in [
        (env.get("CLAUDE_CONFIG_DIR").cloned(), ".claude.json"),
        (process("CLAUDE_CONFIG_DIR"), ".claude.json"),
        (
            env.get("HOME").or_else(|| env.get("USERPROFILE")).cloned(),
            ".claude.json",
        ),
        (
            process("HOME").or_else(|| process("USERPROFILE")),
            ".claude.json",
        ),
    ] {
        let Some(path) = value
            .and_then(|value| cccc_core::path_input::expand_user_path(&value).ok())
            .map(|directory| directory.join(file))
        else {
            continue;
        };
        if !records.contains(&path) {
            records.push(path);
        }
    }
    records
}

fn signature(records: &[PathBuf]) -> Vec<Option<(SystemTime, u64)>> {
    records
        .iter()
        .map(|path| {
            let metadata = std::fs::metadata(path).ok()?;
            Some((metadata.modified().ok()?, metadata.len()))
        })
        .collect()
}

#[cfg(test)]
#[path = "workspace_trust_tests.rs"]
mod tests;
