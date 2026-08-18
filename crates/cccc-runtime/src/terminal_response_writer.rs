use crate::RuntimeError;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, TryLockError};
use std::thread::JoinHandle;
use std::time::Duration;

const MAX_QUEUED_RESPONSES: usize = 64;
const RESPONSE_RETRY_DELAY: Duration = Duration::from_millis(5);
const RESPONSE_FINISH_TIMEOUT: Duration = Duration::from_millis(250);

pub(crate) type SharedPtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;

struct QueueState {
    responses: VecDeque<Vec<u8>>,
    closed: bool,
    overflow_reported: bool,
}

struct SharedQueue {
    state: Mutex<QueueState>,
    wake: Condvar,
}

#[derive(Clone)]
pub(crate) struct TerminalResponseSender {
    queue: Arc<SharedQueue>,
}

pub(crate) struct TerminalResponseWriter {
    finished: Receiver<()>,
    handle: Option<JoinHandle<()>>,
}

impl TerminalResponseSender {
    pub(crate) fn enqueue(&self, responses: Vec<Vec<u8>>) {
        if responses.is_empty() {
            return;
        }
        let Ok(mut state) = self.queue.state.lock() else {
            return;
        };
        if state.closed {
            return;
        }
        for response in responses {
            if state.responses.len() >= MAX_QUEUED_RESPONSES {
                if !state.overflow_reported {
                    eprintln!(
                        "CCCC terminal response queue full; dropping excess terminal responses"
                    );
                    state.overflow_reported = true;
                }
                continue;
            }
            state.responses.push_back(response);
        }
        self.queue.wake.notify_one();
    }

    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.queue.state.lock() {
            state.closed = true;
            self.queue.wake.notify_all();
        }
    }
}

impl TerminalResponseWriter {
    pub(crate) fn start(
        name: String,
        writer: SharedPtyWriter,
        input_gate: Arc<Mutex<()>>,
    ) -> std::io::Result<(Self, TerminalResponseSender)> {
        let queue = Arc::new(SharedQueue {
            state: Mutex::new(QueueState {
                responses: VecDeque::new(),
                closed: false,
                overflow_reported: false,
            }),
            wake: Condvar::new(),
        });
        let sender = TerminalResponseSender {
            queue: Arc::clone(&queue),
        };
        let (finished_tx, finished) = mpsc::channel();
        let handle = std::thread::Builder::new().name(name).spawn(move || {
            run_response_writer(&queue, &writer, &input_gate);
            let _ = finished_tx.send(());
        })?;
        Ok((
            Self {
                finished,
                handle: Some(handle),
            },
            sender,
        ))
    }

    pub(crate) fn finish(mut self) -> Result<bool, RuntimeError> {
        match self.finished.recv_timeout(RESPONSE_FINISH_TIMEOUT) {
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
            .map_err(|_| std::io::Error::other("terminal response writer panicked").into())
    }
}

fn run_response_writer(queue: &SharedQueue, writer: &SharedPtyWriter, input_gate: &Arc<Mutex<()>>) {
    let mut error_reported = false;
    loop {
        if !wait_for_responses(queue) {
            return;
        }
        let _input_guard = match input_gate.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => {
                std::thread::park_timeout(RESPONSE_RETRY_DELAY);
                continue;
            }
            Err(TryLockError::Poisoned(_)) => {
                report_response_error(
                    &mut error_reported,
                    "terminal input gate poisoned".to_owned(),
                );
                return;
            }
        };
        let mut writer = match writer.try_lock() {
            Ok(writer) => writer,
            Err(TryLockError::WouldBlock) => {
                std::thread::park_timeout(RESPONSE_RETRY_DELAY);
                continue;
            }
            Err(TryLockError::Poisoned(_)) => {
                report_response_error(&mut error_reported, "terminal writer poisoned".to_owned());
                return;
            }
        };
        let responses = take_responses(queue);
        if responses.is_empty() {
            continue;
        }
        let result = responses
            .iter()
            .try_for_each(|response| writer.write_all(response))
            .and_then(|()| writer.flush());
        if let Err(error) = result {
            report_response_error(&mut error_reported, error.to_string());
        }
    }
}

fn wait_for_responses(queue: &SharedQueue) -> bool {
    let Ok(mut state) = queue.state.lock() else {
        return false;
    };
    while state.responses.is_empty() && !state.closed {
        let Ok(next) = queue.wake.wait(state) else {
            return false;
        };
        state = next;
    }
    !state.responses.is_empty()
}

fn take_responses(queue: &SharedQueue) -> Vec<Vec<u8>> {
    queue
        .state
        .lock()
        .map(|mut state| state.responses.drain(..).collect())
        .unwrap_or_default()
}

fn report_response_error(reported: &mut bool, error: String) {
    if *reported {
        return;
    }
    eprintln!("CCCC terminal response write failed; continuing to drain PTY output: {error}");
    *reported = true;
}

#[cfg(test)]
mod tests {
    use super::{MAX_QUEUED_RESPONSES, SharedPtyWriter, TerminalResponseWriter};
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    struct RecordingWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for RecordingWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("recording lock")
                .extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn response_queue_is_bounded_while_input_is_contended() {
        let input_gate = Arc::new(Mutex::new(()));
        let input_guard = input_gate.lock().expect("input gate");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let writer: SharedPtyWriter =
            Arc::new(Mutex::new(Box::new(RecordingWriter(Arc::clone(&recorded)))));
        let (worker, sender) = TerminalResponseWriter::start(
            "bounded-terminal-responses".into(),
            writer,
            Arc::clone(&input_gate),
        )
        .expect("worker");

        sender.enqueue((0_u8..100).map(|value| vec![value]).collect());
        sender.close();
        drop(input_guard);

        assert!(worker.finish().expect("finish"));
        assert_eq!(
            recorded.lock().expect("recorded").len(),
            MAX_QUEUED_RESPONSES
        );
    }
}
