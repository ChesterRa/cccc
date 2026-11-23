//! A single Home-scoped test gate; no product build or provider I/O uses it.
use super::*;
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock, mpsc},
    time::Duration,
};

struct Pending {
    home: PathBuf,
    reached: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
fn slot() -> &'static Mutex<Option<Pending>> {
    static SLOT: OnceLock<Mutex<Option<Pending>>> = OnceLock::new();
    SLOT.get_or_init(Mutex::default)
}

pub(super) struct Guard {
    home: PathBuf,
    release: Option<mpsc::Sender<()>>,
}
impl Guard {
    pub(super) fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.release();
        let mut pending = slot().lock().expect("gate registry");
        if pending
            .as_ref()
            .is_some_and(|pending| pending.home == self.home)
        {
            pending.take();
        }
    }
}

pub(super) fn install(home: &HomeLayout) -> (Guard, mpsc::Receiver<()>) {
    let (reached, receiver) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut pending = slot().lock().expect("gate registry");
    assert!(
        pending.is_none(),
        "only one controlled test may install a gate"
    );
    *pending = Some(Pending {
        home: home.root().to_owned(),
        reached,
        release: wait,
    });
    (
        Guard {
            home: home.root().to_owned(),
            release: Some(release),
        },
        receiver,
    )
}

pub(super) fn after_pause(home: &HomeLayout) {
    let pending = {
        let mut pending = slot().lock().expect("gate registry");
        if pending
            .as_ref()
            .is_some_and(|pending| pending.home == home.root())
        {
            pending.take()
        } else {
            None
        }
    };
    if let Some(pending) = pending {
        let _ = pending.reached.send(());
        pending
            .release
            .recv_timeout(Duration::from_secs(5))
            .expect("test gate released within deadline");
    }
}

pub(super) struct Task<T>(Option<tokio::task::JoinHandle<T>>);
impl<T> Task<T> {
    pub(super) fn new(task: tokio::task::JoinHandle<T>) -> Self {
        Self(Some(task))
    }
    pub(super) async fn finish(&mut self) -> T {
        let task = self.0.as_mut().expect("owned task");
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("bounded task completion");
        self.0.take();
        result.expect("task result")
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}
