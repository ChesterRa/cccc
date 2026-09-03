use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub(super) fn spawn_reader(
    stdout: ChildStdout,
    sender: mpsc::Sender<io::Result<Vec<u8>>>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("cccc-managed-acp-out".into())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let frame = read_bounded_frame(&mut reader);
                let ended = matches!(&frame, Ok(None));
                let send = match frame {
                    Ok(Some(frame)) => sender.blocking_send(Ok(frame)),
                    Ok(None) => sender.blocking_send(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "ACP stdout closed",
                    ))),
                    Err(error) => sender.blocking_send(Err(error)),
                };
                if send.is_err() || ended {
                    break;
                }
            }
        })?;
    Ok(())
}

fn read_bounded_frame(reader: &mut BufReader<impl std::io::Read>) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ACP stream ended inside a frame",
                ))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if frame.len().saturating_add(take) > MAX_FRAME_BYTES {
            reader.consume(take);
            if newline.is_none() {
                discard_frame_remainder(reader)?;
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ACP frame exceeds the 1 MiB limit",
            ));
        }
        frame.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            while frame
                .last()
                .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
            {
                frame.pop();
            }
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

pub(super) async fn write_message(
    stdin: &Arc<Mutex<ChildStdin>>,
    message: Value,
) -> io::Result<()> {
    let stdin = Arc::clone(stdin);
    tokio::task::spawn_blocking(move || {
        let mut stdin = stdin
            .lock()
            .map_err(|_| io::Error::other("ACP stdin lock poisoned"))?;
        serde_json::to_writer(&mut *stdin, &message).map_err(io::Error::other)?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    })
    .await
    .map_err(|error| io::Error::other(format!("ACP writer task failed: {error}")))?
}

pub(super) fn parse_frame(frame: &[u8]) -> io::Result<Value> {
    let value: Value = serde_json::from_slice(frame).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid ACP JSON: {error}"),
        )
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "ACP frame is not an object"))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ACP frame has an unsupported JSON-RPC version",
        ));
    }
    if !object.contains_key("method") && !object.contains_key("id") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ACP frame is neither a request, notification, nor response",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn bounded_reader_rejects_an_oversized_record_without_consuming_the_next() {
        let valid = b"{\"jsonrpc\":\"2.0\",\"method\":\"session/update\"}\n";
        let mut input = vec![b'x'; MAX_FRAME_BYTES + 1];
        input.push(b'\n');
        input.extend_from_slice(valid);
        let mut reader = BufReader::new(Cursor::new(input));
        assert!(read_bounded_frame(&mut reader).is_err());
        assert_eq!(
            read_bounded_frame(&mut reader).expect("next frame"),
            Some(valid[..valid.len() - 1].to_vec())
        );
    }
}
