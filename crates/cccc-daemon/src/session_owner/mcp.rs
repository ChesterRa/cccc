//! Main MCP observations live with the session owner, across daemon lifetimes.
//! A new launch never overwrites observations of still-existing native peers.

use super::protocol::{ActorRef, Failure};
use cccc_core::local_sessions::NativeHost;
use cccc_core::owned_mcp::{Binding, Registration, binding_file};
use cccc_core::{GroupDoc, HomeLayout};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_PROCESSES: usize = 4096;
type Key = (String, String);

#[derive(Default)]
pub(super) struct Registry {
    owner: Option<(HomeLayout, String, String)>,
    launches: BTreeMap<Key, Launch>,
    processes: BTreeMap<String, Process>,
}

struct Launch {
    actor: ActorRef,
    binding: Binding,
    file: std::path::PathBuf,
    initialized: bool,
    source_controlled: bool,
}

struct Process {
    actor: ActorRef,
    observed: ObservedProcess,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub binding: Binding,
    /// This launch has completed at least one registration. This alone is not
    /// proof that a provider adopted this configuration or has no other MCP.
    pub initialized: bool,
    /// Captured from the actual prepared launch, never from mutable settings.
    #[serde(default)]
    pub source_controlled: Option<bool>,
    pub processes: Vec<ObservedProcess>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ObservedProcess {
    pub native: NativeHost,
    pub report: Registration,
    /// Inaccessible native metadata is retained as unknown, never as exited.
    pub observable: bool,
}

impl Registry {
    pub(super) fn new(home: HomeLayout, owner_id: String, instance: String) -> Self {
        Self {
            owner: Some((home, owner_id, instance)),
            ..Self::default()
        }
    }

    pub(super) fn reserve(
        &mut self,
        actor: &ActorRef,
        group: &GroupDoc,
        source_controlled: bool,
    ) -> Result<(), Failure> {
        let Some((home, owner_id, instance)) = &self.owner else {
            return Ok(());
        };
        let binding = Binding {
            owner_id: owner_id.clone(),
            instance: instance.clone(),
            launch_id: uuid::Uuid::new_v4().simple().to_string(),
        };
        let file = binding_file(home, group, &actor.actor_id, &actor.actor_created_at)
            .map_err(|error| Failure::new("mcp_binding_unavailable", error.to_string()))?;
        // The caller holds the registry mutex until the active launch is in
        // place, so a new peer reading this publication cannot register early.
        cccc_core::fs::write_secret_json(&file, &binding)
            .map_err(|error| Failure::new("mcp_binding_unavailable", error.to_string()))?;
        self.launches.insert(
            key(actor),
            Launch {
                actor: actor.clone(),
                binding,
                file,
                initialized: false,
                source_controlled,
            },
        );
        Ok(())
    }

    pub(super) fn file(&self, group: &str, actor: &str) -> Option<std::path::PathBuf> {
        self.launches
            .get(&(group.into(), actor.into()))
            .map(|launch| launch.file.clone())
    }

    #[cfg(test)]
    pub(super) fn binding(&self, group: &str, actor: &str) -> Option<Binding> {
        self.launches
            .get(&(group.into(), actor.into()))
            .map(|launch| launch.binding.clone())
    }

    pub(super) fn forget(&mut self, actor: &ActorRef) {
        // Observed peers remain until positive exit evidence, even if Stop or
        // another Start has retired the launch that originally registered them.
        self.launches.remove(&key(actor));
        self.refresh();
    }

    pub(super) fn register(
        &mut self,
        native: NativeHost,
        report: Registration,
    ) -> Result<(), Failure> {
        report
            .validate()
            .map_err(|error| Failure::new("invalid_mcp_registration", error.to_string()))?;
        self.refresh();
        let launch = self
            .launches
            .values_mut()
            .find(|launch| {
                launch.binding.instance == report.instance
                    && launch.binding.launch_id == report.launch_id
            })
            .ok_or_else(|| {
                Failure::new(
                    "mcp_launch_unknown",
                    "the MCP binding is not an active launch of this session owner",
                )
            })?;
        let native_key = native.key();
        if let Some(previous) = self.processes.get(&native_key) {
            if previous.observed.report != report {
                return Err(Failure::new(
                    "mcp_identity_changed",
                    "this native MCP process already registered another build or launch; restart the member",
                ));
            }
        } else {
            if self.processes.len() >= MAX_PROCESSES {
                return Err(Failure::new(
                    "mcp_capacity",
                    "too many unconfirmed MCP processes; stop unused members before starting another MCP",
                ));
            }
            self.processes.insert(
                native_key,
                Process {
                    actor: launch.actor.clone(),
                    observed: ObservedProcess {
                        native,
                        report,
                        observable: true,
                    },
                },
            );
        }
        launch.initialized = true;
        Ok(())
    }

    pub(super) fn snapshot(&self, actor: &ActorRef) -> Option<Snapshot> {
        let launch = self.launches.get(&key(actor))?;
        if launch.actor.actor_created_at != actor.actor_created_at {
            return None;
        }
        Some(Snapshot {
            binding: launch.binding.clone(),
            initialized: launch.initialized,
            source_controlled: Some(launch.source_controlled),
            processes: self
                .processes
                .values()
                .filter(|process| {
                    key(&process.actor) == key(actor)
                        && process.actor.actor_created_at == actor.actor_created_at
                })
                .map(|process| process.observed.clone())
                .collect(),
        })
    }

    pub(super) fn refresh(&mut self) {
        self.processes.retain(|_, process| {
            match NativeHost::capture(process.observed.native.pid) {
                Ok(current) if current != process.observed.native => false,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                observation => {
                    process.observed.observable = observation.is_ok();
                    true
                }
            }
        });
    }
}

fn key(actor: &ActorRef) -> Key {
    (actor.group_id.clone(), actor.actor_id.clone())
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
