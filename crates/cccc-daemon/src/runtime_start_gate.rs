use cccc_core::HomeLayout;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};

type Gates = HashMap<PathBuf, bool>;

fn gates() -> &'static Mutex<Gates> {
    static GATES: OnceLock<Mutex<Gates>> = OnceLock::new();
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct StartPermit {
    _guard: MutexGuard<'static, Gates>,
    home: PathBuf,
}

impl StartPermit {
    /// Turn the same exclusive observation boundary into shutdown admission;
    /// releasing and reacquiring the gate would let another launch slip in.
    pub(crate) fn prevent(mut self) {
        self._guard.insert(self.home, false);
    }
}

pub fn allow(home: &HomeLayout) -> Result<(), &'static str> {
    gates()
        .lock()
        .map_err(|_| "runtime start gate is poisoned")?
        .insert(home.root().to_path_buf(), true);
    Ok(())
}

pub fn prevent(home: &HomeLayout) -> Result<(), &'static str> {
    gates()
        .lock()
        .map_err(|_| "runtime start gate is poisoned")?
        .insert(home.root().to_path_buf(), false);
    Ok(())
}

pub fn permit(home: &HomeLayout) -> Result<StartPermit, &'static str> {
    let guard = gates()
        .lock()
        .map_err(|_| "runtime start gate is poisoned")?;
    if !guard.get(home.root()).copied().unwrap_or(true) {
        return Err("runtime is shutting down");
    }
    Ok(StartPermit {
        _guard: guard,
        home: home.root().to_path_buf(),
    })
}

/// Dispatch phases must never wait for an existing runtime launch while holding
/// their permits: that launch may need an MCP read to finish. A busy result lets
/// the caller release its dispatch permit before waiting for the gate.
pub(crate) fn try_permit(home: &HomeLayout) -> Result<Option<StartPermit>, &'static str> {
    let guard = match gates().try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Poisoned(_)) => return Err("runtime start gate is poisoned"),
    };
    if !guard.get(home.root()).copied().unwrap_or(true) {
        return Err("runtime is shutting down");
    }
    Ok(Some(StartPermit {
        _guard: guard,
        home: home.root().to_path_buf(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn gates_are_isolated_by_home() {
        let root = tempfile::tempdir().expect("tempdir");
        let first = HomeLayout::from_path(root.path().join("first")).expect("first home");
        let second = HomeLayout::from_path(root.path().join("second")).expect("second home");
        allow(&first).expect("allow first");
        allow(&second).expect("allow second");
        prevent(&first).expect("prevent first");

        assert!(permit(&first).is_err());
        assert!(permit(&second).is_ok());
        allow(&first).expect("restore first");
    }

    #[test]
    fn the_checked_permit_closes_its_own_home_before_releasing_the_gate() {
        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        allow(&home).expect("allow");
        let checked = try_permit(&home)
            .expect("gate")
            .expect("no launch in progress");
        assert!(try_permit(&home).expect("busy gate").is_none());
        checked.prevent();
        assert!(permit(&home).is_err());
        allow(&home).expect("restore fixture gate");
    }

    #[test]
    fn prevent_waits_for_an_in_progress_start_permit() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(root.path().join("home")).expect("home");
        allow(&home).expect("allow");
        let permit = permit(&home).expect("permit");
        let worker_home = home.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            prevent(&worker_home).expect("prevent");
            done_tx.send(()).expect("send completion");
        });

        assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(permit);
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("prevent completion");
        worker.join().expect("join prevent");
        allow(&home).expect("restore home");
    }
}
