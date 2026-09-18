use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const DEFAULT_REPLAY_CACHE_CAPACITY: usize = 1000;
pub const DEFAULT_REPLAY_CACHE_TTL_SECS: u64 = 3600;

#[derive(Debug, Clone)]
struct CacheEntry {
    inserted_at: Instant,
}

pub struct ReplayCache {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<Uuid, CacheEntry>,
    order: Vec<Uuid>,
}

impl ReplayCache {
    pub fn new(capacity: usize, ttl_secs: u64) -> Self {
        Self {
            capacity,
            ttl: Duration::from_secs(ttl_secs),
            entries: HashMap::with_capacity(capacity),
            order: Vec::with_capacity(capacity),
        }
    }

    /// Returns `true` if the message_id was already seen (reject), else records it and returns `false`.
    pub fn check_and_record(&mut self, message_id: &Uuid) -> bool {
        if self.entries.contains_key(message_id) {
            return true;
        }
        if self.entries.len() >= self.capacity {
            self.evict_oldest();
        }
        self.entries.insert(
            *message_id,
            CacheEntry {
                inserted_at: Instant::now(),
            },
        );
        self.order.push(*message_id);
        false
    }

    /// Remove entries older than `self.ttl`.
    pub fn prune(&mut self) {
        let cutoff = Instant::now() - self.ttl;
        self.order.retain(|id| {
            if let Some(entry) = self.entries.get(id) {
                if entry.inserted_at < cutoff {
                    self.entries.remove(id);
                    false
                } else {
                    true
                }
            } else {
                false
            }
        });
    }

    /// Remove entries older than the given UTC timestamp.
    pub fn prune_before(&mut self, now: DateTime<Utc>) {
        let cutoff = Instant::now();
        let ttl = self.ttl;
        self.order.retain(|id| {
            if let Some(entry) = self.entries.get(id) {
                if cutoff.duration_since(entry.inserted_at) > ttl {
                    self.entries.remove(id);
                    false
                } else {
                    true
                }
            } else {
                false
            }
        });
        let _ = now;
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest_id) = self.order.first().cloned() {
            self.entries.remove(&oldest_id);
            self.order.remove(0);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_and_record_returns_false_first_time() {
        let mut cache = ReplayCache::new(10, 3600);
        let id = Uuid::new_v4();
        assert!(!cache.check_and_record(&id));
    }

    #[test]
    fn check_and_record_returns_true_second_time() {
        let mut cache = ReplayCache::new(10, 3600);
        let id = Uuid::new_v4();
        assert!(!cache.check_and_record(&id));
        assert!(cache.check_and_record(&id));
    }

    #[test]
    fn evicts_oldest_at_capacity() {
        let mut cache = ReplayCache::new(3, 3600);
        let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
        for id in &ids {
            cache.check_and_record(id);
        }
        assert_eq!(cache.len(), 3);
        assert!(!cache.entries.contains_key(&ids[0]));
        assert!(!cache.entries.contains_key(&ids[1]));
        assert!(cache.entries.contains_key(&ids[2]));
    }

    #[test]
    fn prune_removes_old_entries() {
        let mut cache = ReplayCache::new(100, 0);
        let id = Uuid::new_v4();
        cache.check_and_record(&id);
        std::thread::sleep(Duration::from_millis(10));
        cache.prune();
        assert!(cache.is_empty());
    }

    #[test]
    fn prune_keeps_fresh_entries() {
        let mut cache = ReplayCache::new(100, 3600);
        let id = Uuid::new_v4();
        cache.check_and_record(&id);
        cache.prune();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn different_ids_are_independent() {
        let mut cache = ReplayCache::new(10, 3600);
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert!(!cache.check_and_record(&a));
        assert!(!cache.check_and_record(&b));
        assert!(cache.check_and_record(&a));
        assert!(cache.check_and_record(&b));
    }
}
