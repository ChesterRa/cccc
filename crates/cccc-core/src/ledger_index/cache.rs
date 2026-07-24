use super::LedgerIndex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

const MAX_ENTRIES: usize = 32;
const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;

struct CacheEntry {
    index: Arc<RwLock<LedgerIndex>>,
    source_bytes: u64,
    last_used: u64,
}

struct IndexCache {
    entries: HashMap<PathBuf, CacheEntry>,
    source_bytes: u64,
    clock: u64,
    max_entries: usize,
    max_source_bytes: u64,
}

impl IndexCache {
    fn new(max_entries: usize, max_source_bytes: u64) -> Self {
        Self {
            entries: HashMap::new(),
            source_bytes: 0,
            clock: 0,
            max_entries,
            max_source_bytes,
        }
    }

    fn entry(&mut self, path: &Path, source_bytes: u64) -> Arc<RwLock<LedgerIndex>> {
        self.clock = self.clock.wrapping_add(1);
        let index = if let Some(cached) = self.entries.get_mut(path) {
            let source_bytes = source_bytes.max(cached.source_bytes);
            self.source_bytes = self
                .source_bytes
                .saturating_sub(cached.source_bytes)
                .saturating_add(source_bytes);
            cached.source_bytes = source_bytes;
            cached.last_used = self.clock;
            Arc::clone(&cached.index)
        } else {
            let index = Arc::new(RwLock::new(LedgerIndex::default()));
            self.entries.insert(
                path.to_path_buf(),
                CacheEntry {
                    index: Arc::clone(&index),
                    source_bytes,
                    last_used: self.clock,
                },
            );
            self.source_bytes = self.source_bytes.saturating_add(source_bytes);
            index
        };
        self.evict(path);
        index
    }

    fn get(&mut self, path: &Path) -> Option<Arc<RwLock<LedgerIndex>>> {
        self.clock = self.clock.wrapping_add(1);
        let cached = self.entries.get_mut(path)?;
        cached.last_used = self.clock;
        Some(Arc::clone(&cached.index))
    }

    fn update_weight(
        &mut self,
        path: &Path,
        source_bytes: u64,
        expected: &Arc<RwLock<LedgerIndex>>,
    ) {
        self.clock = self.clock.wrapping_add(1);
        let Some(cached) = self.entries.get_mut(path) else {
            return;
        };
        if !Arc::ptr_eq(&cached.index, expected) {
            return;
        }
        self.source_bytes = self
            .source_bytes
            .saturating_sub(cached.source_bytes)
            .saturating_add(source_bytes);
        cached.source_bytes = source_bytes;
        cached.last_used = self.clock;
        self.evict(path);
    }

    fn remove(&mut self, path: &Path) {
        if let Some(removed) = self.entries.remove(path) {
            self.source_bytes = self.source_bytes.saturating_sub(removed.source_bytes);
        }
    }

    fn evict(&mut self, protected: &Path) {
        while self.entries.len() > self.max_entries
            || (self.source_bytes > self.weight_limit() && self.entries.len() > 1)
        {
            let candidate = self
                .entries
                .iter()
                .filter(|(path, cached)| {
                    path.as_path() != protected && Arc::strong_count(&cached.index) == 1
                })
                .min_by_key(|(_, cached)| cached.last_used)
                .map(|(path, _)| path.clone());
            let Some(candidate) = candidate else { break };
            self.remove(&candidate);
        }
    }

    fn weight_limit(&self) -> u64 {
        let oversized_allowance = self
            .entries
            .values()
            .map(|cached| cached.source_bytes)
            .max()
            .filter(|largest| *largest > self.max_source_bytes)
            .unwrap_or_default();
        self.max_source_bytes.saturating_add(oversized_allowance)
    }
}

fn cache() -> &'static Mutex<IndexCache> {
    static CACHE: OnceLock<Mutex<IndexCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(IndexCache::new(MAX_ENTRIES, MAX_SOURCE_BYTES)))
}

pub(super) fn entry(path: &Path, source_bytes: u64) -> Arc<RwLock<LedgerIndex>> {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(path, source_bytes)
}

pub(super) fn get(path: &Path) -> Option<Arc<RwLock<LedgerIndex>>> {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(path)
}

pub(super) fn update_weight(path: &Path, source_bytes: u64, expected: &Arc<RwLock<LedgerIndex>>) {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .update_weight(path, source_bytes, expected);
}

pub(super) fn invalidate(path: &Path) {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used_entries_by_count_and_weight() {
        let mut cache = IndexCache::new(2, 10);
        let first = Path::new("first");
        let second = Path::new("second");
        let third = Path::new("third");
        cache.entry(first, 4);
        cache.entry(second, 4);
        cache.get(first);
        cache.entry(third, 4);
        assert!(cache.entries.contains_key(first));
        assert!(!cache.entries.contains_key(second));
        assert!(cache.entries.contains_key(third));
        assert!(cache.source_bytes <= 10);
    }

    #[test]
    fn retains_one_oversized_entry_alongside_budgeted_smaller_entries() {
        let mut cache = IndexCache::new(2, 10);
        let path = Path::new("oversized");
        let active = cache.entry(path, 20);
        cache.update_weight(path, 20, &active);
        cache.entry(Path::new("small"), 2);

        assert_eq!(cache.entries.len(), 2);
        assert!(cache.entries.contains_key(path));
        assert!(cache.entries.contains_key(Path::new("small")));
        assert_eq!(cache.source_bytes, 22);
        assert_eq!(Arc::strong_count(&active), 2);
    }

    #[test]
    fn replaces_the_older_oversized_entry() {
        let mut cache = IndexCache::new(4, 10);
        cache.entry(Path::new("old-large"), 20);
        cache.entry(Path::new("small"), 2);
        cache.entry(Path::new("new-large"), 30);

        assert!(!cache.entries.contains_key(Path::new("old-large")));
        assert!(cache.entries.contains_key(Path::new("small")));
        assert!(cache.entries.contains_key(Path::new("new-large")));
        assert_eq!(cache.source_bytes, 32);
    }

    #[test]
    fn updates_weight_and_evicts_the_idle_lru_entry() {
        let mut cache = IndexCache::new(2, 10);
        let first = Path::new("first");
        let second = Path::new("second");
        drop(cache.entry(first, 4));
        drop(cache.entry(second, 4));
        let first_index = cache.get(first).expect("first entry");

        cache.update_weight(first, 9, &first_index);

        assert!(cache.entries.contains_key(first));
        assert!(!cache.entries.contains_key(second));
        assert_eq!(cache.source_bytes, 9);
    }

    #[test]
    fn defers_eviction_while_an_entry_is_in_use() {
        let mut cache = IndexCache::new(1, 10);
        let first = Path::new("first");
        let second = Path::new("second");
        let active = cache.entry(first, 4);
        let second_index = cache.entry(second, 4);
        assert_eq!(cache.entries.len(), 2);

        drop(active);
        drop(cache.entry(second, 4));

        assert!(!cache.entries.contains_key(first));
        assert!(cache.entries.contains_key(second));
        assert_eq!(Arc::strong_count(&second_index), 2);
    }
}
