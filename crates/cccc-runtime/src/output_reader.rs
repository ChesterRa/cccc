use crate::RuntimeError;
use crate::session_history::SessionHistory;
use crate::terminal_response_writer::{
    SharedPtyWriter, TerminalResponseSender, TerminalResponseWriter,
};
use std::io::Read;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);

pub(crate) struct OutputReader {
    finished: Receiver<()>,
    handle: Option<JoinHandle<()>>,
    response_writer: Option<TerminalResponseWriter>,
}

impl OutputReader {
    pub(crate) fn start(
        name: String,
        mut reader: Box<dyn Read + Send>,
        history: SessionHistory,
        writer: SharedPtyWriter,
        input_gate: Arc<Mutex<()>>,
    ) -> std::io::Result<Self> {
        let (response_writer, response_sender) =
            TerminalResponseWriter::start(format!("{name}:responses"), writer, input_gate)?;
        let (finished_tx, finished) = mpsc::channel();
        let reader_response_sender = response_sender.clone();
        let handle = match std::thread::Builder::new().name(name).spawn(move || {
            copy_output(reader.as_mut(), &history, &reader_response_sender);
            reader_response_sender.close();
            let _ = history.seal_output();
            let _ = finished_tx.send(());
        }) {
            Ok(handle) => handle,
            Err(error) => {
                response_sender.close();
                let _ = response_writer.finish();
                return Err(error);
            }
        };
        Ok(Self {
            finished,
            handle: Some(handle),
            response_writer: Some(response_writer),
        })
    }

    pub(crate) fn finish(mut self) -> Result<bool, RuntimeError> {
        match self.finished.recv_timeout(OUTPUT_DRAIN_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                self.join()?;
                match self.response_writer.take() {
                    Some(writer) => writer.finish(),
                    None => Ok(true),
                }
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

fn copy_output(
    reader: &mut dyn Read,
    history: &SessionHistory,
    response_sender: &TerminalResponseSender,
) {
    let mut buffer = [0_u8; 8192];
    let mut history_error_reported = false;
    while let Ok(count) = reader.read(&mut buffer) {
        if count == 0 {
            break;
        }
        match history.push_with_terminal_responses(&buffer[..count]) {
            Ok(outcome) => {
                response_sender.enqueue(outcome.terminal_responses);
                if let Some(error) = outcome.archive_error
                    && !history_error_reported
                {
                    eprintln!(
                        "CCCC terminal history write failed; continuing to drain PTY output: {error}"
                    );
                    history_error_reported = true;
                }
            }
            Err(error) if !history_error_reported => {
                eprintln!(
                    "CCCC terminal history write failed; continuing to drain PTY output: {error}"
                );
                history_error_reported = true;
            }
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OutputReader;
    use crate::session_history::SessionHistory;
    use crate::transcript_archive::HistoryConfig;
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    struct HeldOpenReader {
        released: Arc<(Mutex<bool>, Condvar)>,
    }

    struct ChunkReader {
        chunks: VecDeque<Vec<u8>>,
    }

    struct RecordingWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
        fail: bool,
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

    impl Write for RecordingWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            if self.fail {
                return Err(std::io::Error::other("injected terminal write failure"));
            }
            self.bytes
                .lock()
                .expect("bytes lock")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn shared_writer(bytes: &Arc<Mutex<Vec<u8>>>, fail: bool) -> Arc<Mutex<Box<dyn Write + Send>>> {
        Arc::new(Mutex::new(Box::new(RecordingWriter {
            bytes: Arc::clone(bytes),
            fail,
        })))
    }

    #[test]
    fn finish_is_bounded_when_the_read_end_remains_open() {
        let released = Arc::new((Mutex::new(false), Condvar::new()));
        let written = Arc::new(Mutex::new(Vec::new()));
        let reader = OutputReader::start(
            "held-open-reader".into(),
            Box::new(HeldOpenReader {
                released: Arc::clone(&released),
            }),
            SessionHistory::new(None).expect("history"),
            shared_writer(&written, false),
            Arc::new(Mutex::new(())),
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
        let written = Arc::new(Mutex::new(Vec::new()));
        let reader = OutputReader::start(
            "failing-archive-reader".into(),
            Box::new(ChunkReader {
                chunks: VecDeque::from([b"\x1b[6nfirst".to_vec(), b" second".to_vec()]),
            }),
            history.clone(),
            shared_writer(&written, false),
            Arc::new(Mutex::new(())),
        )
        .expect("reader");

        reader.finish().expect("finish");

        assert_eq!(
            history.retained_page().expect("hot history").data,
            "\u{1b}[6nfirst second"
        );
        assert_eq!(*written.lock().expect("written"), b"\x1b[1;1R");
    }

    #[test]
    fn terminal_query_is_answered_without_a_browser_attachment() {
        let history = SessionHistory::new(None).expect("history");
        let written = Arc::new(Mutex::new(Vec::new()));
        let reader = OutputReader::start(
            "terminal-response-reader".into(),
            Box::new(ChunkReader {
                chunks: VecDeque::from([b"\x1b[5;10H\x1b[6".to_vec(), b"n".to_vec()]),
            }),
            history,
            shared_writer(&written, false),
            Arc::new(Mutex::new(())),
        )
        .expect("reader");

        assert!(reader.finish().expect("finish"));
        assert_eq!(*written.lock().expect("written"), b"\x1b[5;10R");
    }

    #[test]
    fn contended_terminal_response_does_not_block_output_drain() {
        let history = SessionHistory::new(None).expect("history");
        let written = Arc::new(Mutex::new(Vec::new()));
        let input_gate = Arc::new(Mutex::new(()));
        let input_guard = input_gate.lock().expect("input gate");
        let reader = OutputReader::start(
            "contended-terminal-response-reader".into(),
            Box::new(ChunkReader {
                chunks: VecDeque::from([b"\x1b[6n".to_vec(), b"still drained".to_vec()]),
            }),
            history.clone(),
            shared_writer(&written, false),
            Arc::clone(&input_gate),
        )
        .expect("reader");

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let output = history.retained_page().expect("hot history").data;
            if output == "\u{1b}[6nstill drained" {
                break;
            }
            assert!(Instant::now() < deadline, "PTY output drain timed out");
            std::thread::yield_now();
        }
        assert!(written.lock().expect("written").is_empty());

        drop(input_guard);
        assert!(reader.finish().expect("finish"));
        assert_eq!(*written.lock().expect("written"), b"\x1b[1;1R");
    }

    #[test]
    fn terminal_response_failure_does_not_stop_pty_drain() {
        let history = SessionHistory::new(None).expect("history");
        let written = Arc::new(Mutex::new(Vec::new()));
        let reader = OutputReader::start(
            "failing-terminal-response-reader".into(),
            Box::new(ChunkReader {
                chunks: VecDeque::from([b"\x1b[6n".to_vec(), b"still drained".to_vec()]),
            }),
            history.clone(),
            shared_writer(&written, true),
            Arc::new(Mutex::new(())),
        )
        .expect("reader");

        assert!(reader.finish().expect("finish"));
        assert_eq!(
            history.retained_page().expect("hot history").data,
            "\u{1b}[6nstill drained"
        );
    }
}
