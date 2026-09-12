use cccc_contracts::DaemonRequest;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

use crate::ops::operation::Policy;

#[derive(Clone, Default)]
pub struct DispatchLocks {
    global: Arc<RwLock<()>>,
    groups: Arc<Mutex<HashMap<String, Weak<RwLock<()>>>>>,
}

pub enum DispatchPermit {
    ResourceOwned,
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
            Access::ResourceOwned => DispatchPermit::ResourceOwned,
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

    // Background reconciliation must not queue a writer behind Actor startup:
    // startup can need another read permit for its own MCP discovery.
    pub(crate) fn try_global_write(&self) -> Option<DispatchPermit> {
        self.global
            .clone()
            .try_write_owned()
            .ok()
            .map(|guard| DispatchPermit::GlobalWrite { _guard: guard })
    }

    pub async fn group_write(&self, group_id: &str) -> DispatchPermit {
        let global = self.global.clone().read_owned().await;
        let group = self.group(group_id).write_owned().await;
        DispatchPermit::GroupWrite {
            _global: global,
            _group: group,
        }
    }

    pub(crate) fn with_group_write_blocking<T>(
        &self,
        group_id: &str,
        operation: impl FnOnce() -> T,
    ) -> T {
        let _global = self.global.blocking_read();
        let group = self.group(group_id);
        let _group = group.blocking_write();
        operation()
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
    ResourceOwned,
    GlobalRead,
    GlobalWrite,
    GroupRead(String),
    GroupWrite(String),
}

fn access(request: &DaemonRequest) -> Access {
    let policy = crate::dispatch::resolve_operation(request)
        .map_or(Policy::Write, |operation| operation.policy);
    match policy {
        Policy::ResourceOwned => return Access::ResourceOwned,
        Policy::GlobalRead => return Access::GlobalRead,
        Policy::GlobalWrite => return Access::GlobalWrite,
        Policy::Read | Policy::Write => {}
    }
    let group_id = request
        .args
        .get("group_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if request.args.contains_key("dst_group_id") {
        return Access::GlobalWrite;
    }
    match (group_id, matches!(policy, Policy::Read)) {
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
            access(&request(
                "capability_state",
                json!({"group_id":"g_one","actor_id":"peer1"})
            )),
            Access::GroupRead(group_id) if group_id == "g_one"
        ));
        assert!(matches!(
            access(&request(
                "capability_state",
                json!({"group_id":"g_one","actor_id":"peer1","view":"mcp_catalog"})
            )),
            Access::ResourceOwned
        ));
        assert!(matches!(
            access(&request(
                "term_attachment_status",
                json!({"group_id":"g_one","actor_id":"peer1","attachment_id":1})
            )),
            Access::ResourceOwned
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
        for op in ["group_bridge_session_open", "group_bridge_session_deliver"] {
            assert!(matches!(
                access(&request(op, json!({"group_id":"g_one"}))),
                Access::GlobalRead
            ));
        }
        for op in [
            "group_bridge_session_poll",
            "group_bridge_session_complete",
            "group_bridge_session_close",
            "group_bridge_session_ready",
        ] {
            assert!(
                matches!(
                    access(&request(op, json!({"group_id":"g_one"}))),
                    Access::ResourceOwned
                ),
                "{op} must use only the session runtime lock"
            );
        }
        for op in ["capability_enable", "capability_install_target"] {
            assert!(
                matches!(
                    access(&request(op, json!({"group_id":"g_one"}))),
                    Access::GlobalWrite
                ),
                "{op} mutates the global capability catalog"
            );
        }
        assert!(matches!(
            access(&request(
                "assistant_voice_recording_lease",
                json!({"group_id":"g_one","action":"acquire"})
            )),
            Access::GlobalWrite
        ));
        assert!(matches!(
            access(&request(
                "assistant_state",
                json!({"group_id":"g_one"})
            )),
            Access::GroupWrite(group_id) if group_id == "g_one"
        ));
    }

    #[test]
    fn profile_secret_key_aliases_share_the_read_policy() {
        for op in [
            "actor_profile_env_private_keys",
            "actor_profile_secret_keys",
        ] {
            assert!(matches!(
                access(&request(op, json!({}))),
                Access::GlobalRead
            ));
            assert!(matches!(
                access(&request(op, json!({"group_id":"g_one"}))),
                Access::GroupRead(group_id) if group_id == "g_one"
            ));
        }
    }

    #[test]
    fn mutation_names_that_look_like_reads_still_take_write_locks() {
        for op in [
            "group_set_state",
            "headless_set_status",
            "inbox_read",
            "ledger_snapshot",
            "runtime_wait_next_turn",
            "runtime_complete_turn",
            "web_model_browser_delivery_record",
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
    async fn status_polling_during_actor_start_does_not_block_nested_mcp_discovery() {
        use std::future::{Future, poll_fn};
        use std::task::Poll;

        for op in [
            "voice_preferences_get",
            "voice_notifications_get",
            "runtime_hermes_status",
            "group_space_provider_credential_status",
        ] {
            let locks = DispatchLocks::default();
            let _startup = locks
                .acquire(&request("actor_start", json!({"group_id":"g_one"})))
                .await;
            let poll_request = request(op, json!({}));
            let polling = locks.acquire(&poll_request);
            tokio::pin!(polling);
            let mut poll_permit = None;
            // Poll once before discovery: a misclassified GET queues a writer,
            // which prevents the startup's nested read from making progress.
            poll_fn(|cx| {
                if let Poll::Ready(permit) = polling.as_mut().poll(cx) {
                    poll_permit = Some(permit);
                }
                Poll::Ready(())
            })
            .await;
            tokio::time::timeout(
                std::time::Duration::from_millis(250),
                locks.acquire(&request(
                    "capability_state",
                    json!({
                        "group_id":"g_one", "actor_id":"worker", "view":"mcp_catalog"
                    }),
                )),
            )
            .await
            .expect("Status polling must not create an actor-start/MCP lock cycle");
            assert!(matches!(
                poll_permit,
                Some(DispatchPermit::GlobalRead { .. })
            ));
        }
        for op in ["voice_preferences_set", "voice_messages_viewed"] {
            assert!(matches!(
                access(&request(op, json!({}))),
                Access::GlobalWrite
            ));
        }
    }

    #[tokio::test]
    async fn startup_catalog_discovery_can_finish_with_a_global_writer_queued() {
        use std::future::{Future, poll_fn};
        use std::task::Poll;

        let locks = DispatchLocks::default();
        let startup = locks.group_write("g_one").await;
        let update = request("settings_update", json!({}));
        let writer = locks.acquire(&update);
        tokio::pin!(writer);
        poll_fn(|cx| {
            assert!(writer.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;

        for op in [
            "capability_state",
            "term_attachment_status",
            "terminal_write",
            "group_bridge_session_poll",
            "group_bridge_session_complete",
            "group_bridge_session_close",
            "group_bridge_session_ready",
        ] {
            tokio::time::timeout(
                std::time::Duration::from_millis(250),
                locks.acquire(&request(
                    op,
                    json!({"group_id":"g_one","actor_id":"peer","view":"mcp_catalog"}),
                )),
            )
            .await
            .unwrap_or_else(|_| {
                panic!("{op} must not queue behind a writer waiting for its caller")
            });
        }
        // Discovery is independent; the actual mutation still waits for the
        // lifecycle operation to complete.
        poll_fn(|cx| {
            assert!(writer.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(startup);
        tokio::time::timeout(std::time::Duration::from_millis(250), writer)
            .await
            .expect("global mutation proceeds after startup releases its permit");
    }

    #[tokio::test]
    async fn capability_catalog_can_be_read_while_runtime_start_holds_the_group_lock() {
        let locks = DispatchLocks::default();
        let _runtime_start = locks.group_write("g_one").await;

        tokio::time::timeout(
            std::time::Duration::from_millis(50),
            locks.acquire(&request(
                "capability_state",
                json!({"group_id":"g_one","actor_id":"peer1","view":"mcp_catalog"}),
            )),
        )
        .await
        .expect("managed runtime MCP discovery must not deadlock actor startup");
    }

    #[test]
    fn presentation_mutations_are_serialized_as_group_writes() {
        for op in ["presentation_publish", "presentation_clear"] {
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
