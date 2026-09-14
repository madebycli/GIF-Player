use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: usize,
    pub budget: usize,
}

struct Entry<T> {
    value: Arc<T>,
    bytes: usize,
    last_used: u64,
}

pub struct SharedCache<T> {
    entries: HashMap<PathBuf, Entry<T>>,
    bytes: usize,
    budget: usize,
    clock: u64,
}

impl<T> SharedCache<T> {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            budget: budget.max(1),
            clock: 0,
        }
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            bytes: self.bytes,
            budget: self.budget,
        }
    }

    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget.max(1);
        self.prune();
    }

    pub fn get(&mut self, key: &Path) -> Option<Arc<T>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.last_used = self.clock;
        Some(Arc::clone(&entry.value))
    }

    pub fn insert(&mut self, key: PathBuf, value: T, bytes: usize) -> Arc<T> {
        self.clock = self.clock.wrapping_add(1);
        let value = Arc::new(value);
        if let Some(old) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.insert(
            key,
            Entry {
                value: Arc::clone(&value),
                bytes,
                last_used: self.clock,
            },
        );
        self.prune();
        value
    }

    pub fn get_or_insert_with<F>(
        &mut self,
        key: PathBuf,
        bytes: usize,
        load: F,
    ) -> Arc<T>
    where
        F: FnOnce() -> T,
    {
        if let Some(value) = self.get(&key) {
            return value;
        }
        self.insert(key, load(), bytes)
    }

    fn prune(&mut self) {
        while self.bytes > self.budget {
            let candidate = self
                .entries
                .iter()
                .filter(|(_, entry)| Arc::strong_count(&entry.value) == 1)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone());
            let Some(key) = candidate else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_key_returns_shared_allocation() {
        let mut cache = SharedCache::new(1024);
        let key = PathBuf::from("/gifs/cat.gif");
        let first = cache.get_or_insert_with(key.clone(), 100, || vec![1_u8; 4]);
        let second = cache.get_or_insert_with(key, 100, || vec![2_u8; 4]);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(second[0], 1);
        assert_eq!(cache.stats().entries, 1);
    }

    #[test]
    fn cache_does_not_evict_assets_still_referenced_by_widgets() {
        let mut cache = SharedCache::new(100);
        let held = cache.insert(PathBuf::from("a"), vec![1_u8], 90);
        cache.insert(PathBuf::from("b"), vec![2_u8], 90);
        assert_eq!(held[0], 1);
        assert!(cache.stats().bytes >= 90);
    }

    #[test]
    fn lowering_budget_evicts_unreferenced_lru_entries() {
        let mut cache = SharedCache::new(1000);
        cache.insert(PathBuf::from("a"), 1_u8, 100);
        cache.insert(PathBuf::from("b"), 2_u8, 100);
        cache.set_budget(100);
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().bytes, 100);
    }
}
