use cccc_runtime::OwnedProcessTree;
use std::io;
use std::process::Child;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STOP_TIMEOUT: Duration = Duration::from_secs(2);

pub(in crate::ops::codex_voice_analyst) struct ChildOwner {
    child: Mutex<Option<Child>>,
    process_tree: OwnedProcessTree,
}

impl ChildOwner {
    pub(in crate::ops::codex_voice_analyst) fn new(
        child: Child,
        process_tree: OwnedProcessTree,
    ) -> Self {
        Self {
            child: Mutex::new(Some(child)),
            process_tree,
        }
    }

    pub(in crate::ops::codex_voice_analyst) fn stop(&self) -> io::Result<()> {
        self.stop_with(stop_child)
    }

    fn stop_with(
        &self,
        operation: impl FnOnce(&mut Child, &OwnedProcessTree) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut guard = self
            .child
            .lock()
            .map_err(|_| io::Error::other("managed Agent child lock poisoned"))?;
        let Some(child) = guard.as_mut() else {
            return Ok(());
        };
        // Keep ownership on every failure, including signal, poll and reap
        // errors. Concurrent stop calls cannot mistake an in-flight stop for
        // success. The emergency registry never needs this child mutex.
        operation(child, &self.process_tree)?;
        guard.take();
        Ok(())
    }

    pub(in crate::ops::codex_voice_analyst) fn running(&self) -> bool {
        self.child
            .lock()
            .ok()
            .and_then(|mut child| {
                child
                    .as_mut()
                    .map(|child| self.process_tree.try_wait(|| child.try_wait()))
            })
            .is_some_and(|status| status.ok().flatten().is_none())
    }

    pub(in crate::ops::codex_voice_analyst) fn id(&self) -> Option<u32> {
        self.child
            .lock()
            .ok()
            .and_then(|child| child.as_ref().map(Child::id))
    }
}

impl Drop for ChildOwner {
    fn drop(&mut self) {
        let child = self.child.get_mut().ok().and_then(Option::take);
        if let Some(mut child) = child {
            let _ = self.process_tree.terminate();
            let _ = child.wait();
        }
    }
}

fn stop_child(child: &mut Child, tree: &OwnedProcessTree) -> io::Result<()> {
    if tree.try_wait(|| child.try_wait())?.is_none() {
        tree.request_stop()?;
        if !wait_bounded(child, tree, STOP_TIMEOUT)? {
            tree.terminate()?;
        }
    }
    child.wait()?;
    Ok(())
}

fn wait_bounded(child: &mut Child, tree: &OwnedProcessTree, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        if tree.try_wait(|| child.try_wait())?.is_some() {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(all(test, unix))]
#[path = "child_tests.rs"]
mod tests;
