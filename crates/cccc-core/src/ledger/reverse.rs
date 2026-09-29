use super::{
    acquire_reader_lock, decode_event_line, is_gzip, ledger_group_id, read_source, source_paths,
    trim_ascii,
};
use cccc_contracts::Event;
use fs2::FileExt;
use std::fs::File;
use std::io::{self, Read, Seek};
use std::ops::ControlFlow;
use std::path::Path;

/// Visit the history newest-first across rotated segments until `visit`
/// breaks. Unlike `ledger::inspect`, nothing outlives the call, so short-lived
/// processes can answer tail questions without keeping a multiple of the whole
/// ledger resident in the shared index cache.
pub fn visit_newest_first(
    path: &Path,
    mut visit: impl FnMut(Event) -> ControlFlow<()>,
) -> io::Result<()> {
    let group_id = ledger_group_id(path);
    // Compaction renames the active file into a segment under the stable
    // ledger lock. Hold it across enumeration and reads so a rotation cannot
    // land between them and hide the newly sealed segment.
    let _source_lock = acquire_reader_lock(path)?;
    for source in source_paths(path)?.iter().rev() {
        if visit_source_newest_first(source, &group_id, &mut visit)?.is_break() {
            break;
        }
    }
    Ok(())
}

pub(super) fn visit_source_newest_first(
    path: &Path,
    group_id: &str,
    visit: &mut impl FnMut(Event) -> ControlFlow<()>,
) -> io::Result<ControlFlow<()>> {
    if is_gzip(path) {
        // Compressed segments only decode forward; rotation bounds each one.
        for event in read_source(path, group_id)?.into_iter().rev() {
            if visit(event).is_break() {
                return Ok(ControlFlow::Break(()));
            }
        }
        return Ok(ControlFlow::Continue(()));
    }

    const CHUNK_SIZE: u64 = 64 * 1024;
    let mut file = File::open(path)?;
    FileExt::lock_shared(&file)?;
    let result: io::Result<ControlFlow<()>> = (|| {
        let mut position = file.metadata()?.len();
        let mut pending = Vec::new();
        while position > 0 {
            let start = position.saturating_sub(CHUNK_SIZE);
            let chunk_len = usize::try_from(position - start).map_err(io::Error::other)?;
            let mut buffer = vec![0; chunk_len];
            file.seek(io::SeekFrom::Start(start))?;
            file.read_exact(&mut buffer)?;
            buffer.extend_from_slice(&pending);

            let mut line_end = buffer.len();
            while let Some(newline) = buffer[..line_end].iter().rposition(|byte| *byte == b'\n') {
                if visit_line(&buffer[newline + 1..line_end], path, group_id, visit).is_break() {
                    return Ok(ControlFlow::Break(()));
                }
                line_end = newline;
            }
            pending = buffer[..line_end].to_vec();
            position = start;
        }
        Ok(visit_line(&pending, path, group_id, visit))
    })();
    let unlock_result = FileExt::unlock(&file);
    let flow = result?;
    unlock_result?;
    Ok(flow)
}

fn visit_line(
    line: &[u8],
    source: &Path,
    group_id: &str,
    visit: &mut impl FnMut(Event) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let line = trim_ascii(line);
    if line.is_empty() {
        return ControlFlow::Continue(());
    }
    match decode_event_line(line, source, 0, group_id) {
        Some(event) => visit(event),
        None => ControlFlow::Continue(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn visit_waits_for_rotation_writer() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let event = Event::new("chat.message", "g_test");
        super::super::append(&path, &event).expect("append");
        let writer = super::super::acquire_writer_lock(&path).expect("writer lock");
        let (sender, receiver) = mpsc::channel();
        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            let mut visited = 0;
            let result = visit_newest_first(&reader_path, |_| {
                visited += 1;
                ControlFlow::Continue(())
            });
            sender.send(result.map(|()| visited)).expect("send");
        });

        // Compaction renames the active file under this lock; a visit that
        // enumerated sources before the rename would skip the new segment.
        assert!(
            receiver.recv_timeout(Duration::from_millis(200)).is_err(),
            "visit must not read sources while rotation holds the writer lock"
        );
        drop(writer);
        let visited = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("visit resumes after rotation")
            .expect("visit");
        assert_eq!(visited, 1);
        reader.join().expect("reader thread");
    }
}
