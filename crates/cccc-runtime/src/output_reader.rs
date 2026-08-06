use crate::RuntimeError;
use crate::session_history::SessionHistory;
use std::io::Read;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);

pub(crate) struct OutputReader {
    finished: Receiver<()>,
    handle: Option<JoinHandle<()>>,
}

impl OutputReader {
    pub(crate) fn start(
        name: String,
        mut reader: Box<dyn Read + Send>,
        history: SessionHistory,
    ) -> std::io::Result<Self> {
        let (finished_tx, finished) = mpsc::channel();
        let handle = std::thread::Builder::new().name(name).spawn(move || {
            copy_output(reader.as_mut(), &history);
            let _ = finished_tx.send(());
        })?;
        Ok(Self {
            finished,
            handle: Some(handle),
        })
    }

    pub(crate) fn finish(mut self) -> Result<bool, RuntimeError> {
        match self.finished.recv_timeout(OUTPUT_DRAIN_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                self.join()?;
                Ok(true)
            }
            Err(RecvTimeoutError::Timeout) => Ok(false),
        }
    }

    fn join(&mut self) -> Result<(), RuntimeError> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        handle
            .join()
            .map_err(|_| std::io::Error::other("terminal output reader panicked").into())
    }
}

fn copy_output(reader: &mut dyn Read, history: &SessionHistory) {
    let mut buffer = [0_u8; 8192];
    let mut history_error_reported = false;
    while let Ok(count) = reader.read(&mut buffer) {
        if count == 0 {
            break;
        }
        if let Err(error) = history.push(&buffer[..count])
            && !history_error_reported
        {
            eprintln!(
                "CCCC terminal history write failed; continuing to drain PTY output: {error}"
            );
            history_error_reported = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OutputReader;
    use crate::session_history::SessionHistory;
    use crate::transcript_archive::HistoryConfig;
    use std::collections::VecDeque;
    use std::io::Read;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    struct HeldOpenReader {
        released: Arc<(Mutex<bool>, Condvar)>,
    }

    struct ChunkReader {
        chunks: VecDeque<Vec<u8>>,
    }

    impl Read for HeldOpenReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            let (lock, wake) = &*self.released;
            let mut released = lock.lock().expect("lock");
            while !*released {
                released = wake.wait(released).expect("wait");
            }
            Ok(0)
        }
    }

    impl Read for ChunkReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let Some(chunk) = self.chunks.pop_front() else {
                return Ok(0);
            };
            buffer[..chunk.len()].copy_from_slice(&chunk);
            Ok(chunk.len())
        }
    }

    #[test]
    fn finish_is_bounded_when_the_read_end_remains_open() {
        let released = Arc::new((Mutex::new(false), Condvar::new()));
        let reader = OutputReader::start(
            "held-open-reader".into(),
            Box::new(HeldOpenReader {
                released: Arc::clone(&released),
            }),
            SessionHistory::new(None).expect("history"),
        )
        .expect("reader");

        let started = Instant::now();
        reader.finish().expect("finish");
        assert!(started.elapsed() < Duration::from_secs(1));

        let (lock, wake) = &*released;
        *lock.lock().expect("lock") = true;
        wake.notify_all();
    }

    #[cfg(unix)]
    #[test]
    fn archive_failure_does_not_stop_pty_drain() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("history");
        let history = SessionHistory::new(Some(HistoryConfig {
            path: root.join("session.pty"),
            max_bytes: 1024,
            hot_bytes: 1024,
            persist: true,
        }))
        .expect("history");
        std::fs::remove_dir_all(&root).expect("remove archive directory");
        let reader = OutputReader::start(
            "failing-archive-reader".into(),
            Box::new(ChunkReader {
                chunks: VecDeque::from([b"first".to_vec(), b" second".to_vec()]),
            }),
            history.clone(),
        )
        .expect("reader");

        reader.finish().expect("finish");

        assert_eq!(
            history.retained_page().expect("hot history").data,
            "first second"
        );
    }
}
