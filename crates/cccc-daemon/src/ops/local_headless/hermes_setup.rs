//! A Hermes profile without a provider cannot create an ACP session, and Hermes
//! then advertises its own setup as an ACP terminal method. Run that setup in
//! the member's terminal, and connect once when it exits.

use super::supervisor::{Key, StartGuard, attach_managed, launch_managed};
use super::workspace_trust_recovery::Recovery;
use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use cccc_core::{GroupDoc, HomeLayout};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(1);

/// Only a member with a terminal runs the setup; in ACP mode the start fails
/// with the setup command in its error.
pub(super) fn needed(actor: &Actor, error: &io::Error) -> bool {
    actor.runtime == ActorRuntime::Hermes
        && !actor.runtime.is_headless_acp(actor.runtime_mode)
        && super::super::codex_voice_analyst::hermes_setup(error).is_some()
}

/// Opens the setup in the member's terminal. Messages wait, as for a Claude
/// confirmation, until the member connects.
pub(super) fn open(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    key: Key,
    cwd: PathBuf,
    refusal: io::Error,
) -> io::Result<()> {
    let Some(setup) = super::super::codex_voice_analyst::hermes_setup(&refusal) else {
        return Err(refusal);
    };
    let unavailable = |error: &dyn std::fmt::Display| {
        io::Error::new(
            refusal.kind(),
            format!("{refusal} The setup could not be opened in the agent's terminal: {error}"),
        )
    };
    let history =
        super::super::actor_runtime::terminal_history::config(home, &group.group_id, &actor.id)
            .map_err(|error| unavailable(&error))?;
    let recovery = Recovery::register(key.clone()).map_err(|error| unavailable(&error))?;
    let terminal = cccc_runtime::start_with_history(
        cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command: setup.command.clone(),
            cwd: cwd.clone(),
            env: setup.environment.clone(),
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
        recovery,
    };
    if let Err(error) = std::thread::Builder::new()
        .name("hermes-setup".into())
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
    recovery: Recovery,
}

impl Watch {
    fn run(self) {
        while self.setup_open() {
            std::thread::sleep(POLL);
        }
        // Stopping or replacing the member cancels this recovery.
        if self.recovery.cancelled() {
            return;
        }
        // The setup exited; its exit status says nothing reliable about the
        // profile. Connect once, as the member's start would. When that fails,
        // messages keep waiting and only the user's next start tries again.
        if let Err(error) = self.connect() {
            tracing::warn!(%error, group_id = %self.key.0, actor_id = %self.key.1,
                "managed Hermes did not connect after its setup");
            if self.recovery.finish_if_current(true).unwrap_or(false) {
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
    }

    fn setup_open(&self) -> bool {
        !self.recovery.cancelled()
            && cccc_runtime::status(&self.key.0, &self.key.1)
                .is_ok_and(|status| status.running && status.pid == self.pid)
    }

    fn connect(&self) -> io::Result<()> {
        // Match actor_runtime::start_local_headless: permit MUST precede StartGuard.
        let Ok(_permit) = crate::runtime_start_gate::permit(&self.home) else {
            return Ok(());
        };
        let _start = StartGuard::acquire(&self.key)?;
        if self.recovery.cancelled() {
            return Ok(());
        }
        let app = launch_managed(&self.home, &self.group, &self.actor, &self.cwd)?;
        if self.recovery.cancelled() {
            return super::block_on_managed(app.stop(app.generation()));
        }
        // The exited setup terminal makes way for the member's own.
        let _ = cccc_runtime::stop(&self.key.0, &self.key.1);
        attach_managed(
            &self.home,
            &self.group,
            &self.actor,
            self.key.clone(),
            self.cwd.clone(),
            app,
        )?;
        if !crate::session_owner::is_owner() {
            super::super::actor_delivery::dispatch_group_unread(&self.home, &self.group);
        }
        Ok(())
    }
}
