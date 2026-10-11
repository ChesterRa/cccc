use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const FRAME_LIMIT: usize = 16 * 1024 * 1024;

pub(super) fn spawn_reader(
    stdout: ChildStdout,
    sender: mpsc::Sender<io::Result<Value>>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("cccc-pi-rpc-out".into())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let result = read_record(&mut reader);
                let ended = result.is_err();
                if sender.blocking_send(result).is_err() || ended {
                    break;
                }
            }
        })?;
    Ok(())
}

fn read_record(reader: &mut BufReader<impl std::io::Read>) -> io::Result<Value> {
    let mut record = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Pi RPC stdout ended",
            ));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |end| end + 1);
        if record.len().saturating_add(take) > FRAME_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Pi RPC record exceeded 16 MiB",
            ));
        }
        record.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            let value: Value = serde_json::from_slice(&record).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Pi RPC record was not valid JSON",
                )
            })?;
            if !value.is_object() || !value["type"].is_string() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Pi RPC record had no type",
                ));
            }
            return Ok(value);
        }
    }
}

pub(super) async fn write(stdin: &Arc<Mutex<ChildStdin>>, message: Value) -> io::Result<()> {
    let stdin = Arc::clone(stdin);
    tokio::task::spawn_blocking(move || {
        let mut stdin = stdin
            .lock()
            .map_err(|_| io::Error::other("Pi RPC stdin lock failed"))?;
        serde_json::to_writer(&mut *stdin, &message).map_err(io::Error::other)?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    })
    .await
    .map_err(|_| io::Error::other("Pi RPC writer failed"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_use_lf_without_splitting_unicode_separators() {
        let text = "{\"type\":\"message_update\",\"text\":\"世界\u{2028}é\u{2029}\"}\r\n";
        let mut reader = BufReader::with_capacity(1, std::io::Cursor::new(text.as_bytes()));
        assert_eq!(
            read_record(&mut reader).expect("record")["text"],
            "世界\u{2028}é\u{2029}"
        );
        assert!(read_record(&mut reader).is_err());
    }
    #[test]
    fn malformed_or_unterminated_records_fail_closed() {
        for text in ["{\"type\":\"message_end\"}", "not json\n", "{}\n"] {
            assert!(read_record(&mut BufReader::new(std::io::Cursor::new(text))).is_err());
        }
    }
}
