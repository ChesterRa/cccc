use super::{DeepSeekSupervisor, STDERR_TAIL_BYTES, SupervisorError};
use crate::deepseek_acp::{self, NdjsonSession, ProtocolError};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const STDOUT_FRAME_CAPACITY: usize = 512;

impl DeepSeekSupervisor {
    pub fn start(
        &mut self,
        command: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<u64, SupervisorError> {
        if command.is_empty() {
            return Err(SupervisorError::EmptyCommand);
        }
        if self.is_running() {
            return Ok(self.generation);
        }
        let mut process = Command::new(&command[0]);
        process
            .args(&command[1..])
            .current_dir(cwd)
            .envs(env.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (child, tree) = crate::OwnedProcessTree::spawn(&mut process)?;
        self.child = Some(child);
        self.process_tree = Some(tree);
        let stdout = self
            .child
            .as_mut()
            .and_then(|child| child.stdout.take())
            .ok_or_else(|| io::Error::other("deepseek stdout pipe unavailable"))?;
        let stderr = self
            .child
            .as_mut()
            .and_then(|child| child.stderr.take())
            .ok_or_else(|| io::Error::other("deepseek stderr pipe unavailable"))?;
        let (sender, receiver) = mpsc::sync_channel(STDOUT_FRAME_CAPACITY);
        let thread = std::thread::Builder::new()
            .name("cccc-deepseek-acp-stdout".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    match read_bounded_frame(&mut reader) {
                        Ok(Some(line)) => {
                            if sender.send(Some(line)).is_err() {
                                break;
                            }
                        }
                        Ok(None) => {
                            let _ = sender.send(None);
                            break;
                        }
                        Err(_) => {
                            let _ = sender.send(None);
                            break;
                        }
                    }
                }
            })?;
        self.stdout_rx = Some(receiver);
        self.stdout_thread = Some(thread);
        let stderr_tail = Arc::clone(&self.stderr_tail);
        self.stderr_thread = Some(
            std::thread::Builder::new()
                .name("cccc-deepseek-acp-stderr".into())
                .spawn(move || {
                    let mut reader = BufReader::new(stderr);
                    let mut chunk = [0_u8; 4096];
                    loop {
                        match std::io::Read::read(&mut reader, &mut chunk) {
                            Ok(0) => break,
                            Ok(size) => {
                                if let Ok(mut tail) = stderr_tail.lock() {
                                    tail.extend_from_slice(&chunk[..size]);
                                    if tail.len() > STDERR_TAIL_BYTES {
                                        let start = tail.len() - STDERR_TAIL_BYTES;
                                        tail.drain(..start);
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })?,
        );
        self.generation = self.generation.wrapping_add(1).max(1);
        self.next_request_id = 1;
        self.queue.clear();
        if let Ok(mut tail) = self.stderr_tail.lock() {
            tail.clear();
        }
        self.protocol = NdjsonSession::default();
        self.session_id = None;
        self.active_request_id = None;
        self.pending_permissions.clear();
        Ok(self.generation)
    }

    /// Complete the ACP initialize/session-new handshake for this generation.
    /// The caller must invoke this before treating the actor as runnable.
    pub fn handshake(&mut self, cwd: &Path, timeout: Duration) -> Result<String, SupervisorError> {
        self.send_request(&deepseek_acp::initialize_request("0.1.0"), 1)?;
        let initialize = self.recv_response(1, timeout)?;
        deepseek_acp::validate_initialize_result(&initialize)?;

        let cwd = cwd
            .to_str()
            .ok_or_else(|| SupervisorError::Io(io::Error::other("deepseek cwd is not UTF-8")))?;
        let request = deepseek_acp::session_new_request(cwd)
            .map_err(|_| SupervisorError::Protocol(ProtocolError::InvalidFrame))?;
        self.send_request(&request, 2)?;
        let response = self.recv_response(2, timeout)?;
        let session_id = deepseek_acp::validate_session_new_result(
            &response,
            &mut std::collections::HashSet::new(),
        )?;
        self.session_id = Some(session_id.clone());
        self.next_request_id = self.next_request_id.max(3);
        Ok(session_id)
    }

    pub fn stop(&mut self) -> Result<(), SupervisorError> {
        if self.child.is_none() {
            return Ok(());
        }
        // Resolve the active turn and all outstanding permission requests
        // before closing stdin; late frames are discarded with the generation.
        if self.session_id.is_some() {
            let _ = self.cancel();
            for request_id in self.pending_permissions.clone() {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&request_id) {
                    let _ = self.respond_permission(&value, &serde_json::json!([]), true);
                }
            }
        }
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        close_stdin(&mut child);
        let tree = self
            .process_tree
            .take()
            .expect("started child owns its process tree");
        tree.request_stop()?;
        if !wait_bounded(&mut child, &tree, Duration::from_millis(750))? {
            tree.terminate()?;
            let _ = wait_bounded(&mut child, &tree, Duration::from_millis(750));
        }
        self.stdout_rx.take();
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
        self.queue.clear();
        self.protocol = NdjsonSession::default();
        self.session_id = None;
        self.active_request_id = None;
        self.pending_permissions.clear();
        self.generation = self.generation.wrapping_add(1).max(1);
        Ok(())
    }
}

fn read_bounded_frame(reader: &mut BufReader<impl std::io::Read>) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Ok(Some(frame))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if frame.len() + take > deepseek_acp::MAX_FRAME_BYTES {
            let record_ended = newline.is_some();
            reader.consume(take);
            if !record_ended {
                discard_frame_remainder(reader)?;
            }
            // Return one byte over the cap so the protocol parser fails closed
            // exactly once for this physical NDJSON record.
            return Ok(Some(vec![0; deepseek_acp::MAX_FRAME_BYTES + 1]));
        }
        frame.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

fn discard_frame_remainder(reader: &mut BufReader<impl std::io::Read>) -> io::Result<()> {
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        reader.consume(take);
        if newline.is_some() {
            return Ok(());
        }
    }
}

fn close_stdin(child: &mut Child) {
    child.stdin.take();
}

fn wait_bounded(
    child: &mut Child,
    tree: &crate::OwnedProcessTree,
    timeout: Duration,
) -> io::Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::read_bounded_frame;
    use crate::deepseek_acp;
    use std::io::{BufReader, Cursor};

    #[test]
    fn oversized_frame_is_discarded_as_one_physical_record() {
        let valid = b"{\"jsonrpc\":\"2.0\",\"method\":\"session/update\"}\n";
        let mut bytes = vec![b'x'; deepseek_acp::MAX_FRAME_BYTES + 1];
        bytes.push(b'\n');
        bytes.extend_from_slice(valid);
        let mut reader = BufReader::new(Cursor::new(bytes));

        let oversized = read_bounded_frame(&mut reader)
            .expect("oversized read")
            .expect("oversized frame");
        assert!(oversized.len() > deepseek_acp::MAX_FRAME_BYTES);
        assert_eq!(
            read_bounded_frame(&mut reader)
                .expect("valid read")
                .expect("valid frame"),
            valid
        );
    }
}
