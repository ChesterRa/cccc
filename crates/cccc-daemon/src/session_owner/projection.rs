//! Explicit synchronous observations share one snapshot per owner instance.
//! Outside this scope, every observation still reads the owner directly.

use super::protocol::ActorSnapshot;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;

type Snapshot = Result<Vec<ActorSnapshot>, (io::ErrorKind, String)>;
thread_local! {
    static SCOPE: RefCell<Option<BTreeMap<String, Snapshot>>> = const { RefCell::new(None) };
}

pub(super) fn read_scope<T>(read: impl FnOnce() -> T) -> T {
    let outer = SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        if scope.is_some() {
            false
        } else {
            *scope = Some(BTreeMap::new());
            true
        }
    });
    let _guard = Scope { outer };
    read()
}

pub(super) fn get(instance: &str) -> Option<io::Result<Vec<ActorSnapshot>>> {
    SCOPE
        .with(|scope| scope.borrow().as_ref()?.get(instance).cloned())
        .map(|snapshot| snapshot.map_err(|(kind, message)| io::Error::new(kind, message)))
}

pub(super) fn remember(instance: &str, snapshot: &io::Result<Vec<ActorSnapshot>>) {
    SCOPE.with(|scope| {
        if let Some(scope) = scope.borrow_mut().as_mut() {
            let value = match snapshot {
                Ok(actors) => Ok(actors.clone()),
                Err(error) => Err((error.kind(), error.to_string())),
            };
            scope.insert(instance.into(), value);
        }
    });
}

pub(super) fn invalidate() {
    SCOPE.with(|scope| {
        if let Some(scope) = scope.borrow_mut().as_mut() {
            scope.clear();
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
}
