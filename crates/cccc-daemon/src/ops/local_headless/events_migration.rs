use serde_json::Value;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader};

const MAX_EVENT_LINE_BYTES: usize = 1024 * 1024;
const DEDUPE_KEY_TOKEN: &[u8] = b"\"dedupe_key\"";

enum LegacyLine {
    Complete(Vec<u8>),
    OversizedWithoutDedupeIdentity,
}

pub(super) fn scan_dedupe_keys(
    path: &std::path::Path,
    mut on_key: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let mut reader = BufReader::new(OpenOptions::new().read(true).open(path)?);
    while let Some(line) = read_line(&mut reader)? {
        let LegacyLine::Complete(line) = line else {
            continue;
        };
        let line = line.strip_suffix(b"\n").unwrap_or(&line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Ok(payload) = serde_json::from_slice::<Value>(line)
            && let Some(key) = payload.get("dedupe_key").and_then(Value::as_str)
        {
            on_key(key)?;
        }
    }
    Ok(())
}

fn read_line(reader: &mut impl BufRead) -> io::Result<Option<LegacyLine>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(LegacyLine::Complete(line)))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if line.len() + take > MAX_EVENT_LINE_BYTES {
            let mut mentions_dedupe_key = contains_token(&line);
            let mut tail = line[line
                .len()
                .saturating_sub(DEDUPE_KEY_TOKEN.len().saturating_sub(1))..]
                .to_vec();
            scan_chunk_for_token(&mut mentions_dedupe_key, &mut tail, &available[..take]);
            reader.consume(take);
            if newline.is_none() {
                drain_oversized_line(reader, &mut mentions_dedupe_key, &mut tail)?;
            }
            if mentions_dedupe_key {
                return Err(io::Error::other(
                    "oversized deepseek dedupe migration event has dedupe identity",
                ));
            }
            return Ok(Some(LegacyLine::OversizedWithoutDedupeIdentity));
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Some(LegacyLine::Complete(line)));
        }
    }
}

fn drain_oversized_line(
    reader: &mut impl BufRead,
    mentions_dedupe_key: &mut bool,
    tail: &mut Vec<u8>,
) -> io::Result<()> {
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        scan_chunk_for_token(mentions_dedupe_key, tail, &available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(());
        }
    }
}

fn scan_chunk_for_token(seen: &mut bool, tail: &mut Vec<u8>, chunk: &[u8]) {
    if *seen {
        return;
    }
    let boundary_len = chunk.len().min(DEDUPE_KEY_TOKEN.len().saturating_sub(1));
    let mut boundary = Vec::with_capacity(tail.len() + boundary_len);
    boundary.extend_from_slice(tail);
    boundary.extend_from_slice(&chunk[..boundary_len]);
    *seen = contains_token(&boundary) || contains_token(chunk);
    if *seen {
        return;
    }
    let keep = DEDUPE_KEY_TOKEN.len().saturating_sub(1);
    if chunk.len() >= keep {
        tail.clear();
        tail.extend_from_slice(&chunk[chunk.len() - keep..]);
    } else {
        tail.extend_from_slice(chunk);
        if tail.len() > keep {
            tail.drain(..tail.len() - keep);
        }
    }
}

fn contains_token(bytes: &[u8]) -> bool {
    bytes
        .windows(DEDUPE_KEY_TOKEN.len())
        .any(|window| window == DEDUPE_KEY_TOKEN)
}
