use super::{Fixture, NativeSession};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// Keep the attached receiver alive while a blocked PTY write outlasts its
/// lease. Real receivers send these heartbeats while the old owner stops.
pub(super) fn while_connected<T>(
    fixture: &Fixture,
    source: &NativeSession,
    action: impl FnOnce() -> T,
) -> T {
    std::thread::scope(|scope| {
        let (stop, stopped) = mpsc::channel::<()>();
        let heartbeat = scope.spawn(move || {
            while matches!(
                stopped.recv_timeout(Duration::from_secs(1)),
                Err(RecvTimeoutError::Timeout)
            ) {
                let response = fixture.request(
                    "local_session_receiver_heartbeat",
                    Fixture::receiver(source),
                );
                assert!(response.ok, "receiver heartbeat: {:?}", response.error);
            }
        });
        let result = action();
        drop(stop);
        heartbeat.join().expect("receiver heartbeat");
        result
    })
}
