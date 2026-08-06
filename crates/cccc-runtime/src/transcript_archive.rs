use crate::RuntimeError;
use crate::output::HistoryPage;
use crate::transcript_files::{
    HEADER_BYTES, MAGIC, latest_end, prune_sessions, publish_latest, remove_other_sessions,
    replace_file, secure_create,
};
use crate::transcript_reader::{read_latest_page, read_latest_since};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const COMPACT_SLACK_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct HistoryConfig {
    pub path: PathBuf,
    pub max_bytes: usize,
    pub hot_bytes: usize,
    pub persist: bool,
}

pub(crate) struct TranscriptArchive {
    config: HistoryConfig,
    file: Option<File>,
    start: u64,
    end: u64,
    next_prune_at: u64,
}

impl TranscriptArchive {
    #[cfg(test)]
    pub(crate) fn create(config: HistoryConfig) -> Result<Self, RuntimeError> {
        Self::create_at(config, 0)
    }

    pub(crate) fn create_at(
        config: HistoryConfig,
        cursor_floor: u64,
    ) -> Result<Self, RuntimeError> {
        let parent = config
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("terminal transcript path has no parent"))?;
        fs::create_dir_all(parent)?;
        let cursor = latest_end(parent)?.max(cursor_floor);
        let mut file = secure_create(&config.path)?;
        write_header(&mut file, cursor)?;
        publish_latest(parent, &config.path)?;
        prune_sessions(parent, config.max_bytes as u64, &config.path)?;
        Ok(Self {
            config,
            file: Some(file),
            start: cursor,
            end: cursor,
            next_prune_at: cursor,
        })
    }

    pub(crate) fn append(&mut self, data: &[u8]) -> Result<(), RuntimeError> {
        if data.is_empty() {
            return Ok(());
        }
        let file = self.file_mut()?;
        file.seek(SeekFrom::End(0))?;
        file.write_all(data)?;
        self.end = self.end.saturating_add(data.len() as u64);
        let retained = self.end.saturating_sub(self.start);
        let max = self.config.max_bytes.max(1) as u64;
        if retained > max.saturating_add(COMPACT_SLACK_BYTES.min(max)) {
            self.compact(max)?;
        }
        if self.end >= self.next_prune_at {
            let parent = self.config.path.parent().unwrap_or(Path::new("."));
            prune_sessions(parent, max, &self.config.path)?;
            self.next_prune_at = self.end.saturating_add(COMPACT_SLACK_BYTES);
        }
        Ok(())
    }

    pub(crate) fn page(
        &mut self,
        before: Option<u64>,
        limit: usize,
    ) -> Result<HistoryPage, RuntimeError> {
        self.file_mut()?.flush()?;
        read_latest_page(self.parent(), before, limit)
    }

    pub(crate) fn page_since(
        &mut self,
        after: u64,
        limit: usize,
    ) -> Result<HistoryPage, RuntimeError> {
        self.file_mut()?.flush()?;
        read_latest_since(self.parent(), after, limit)
    }

    pub(crate) fn clear(&mut self) -> Result<(), RuntimeError> {
        self.start = self.end;
        self.file.take();
        let mut file = secure_create(&self.config.path)?;
        write_header(&mut file, self.start)?;
        self.file = Some(file);
        remove_other_sessions(self.parent(), &self.config.path)?;
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> Result<(), RuntimeError> {
        self.file_mut()?.flush()?;
        self.file_mut()?.sync_data()?;
        let parent = self.config.path.parent().unwrap_or(Path::new("."));
        prune_sessions(parent, self.config.max_bytes as u64, &self.config.path)?;
        Ok(())
    }

    fn compact(&mut self, keep: u64) -> Result<(), RuntimeError> {
        self.file_mut()?.flush()?;
        let retained = self.end.saturating_sub(self.start);
        let remove = retained.saturating_sub(keep);
        let next_start = self.start.saturating_add(remove);
        let mut source = File::open(&self.config.path)?;
        source.seek(SeekFrom::Start(HEADER_BYTES.saturating_add(remove)))?;
        let temp = self.config.path.with_extension("pty.tmp");
        let mut target = secure_create(&temp)?;
        write_header(&mut target, next_start)?;
        std::io::copy(&mut source, &mut target)?;
        target.sync_all()?;
        self.file.take();
        replace_file(&temp, &self.config.path)?;
        self.file = Some(
            OpenOptions::new()
                .read(true)
                .append(true)
                .open(&self.config.path)?,
        );
        self.start = next_start;
        let parent = self.config.path.parent().unwrap_or(Path::new("."));
        prune_sessions(parent, self.config.max_bytes as u64, &self.config.path)?;
        Ok(())
    }

    fn file_mut(&mut self) -> Result<&mut File, RuntimeError> {
        self.file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("terminal transcript file is closed").into())
    }

    pub(crate) const fn end_cursor(&self) -> u64 {
        self.end
    }

    fn parent(&self) -> &Path {
        self.config.path.parent().unwrap_or(Path::new("."))
    }
}

fn write_header(file: &mut File, start: u64) -> std::io::Result<()> {
    file.write_all(MAGIC)?;
    file.write_all(&start.to_le_bytes())
}

#[cfg(test)]
#[path = "transcript_archive_tests.rs"]
mod tests;
