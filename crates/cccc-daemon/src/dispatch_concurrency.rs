use cccc_contracts::DaemonRequest;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

mod operation_access;
use operation_access::{is_global_write, is_read_only};

#[derive(Clone, Default)]
pub struct DispatchLocks {
    global: Arc<RwLock<()>>,
    groups: Arc<Mutex<HashMap<String, Weak<RwLock<()>>>>>,
}

pub enum DispatchPermit {
    GlobalRead {
        _guard: OwnedRwLockReadGuard<()>,
    },
    GlobalWrite {
        _guard: OwnedRwLockWriteGuard<()>,
    },
    GroupRead {
        _global: OwnedRwLockReadGuard<()>,
        _group: OwnedRwLockReadGuard<()>,
    },
    GroupWrite {
        _global: OwnedRwLockReadGuard<()>,
        _group: OwnedRwLockWriteGuard<()>,
    },
}

impl DispatchLocks {
    pub async fn acquire(&self, request: &DaemonRequest) -> DispatchPermit {
        match access(request) {
            Access::GlobalRead => DispatchPermit::GlobalRead {
                _guard: self.global.clone().read_owned().await,
            },
            Access::GlobalWrite => DispatchPermit::GlobalWrite {
                _guard: self.global.clone().write_owned().await,
            },
            Access::GroupRead(group_id) => {
                let global = self.global.clone().read_owned().await;
                let group = self.group(&group_id).read_owned().await;
                DispatchPermit::GroupRead {
                    _global: global,
                    _group: group,
                }
            }
            Access::GroupWrite(group_id) => {
                let global = self.global.clone().read_owned().await;
                let group = self.group(&group_id).write_owned().await;
                DispatchPermit::GroupWrite {
                    _global: global,
                    _group: group,
                }
            }
        }
    }

    pub async fn global_read(&self) -> DispatchPermit {
        DispatchPermit::GlobalRead {
            _guard: self.global.clone().read_owned().await,
        }
    }

    pub async fn group_write(&self, group_id: &str) -> DispatchPermit {
        let global = self.global.clone().read_owned().await;
        let group = self.group(group_id).write_owned().await;
        DispatchPermit::GroupWrite {
            _global: global,
            _group: group,
        }
    }

    fn group(&self, group_id: &str) -> Arc<RwLock<()>> {
        let mut groups = self
            .groups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(lock) = groups.get(group_id).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(RwLock::new(()));
        groups.insert(group_id.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

enum Access {
    GlobalRead,
    GlobalWrite,
    GroupRead(String),
    GroupWrite(String),
}

fn access(request: &DaemonRequest) -> Access {
    let group_id = request
        .args
        .get("group_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if is_global_write(request) {
        return Access::GlobalWrite;
    }
    match (group_id, is_read_only(&request.op)) {
        (Some(group_id), true) => Access::GroupRead(group_id),
        (Some(group_id), false) => Access::GroupWrite(group_id),
        (None, true) => Access::GlobalRead,
        (None, false) => Access::GlobalWrite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    fn request(op: &str, args: serde_json::Value) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        }
    }

    #[test]
    fn classifies_group_reads_and_writes_without_relaxing_global_mutations() {
        assert!(matches!(
            access(&request("ledger_tail", json!({"group_id":"g_one"}))),
            Access::GroupRead(group_id) if group_id == "g_one"
        ));
        assert!(matches!(
            access(&request(
                "terminal_snapshot",
                json!({"group_id":"g_one","actor_id":"peer1"})
            )),
            Access::GroupRead(group_id) if group_id == "g_one"
        ));
        assert!(matches!(
            access(&request(
                "terminal_replay",
                json!({"group_id":"g_one","actor_id":"peer1"})
            )),
            Access::GroupRead(group_id) if group_id == "g_one"
        ));
        assert!(matches!(
            access(&request("send", json!({"group_id":"g_one"}))),
            Access::GroupWrite(group_id) if group_id == "g_one"
        ));
        assert!(matches!(
            access(&request("group_delete", json!({"group_id":"g_one"}))),
            Access::GlobalWrite
        ));
        assert!(matches!(
            access(&request(
                "send_cross_group",
                json!({"group_id":"g_one","dst_group_id":"g_two"})
            )),
            Access::GlobalWrite
        ));
        for op in ["capability_install", "capability_install_target"] {
            assert!(
                matches!(
                    access(&request(op, json!({"group_id":"g_one"}))),
                    Access::GlobalWrite
                ),
                "{op} mutates the global capability catalog"
            );
        }
    }

    #[test]
    fn mutation_names_that_look_like_reads_still_take_write_locks() {
        for op in [
            "group_set_state",
            "headless_set_status",
            "inbox_mark_read",
            "inbox_mark_all_read",
            "ledger_snapshot",
            "runtime_wait_next_turn",
            "web_model_runtime_wait_next_turn",
            "runtime_complete_turn",
            "web_model_runtime_complete_turn",
            "future_unknown_operation",
        ] {
            assert!(
                matches!(
                    access(&request(op, json!({"group_id":"g_one"}))),
                    Access::GroupWrite(group_id) if group_id == "g_one"
                ),
                "{op} must be serialized as a group write"
            );
        }
    }

    #[tokio::test]
    async fn group_writes_are_isolated_without_blocking_other_groups() {
        let locks = DispatchLocks::default();
        let first = locks.group_write("g_one").await;
        let second = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            locks.group_write("g_two"),
        )
        .await;
        assert!(second.is_ok(), "another group should remain available");

        let shutdown = request("shutdown", json!({}));
        let global = tokio::time::timeout(
            std::time::Duration::from_millis(20),
            locks.acquire(&shutdown),
        )
        .await;
        assert!(global.is_err(), "global writes must wait for active groups");
        drop(first);
    }
}
