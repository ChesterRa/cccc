use cccc_contracts::Event;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

pub fn append(path: &Path, event: &Event) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;
    file.lock_exclusive()?;
    let result = append_locked(&mut file, event);
    let unlock_result = FileExt::unlock(&file);
    result.and(unlock_result)
}

fn append_locked(file: &mut File, event: &Event) -> io::Result<()> {
    serde_json::to_writer(&mut *file, event).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_data()
}

pub fn read_all(path: &Path) -> io::Result<Vec<Event>> {
    let mut events = Vec::new();
    let segments = path
        .parent()
        .map(|group| group.join("state/ledger/segments"));
    if let Some(segments) = segments.filter(|dir| dir.is_dir()) {
        let mut paths: Vec<_> = std::fs::read_dir(segments)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| candidate.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        paths.sort();
        for source in paths {
            events.extend(read_source(&source)?);
        }
    }
    if path.exists() {
        events.extend(read_source(path)?);
    }
    Ok(events)
}

fn read_source(path: &Path) -> io::Result<Vec<Event>> {
    BufReader::new(File::open(path)?)
        .lines()
        .filter(|line| line.as_ref().map_or(true, |value| !value.trim().is_empty()))
        .map(|line| {
            let line = line?;
            serde_json::from_str(&line).map_err(io::Error::other)
        })
        .collect()
}

pub fn tail(path: &Path, limit: usize) -> io::Result<Vec<Event>> {
    let events = read_all(path)?;
    let start = events.len().saturating_sub(limit);
    Ok(events[start..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_reads_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        append(&path, &Event::new("group.create", "g_test")).expect("append");
        append(&path, &Event::new("chat.message", "g_test")).expect("append");
        let events = tail(&path, 1).expect("tail");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "chat.message");
    }
}
