use super::LedgerIndex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
    counters: Counters,
}

/// Process-lifetime counts for diagnostics; they never influence caching.
#[derive(Clone, Copy, Default)]
struct Counters {
    hits: u64,
    entries_created: u64,
    evictions: u64,
    invalidations: u64,
}

/// Completed full index builds, cold reads included. Counted outside the cache
/// lock because a build runs under the index lock only.
static INDEX_REBUILDS: AtomicU64 = AtomicU64::new(0);

/// A diagnostic view of the ledger index cache. Budget figures are the
/// eviction weights (the larger of source file bytes and the estimated event
/// memory), not measured process memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub entries: u64,
    pub budget_bytes: u64,
    pub budget_limit_bytes: u64,
    pub largest_budget_bytes: u64,
    pub pinned: u64,
    pub hits: u64,
    pub entries_created: u64,
    pub index_rebuilds: u64,
    pub evictions: u64,
    pub invalidations: u64,
}

impl IndexCache {
    fn new(max_entries: usize, max_source_bytes: u64) -> Self {
        Self {
            entries: HashMap::new(),
            source_bytes: 0,
            clock: 0,
            max_entries,
            max_source_bytes,
            counters: Counters::default(),
        }
    }

    fn stats(&self) -> Stats {
        Stats {
            entries: self.entries.len() as u64,
            budget_bytes: self.source_bytes,
            budget_limit_bytes: self.weight_limit(),
            largest_budget_bytes: self
                .entries
                .values()
                .map(|cached| cached.source_bytes)
                .max()
                .unwrap_or_default(),
            pinned: self
                .entries
                .values()
                .filter(|cached| Arc::strong_count(&cached.index) > 1)
                .count() as u64,
            hits: self.counters.hits,
            entries_created: self.counters.entries_created,
            index_rebuilds: INDEX_REBUILDS.load(Ordering::Relaxed),
            evictions: self.counters.evictions,
            invalidations: self.counters.invalidations,
        }
    }

    fn entry(
        &mut self,
        path: &Path,
        source_bytes: u64,
    ) -> (Arc<RwLock<LedgerIndex>>, Vec<CacheEntry>) {
        self.clock = self.clock.wrapping_add(1);
        let index = if let Some(cached) = self.entries.get_mut(path) {
            let source_bytes = source_bytes.max(cached.source_bytes);
            self.source_bytes = self
                .source_bytes
                .saturating_sub(cached.source_bytes)
                .saturating_add(source_bytes);
            cached.source_bytes = source_bytes;
            cached.last_used = self.clock;
            self.counters.hits += 1;
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
            self.counters.entries_created += 1;
            index
        };
        let retired = self.evict(path);
        (index, retired)
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
    ) -> Vec<CacheEntry> {
        self.clock = self.clock.wrapping_add(1);
        let Some(cached) = self.entries.get_mut(path) else {
            return Vec::new();
        };
        if !Arc::ptr_eq(&cached.index, expected) {
            return Vec::new();
        }
        self.source_bytes = self
            .source_bytes
            .saturating_sub(cached.source_bytes)
            .saturating_add(source_bytes);
        cached.source_bytes = source_bytes;
        cached.last_used = self.clock;
        self.evict(path)
    }

    fn remove(&mut self, path: &Path) -> Option<CacheEntry> {
        let removed = self.entries.remove(path)?;
        self.source_bytes = self.source_bytes.saturating_sub(removed.source_bytes);
        Some(removed)
    }

    fn evict(&mut self, protected: &Path) -> Vec<CacheEntry> {
        let mut retired = Vec::new();
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
            if let Some(removed) = self.remove(&candidate) {
                self.counters.evictions += 1;
                retired.push(removed);
            }
        }
        retired
    }

    /// The base budget, raised so the largest index always fits beside one other of the same
    /// size up to the base budget. Someone moving between two Groups then keeps both indexes;
    /// without it, two mid-sized ledgers would rebuild on every switch while a larger one fit.
    fn weight_limit(&self) -> u64 {
        let largest = self
            .entries
            .values()
            .map(|cached| cached.source_bytes)
            .max()
            .unwrap_or_default();
        self.max_source_bytes
            .max(largest.saturating_add(largest.min(self.max_source_bytes)))
    }
}

fn cache() -> &'static Mutex<IndexCache> {
    static CACHE: OnceLock<Mutex<IndexCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(IndexCache::new(MAX_ENTRIES, MAX_SOURCE_BYTES)))
}

pub(super) fn entry(path: &Path, source_bytes: u64) -> Arc<RwLock<LedgerIndex>> {
    let (index, retired) = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(path, source_bytes);
    // Large indexes can contain millions of allocations. Free them after the
    // cache lock is released so unrelated Groups can keep reading.
    drop(retired);
    index
}

pub(super) fn get(path: &Path) -> Option<Arc<RwLock<LedgerIndex>>> {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(path)
}

pub(super) fn update_weight(path: &Path, source_bytes: u64, expected: &Arc<RwLock<LedgerIndex>>) {
    let retired = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .update_weight(path, source_bytes, expected);
    drop(retired);
}

pub(super) fn invalidate(path: &Path) {
    let retired = {
        let mut cache = cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let retired = cache.remove(path);
        if retired.is_some() {
            cache.counters.invalidations += 1;
        }
        retired
    };
    drop(retired);
}

pub(super) fn note_rebuild() {
    INDEX_REBUILDS.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn stats() -> Stats {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .stats()
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
        let active = cache.entry(path, 20).0;
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
        let mut cache = IndexCache::new(3, 10);
        let first = Path::new("first");
        let second = Path::new("second");
        let third = Path::new("third");
        drop(cache.entry(first, 3));
        drop(cache.entry(second, 4));
        drop(cache.entry(third, 3));
        let first_index = cache.get(first).expect("first entry");

        cache.update_weight(first, 5, &first_index);

        assert!(cache.entries.contains_key(first));
        assert!(!cache.entries.contains_key(second));
        assert!(cache.entries.contains_key(third));
        assert_eq!(cache.source_bytes, 8);
    }

    #[test]
    fn budget_grows_with_the_largest_entry_without_a_step() {
        for (largest, limit) in [(5, 10), (6, 12), (10, 20), (11, 21)] {
            let mut cache = IndexCache::new(4, 10);
            cache.entry(Path::new("largest"), largest);
            assert_eq!(cache.weight_limit(), limit, "largest weight {largest}");
        }
    }

    #[test]
    fn keeps_two_alternating_mid_sized_entries() {
        let mut cache = IndexCache::new(4, 10);
        let first = Path::new("first");
        let second = Path::new("second");
        // Hold no index between accesses: an entry in use is never evicted.
        for _ in 0..3 {
            for (path, weight) in [(first, 8), (second, 6)] {
                let (_, retired) = cache.entry(path, weight);
                assert!(retired.is_empty(), "{} was evicted", path.display());
            }
        }
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.source_bytes, 14);
    }

    #[test]
    fn counts_hits_creations_evictions_and_pinned_entries() {
        let mut cache = IndexCache::new(1, 10);
        let first = Path::new("first");
        let second = Path::new("second");
        drop(cache.entry(first, 4));
        drop(cache.entry(first, 4));
        let active = cache.entry(second, 6).0;

        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.entries_created, 2);
        assert_eq!(stats.evictions, 1);
        assert_eq!(stats.pinned, 1);
        assert_eq!(stats.budget_bytes, 6);
        assert_eq!(stats.largest_budget_bytes, 6);
        assert_eq!(stats.budget_limit_bytes, 12);
        drop(active);
        assert_eq!(cache.stats().pinned, 0);
    }

    #[test]
    fn defers_eviction_while_an_entry_is_in_use() {
        let mut cache = IndexCache::new(1, 10);
        let first = Path::new("first");
        let second = Path::new("second");
        let active = cache.entry(first, 4).0;
        let second_index = cache.entry(second, 4).0;
        assert_eq!(cache.entries.len(), 2);

        drop(active);
        drop(cache.entry(second, 4));

        assert!(!cache.entries.contains_key(first));
        assert!(cache.entries.contains_key(second));
        assert_eq!(Arc::strong_count(&second_index), 2);
    }
}
