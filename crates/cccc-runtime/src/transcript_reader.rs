use crate::RuntimeError;
use crate::output::{HistoryPage, complete_utf8_prefix_len, cursor_preserving_text};
use crate::transcript_files::{HEADER_BYTES, latest_path, transcript_bounds};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

struct TranscriptSegment {
    path: PathBuf,
    start: u64,
    end: u64,
    is_latest: bool,
    modified: Option<std::time::SystemTime>,
}

pub fn read_latest_page(
    actor_dir: &Path,
    before: Option<u64>,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    let segments = segments(actor_dir)?;
    let (retained_start, retained_end) = retained_bounds(&segments)?;
    let requested = before.unwrap_or(retained_end);
    if requested < retained_start {
        return Ok(HistoryPage {
            data: String::new(),
            start_cursor: retained_start,
            end_cursor: retained_start,
            has_more: false,
            cursor_expired: true,
        });
    }
    let wanted_end = requested.min(retained_end);
    let wanted_start = wanted_end
        .saturating_sub(limit.max(1) as u64)
        .max(retained_start);
    let (page_start, page_end, bytes) = read_span(&segments, wanted_start, wanted_end)?;
    Ok(HistoryPage {
        data: cursor_preserving_text(&bytes),
        start_cursor: page_start,
        end_cursor: page_end,
        has_more: page_start > retained_start,
        cursor_expired: page_start > wanted_start,
    })
}

pub fn read_latest_since(
    actor_dir: &Path,
    after: u64,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    let segments = segments(actor_dir)?;
    let (retained_start, retained_end) = retained_bounds(&segments)?;
    let wanted_start = after.clamp(retained_start, retained_end);
    let candidate_end = wanted_start
        .saturating_add(limit.max(1) as u64)
        .min(retained_end);
    let lookahead_end = candidate_end.saturating_add(3).min(retained_end);
    let (page_start, available_end, bytes) = read_span(&segments, wanted_start, lookahead_end)?;
    let candidate_len = candidate_end.min(available_end).saturating_sub(page_start) as usize;
    let mut requested_len = candidate_len.min(bytes.len());
    while requested_len < bytes.len() && bytes[requested_len] & 0b1100_0000 == 0b1000_0000 {
        requested_len += 1;
    }
    let complete_len = complete_utf8_prefix_len(&bytes[..requested_len]);
    let page_end = page_start.saturating_add(complete_len as u64);
    Ok(HistoryPage {
        data: cursor_preserving_text(&bytes[..complete_len]),
        start_cursor: page_start,
        end_cursor: page_end,
        has_more: page_end < retained_end,
        cursor_expired: after < retained_start || page_start > wanted_start,
    })
}

fn segments(actor_dir: &Path) -> Result<Vec<TranscriptSegment>, RuntimeError> {
    let latest = latest_path(actor_dir)?;
    transcript_bounds(&latest)?;
    let mut segments = Vec::new();
    for entry in fs::read_dir(actor_dir)? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "pty") {
            continue;
        }
        match transcript_bounds(&path) {
            Ok((start, end)) => segments.push(TranscriptSegment {
                modified: path.metadata().and_then(|value| value.modified()).ok(),
                is_latest: path == latest,
                path,
                start,
                end,
            }),
            Err(error) if path == latest => return Err(error.into()),
            Err(_) => {}
        }
    }
    segments.sort_by(|left, right| {
        (left.start, left.is_latest, left.modified, &left.path).cmp(&(
            right.start,
            right.is_latest,
            right.modified,
            &right.path,
        ))
    });
    let mut distinct = Vec::<TranscriptSegment>::with_capacity(segments.len());
    for segment in segments {
        if let Some(previous) = distinct.last_mut()
            && previous.start == segment.start
        {
            *previous = segment;
            continue;
        }
        distinct.push(segment);
    }
    let mut segments = distinct;
    for index in 0..segments.len().saturating_sub(1) {
        segments[index].end = segments[index].end.min(segments[index + 1].start);
    }
    if segments.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no terminal transcripts found",
        )
        .into());
    }
    Ok(segments)
}

fn retained_bounds(segments: &[TranscriptSegment]) -> Result<(u64, u64), RuntimeError> {
    let start = segments
        .iter()
        .map(|segment| segment.start)
        .min()
        .ok_or_else(|| std::io::Error::other("no terminal transcript bounds"))?;
    let end = segments
        .iter()
        .map(|segment| segment.end)
        .max()
        .ok_or_else(|| std::io::Error::other("no terminal transcript bounds"))?;
    Ok((start, end))
}

fn read_span(
    segments: &[TranscriptSegment],
    wanted_start: u64,
    wanted_end: u64,
) -> Result<(u64, u64, Vec<u8>), RuntimeError> {
    let mut page_start = wanted_start;
    let mut cursor = wanted_start;
    let mut bytes = Vec::with_capacity(wanted_end.saturating_sub(wanted_start) as usize);
    for segment in segments {
        if segment.end <= cursor || segment.start >= wanted_end {
            continue;
        }
        if segment.start > cursor {
            bytes.clear();
            cursor = segment.start;
            page_start = cursor;
        }
        let end = segment.end.min(wanted_end);
        if cursor < end {
            bytes.extend(read_range(segment, cursor, end)?);
            cursor = end;
        }
        if cursor >= wanted_end {
            break;
        }
    }
    Ok((page_start, cursor, bytes))
}

fn read_range(segment: &TranscriptSegment, start: u64, end: u64) -> Result<Vec<u8>, RuntimeError> {
    let mut file = File::open(&segment.path)?;
    let mut bytes = vec![0_u8; end.saturating_sub(start) as usize];
    file.seek(SeekFrom::Start(
        HEADER_BYTES.saturating_add(start.saturating_sub(segment.start)),
    ))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}
