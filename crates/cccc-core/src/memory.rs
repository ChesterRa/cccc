use cccc_contracts::utc_now;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{GroupStore, HomeLayout};

#[derive(Debug, Clone, Serialize)]
pub struct MemoryLayout {
    pub root: PathBuf,
    pub memory_file: PathBuf,
    pub daily_dir: PathBuf,
    pub today_file: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryHit {
    pub path: String,
    pub start_line: usize,
    pub score: f64,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct MemoryStore {
    home: HomeLayout,
}

impl MemoryStore {
    #[must_use]
    pub fn new(home: HomeLayout) -> Self {
        Self { home }
    }

    pub fn layout(&self, group_id: &str, date: Option<&str>) -> io::Result<MemoryLayout> {
        let group = GroupStore::new(self.home.clone())?.load(group_id)?;
        let root = GroupStore::new(self.home.clone())?
            .state_dir(group_id)?
            .join("memory");
        let daily_dir = root.join("daily");
        fs::create_dir_all(&daily_dir)?;
        let label = sanitize(&group.title);
        let date = date
            .filter(|value| valid_date(value))
            .map_or_else(today, str::to_owned);
        let memory_file = root.join("MEMORY.md");
        let today_file = daily_dir.join(format!("{date}__{label}.md"));
        ensure_file(&memory_file, &format!("# MEMORY ({})\n\n", group.title))?;
        ensure_file(
            &today_file,
            &format!("# Daily Memory ({}) - {date}\n\n", group.title),
        )?;
        Ok(MemoryLayout {
            root,
            memory_file,
            daily_dir,
            today_file,
        })
    }

    pub fn write(
        &self,
        group_id: &str,
        target: &str,
        content: &str,
        date: Option<&str>,
    ) -> io::Result<(PathBuf, String, bool)> {
        let layout = self.layout(group_id, date)?;
        let path = if target == "daily" {
            layout.today_file
        } else {
            layout.memory_file
        };
        let normalized = content.trim();
        if normalized.is_empty() {
            return Err(io::Error::other("memory content is required"));
        }
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let deduped = existing.contains(normalized);
        if !deduped {
            let separator = if existing.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            fs::write(&path, format!("{existing}{separator}{normalized}\n"))?;
        }
        Ok((
            path,
            format!("{:x}", Sha256::digest(normalized.as_bytes())),
            deduped,
        ))
    }

    pub fn get(
        &self,
        group_id: &str,
        target: &str,
        date: Option<&str>,
    ) -> io::Result<(PathBuf, String)> {
        let layout = self.layout(group_id, date)?;
        let path = if target == "daily" {
            layout.today_file
        } else {
            layout.memory_file
        };
        let content = fs::read_to_string(&path)?;
        Ok((path, content))
    }

    pub fn search(&self, group_id: &str, query: &str, limit: usize) -> io::Result<Vec<MemoryHit>> {
        let layout = self.layout(group_id, None)?;
        let terms: Vec<_> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let mut files = vec![layout.memory_file];
        files.extend(
            fs::read_dir(layout.daily_dir)?
                .filter_map(Result::ok)
                .map(|entry| entry.path()),
        );
        let mut hits = Vec::new();
        for path in files {
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            for (index, line) in fs::read_to_string(&path)
                .unwrap_or_default()
                .lines()
                .enumerate()
            {
                let lower = line.to_lowercase();
                let matched = terms
                    .iter()
                    .filter(|term| lower.contains(term.as_str()))
                    .count();
                if matched > 0 {
                    hits.push(MemoryHit {
                        path: relative(&layout.root, &path),
                        start_line: index + 1,
                        score: matched as f64 / terms.len() as f64,
                        snippet: line.into(),
                    });
                }
            }
        }
        hits.sort_by(|left, right| right.score.total_cmp(&left.score));
        hits.truncate(limit.min(100));
        Ok(hits)
    }
}

fn ensure_file(path: &Path, header: &str) -> io::Result<()> {
    if !path.exists() {
        fs::write(path, header)?;
    }
    Ok(())
}
fn today() -> String {
    utc_now()[..10].into()
}
fn valid_date(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}
