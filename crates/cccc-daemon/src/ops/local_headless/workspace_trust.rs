//! A detached Claude launch can require workspace trust or a bypass disclaimer.
//! Keep Claude's own confirmation in the Actor terminal until a managed retry succeeds.

use super::supervisor::{Key, attach_managed, launch_managed};
use super::workspace_trust_recovery::{self, Recovery};
use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use cccc_core::{GroupDoc, HomeLayout};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const POLL: Duration = Duration::from_secs(1);

pub(super) fn refused(actor: &Actor, error: &io::Error) -> bool {
    actor.runtime == ActorRuntime::Claude
        && super::super::codex_voice_analyst::claude_confirmation_launch(error).is_some()
}

/// Opens the configured Claude command interactively. Only Claude records the user's decision.
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
            format!("{refusal} The confirmation prompt could not be opened: {error}"),
        )
    };
    let history =
        super::super::actor_runtime::terminal_history::config(home, &group.group_id, &actor.id)
            .map_err(|error| unavailable(&error))?;
    let recovery = Recovery::register(key.clone()).map_err(|error| unavailable(&error))?;
    // Capture before the prompt can record approval, even if the watcher starts late.
    let launch = super::super::codex_voice_analyst::claude_confirmation_launch(&refusal);
    let (command, environment, records) = if let Some(launch) = launch {
        let mut records = trust_records(&launch.environment);
        records.push(launch.config_dir.join("settings.json"));
        records.push(launch.config_dir.join(".config.json"));
        let mut command = vec![launch.executable.clone()];
        command.extend(launch.arguments.clone());
        command.push("--dangerously-skip-permissions".into());
        (command, launch.environment.clone(), records)
    } else {
        // Existing isolated trust fixtures without a provider refusal retain their own command.
        (
            cccc_runtime::resolve_command_executable(
                &super::provider_cli::base_command(actor),
                &actor.env,
            ),
            actor.env.clone(),
            trust_records(&actor.env),
        )
    };
    let seen = signature(&records);
    let terminal = cccc_runtime::start_with_history_isolated_environment(
        cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command,
            cwd: cwd.clone(),
            env: environment,
            cols: 120,
            rows: 40,
        },
        history,
    )
    .map_err(|error| unavailable(&error))?;
    if let Err(error) = super::confirmation_terminal::protect(&terminal) {
        let _ = cccc_runtime::stop(&group.group_id, &actor.id);
        return Err(unavailable(&error));
    }
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
        failed: Cell::new(false),
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
    failed: Cell<bool>,
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
        // The confirmation terminal may have exited, or a genuine retry failed.
        // Publish only while this recovery still owns the generation; never reinterpret
        // an exit code or changed file as consent, and never publish opaque CLI stderr.
        if !super::supervisor::running(&self.key.0, &self.key.1)
            && self
                .recovery
                .finish_if_current(!self.failed.get())
                .unwrap_or(false)
        {
            let _ = super::events::append(
                &self.home,
                &self.key.0,
                &self.key.1,
                "headless.session.exited",
                serde_json::Map::from_iter([(
                    "reason".into(),
                    serde_json::json!("interactive_confirmation_ended"),
                )]),
            );
        }
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
            || {
                let result = launch_managed(&self.home, &self.group, &self.actor, &self.cwd);
                if result
                    .as_ref()
                    .is_err_and(|error| !refused(&self.actor, error))
                {
                    self.failed.set(true);
                    self.recovery.retire_failed_start();
                    // The start guard still owns this exact prompt. Other launch
                    // failures are not trust decisions; retire the temporary PTY
                    // so status/UI expose the failed managed startup.
                    if self.prompt_open() {
                        cccc_runtime::stop(&self.key.0, &self.key.1).map_err(|_| {
                            io::Error::other("Could not close the failed Claude trust prompt")
                        })?;
                    }
                }
                result
            },
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
            // A real startup/resume failure keeps its existing retry/receipt policy.
            // It is not the operator declining or closing an interactive confirmation.
            self.failed.set(true);
            self.recovery.retire_failed_start();
            tracing::warn!(%error, group_id = %self.key.0, actor_id = %self.key.1,
                "managed Claude confirmation recovery failed");
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
        (
            env.get("CLAUDE_CONFIG_DIR")
                .cloned()
                .or_else(|| process("CLAUDE_CONFIG_DIR")),
            ".claude.json",
        ),
        (
            env.get("HOME")
                .or_else(|| env.get("USERPROFILE"))
                .cloned()
                .or_else(|| process("HOME").or_else(|| process("USERPROFILE"))),
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
