use serde_json::Value;
use std::io;

/// Bound each JSONL record, not a read batch that may contain several valid records.
pub(super) fn take_records(buffer: &mut Vec<u8>, limit: usize) -> io::Result<Vec<Value>> {
    let mut records = Vec::new();
    let mut start = 0;
    for (end, byte) in buffer.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let line = &buffer[start..end];
        check_length(line.len(), limit)?;
        if !line.iter().all(u8::is_ascii_whitespace) {
            records.push(serde_json::from_slice(line).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Claude transcript record is invalid: {error}"),
                )
            })?);
        }
        start = end + 1;
    }
    check_length(buffer.len() - start, limit)?;
    buffer.drain(..start);
    Ok(records)
}

fn check_length(length: usize, limit: usize) -> io::Result<()> {
    if length > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude transcript record exceeded its limit",
        ));
    }
    Ok(())
}
