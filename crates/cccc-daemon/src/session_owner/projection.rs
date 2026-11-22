//! Explicit synchronous observations share one snapshot per owner instance.
//! Outside this scope, every observation still reads the owner directly.

use super::protocol::ActorSnapshot;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) type Snapshot = Result<Vec<ActorSnapshot>, (io::ErrorKind, String)>;
pub(super) type Snapshots = BTreeMap<String, Snapshot>;

#[derive(Default)]
struct Observations {
    snapshots: Arc<Snapshots>,
    // A captured maintenance pass cannot fall through to a live read.
    home: Option<PathBuf>,
}

thread_local! {
    static SCOPE: RefCell<Option<Observations>> = const { RefCell::new(None) };
}

pub(super) fn read_scope<T>(read: impl FnOnce() -> T) -> T {
    let outer = SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        if scope.is_some() {
            false
        } else {
            *scope = Some(Observations::default());
            true
        }
    });
    let _guard = Scope { outer };
    read()
}

pub(super) fn captured<T>(home: &Path, snapshots: Arc<Snapshots>, read: impl FnOnce() -> T) -> T {
    struct Restore(Option<Observations>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCOPE.with(|scope| scope.replace(self.0.take()));
        }
    }

    let previous = SCOPE.with(|scope| {
        scope.replace(Some(Observations {
            snapshots,
            home: Some(home.to_path_buf()),
        }))
    });
    let _restore = Restore(previous);
    read()
}

pub(super) fn captured_home() -> Option<PathBuf> {
    SCOPE.with(|scope| scope.borrow().as_ref()?.home.clone())
}

pub(super) fn get(instance: &str) -> Option<io::Result<Vec<ActorSnapshot>>> {
    SCOPE.with(|scope| {
        let scope = scope.borrow();
        let scope = scope.as_ref()?;
        match scope.snapshots.get(instance) {
            Some(snapshot) => Some(
                snapshot
                    .clone()
                    .map_err(|(kind, message)| io::Error::new(kind, message)),
            ),
            None if scope.home.is_some() => Some(Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "session owner was not observed in this maintenance pass",
            ))),
            None => None,
        }
    })
}

pub(super) fn capture(snapshot: io::Result<Vec<ActorSnapshot>>) -> Snapshot {
    snapshot.map_err(|error| (error.kind(), error.to_string()))
}

pub(super) fn remember(instance: &str, snapshot: &io::Result<Vec<ActorSnapshot>>) {
    SCOPE.with(|scope| {
        if let Some(scope) = scope.borrow_mut().as_mut() {
            let value = match snapshot {
                Ok(actors) => Ok(actors.clone()),
                Err(error) => Err((error.kind(), error.to_string())),
            };
            Arc::make_mut(&mut scope.snapshots).insert(instance.into(), value);
        }
    });
}

pub(super) fn invalidate() {
    SCOPE.with(|scope| {
        if let Some(scope) = scope.borrow_mut().as_mut() {
            scope.snapshots = Arc::default();
        }
    });
}

struct Scope {
    outer: bool,
}
impl Drop for Scope {
    fn drop(&mut self) {
        if self.outer {
            SCOPE.with(|scope| {
                scope.borrow_mut().take();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_shared_only_within_the_explicit_observation() {
        assert!(get("owner").is_none());
        read_scope(|| {
            remember("owner", &Ok(Vec::new()));
            read_scope(|| assert!(get("owner").is_some()));
            assert!(get("owner").is_some());
            invalidate();
            assert!(get("owner").is_none());
            remember(
                "owner",
                &Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "owner unavailable",
                )),
            );
            assert_eq!(
                get("owner")
                    .expect("registered fixture entry")
                    .err()
                    .expect("unavailable snapshot")
                    .kind(),
                io::ErrorKind::NotConnected
            );
        });
        assert!(get("owner").is_none());
    }

    #[test]
    fn unwinding_never_carries_a_snapshot_to_the_next_request() {
        let _ = std::panic::catch_unwind(|| {
            read_scope(|| {
                remember("owner", &Ok(Vec::new()));
                panic!("test request failed");
            })
        });
        assert!(get("owner").is_none());
    }

    #[test]
    fn captured_observations_refuse_live_fallback_after_invalidation() {
        captured(
            Path::new("/captured"),
            Arc::new(BTreeMap::from([("known".into(), Ok(Vec::new()))])),
            || {
                assert!(get("known").expect("captured owner snapshot").is_ok());
                assert!(get("new-owner").expect("missing owner is fenced").is_err());
                invalidate();
                assert!(get("known").expect("invalidated owner is fenced").is_err());
            },
        );
        assert!(get("known").is_none());
    }
}
