use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK_BYTES: usize = 64 * 1024;
const MAX_TAIL_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn read_last_lines(path: &Path, limit: usize) -> io::Result<Vec<String>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path)?;
    let mut position = file.metadata()?.len();
    let mut newline_count = 0usize;
    let mut bytes_read = 0usize;
    let mut chunks = Vec::new();
    while position > 0 && newline_count <= limit && bytes_read < MAX_TAIL_BYTES {
        let chunk_len = (position.min(CHUNK_BYTES as u64) as usize)
            .min(MAX_TAIL_BYTES.saturating_sub(bytes_read));
        position -= chunk_len as u64;
        file.seek(SeekFrom::Start(position))?;
        let mut chunk = vec![0; chunk_len];
        file.read_exact(&mut chunk)?;
        newline_count += chunk.iter().filter(|byte| **byte == b'\n').count();
        bytes_read += chunk_len;
        chunks.push(chunk);
    }
    chunks.reverse();
    let bytes = chunks.concat();
    let mut lines = bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    if position > 0 && !bytes.starts_with(b"\n") && !lines.is_empty() {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..]
        .iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_only_the_requested_tail_from_a_large_file() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        for index in 0..20_000 {
            writeln!(file, "line-{index}").expect("write line");
        }
        assert_eq!(
            read_last_lines(file.path(), 2).expect("tail"),
            ["line-19998", "line-19999"]
        );
    }

    #[test]
    fn preserves_a_final_line_without_newline() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        write!(file, "first\nlast").expect("write lines");
        assert_eq!(read_last_lines(file.path(), 1).expect("tail"), ["last"]);
    }
}
