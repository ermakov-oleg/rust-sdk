use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// Unique identifier for a watcher
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatcherId(u64);

static NEXT_WATCHER_ID: AtomicU64 = AtomicU64::new(0);

impl WatcherId {
    fn next() -> Self {
        Self(NEXT_WATCHER_ID.fetch_add(1, Ordering::SeqCst))
    }
}

/// Sync watcher callback
pub type Watcher = Box<dyn Fn(Option<serde_json::Value>, Option<serde_json::Value>) + Send + Sync>;

struct WatcherEntry {
    id: WatcherId,
    callback: Watcher,
}

pub struct WatchersService {
    watchers: RwLock<HashMap<String, Vec<WatcherEntry>>>,
    snapshot: RwLock<HashMap<String, serde_json::Value>>,
}

impl WatchersService {
    pub fn new() -> Self {
        Self {
            watchers: RwLock::new(HashMap::new()),
            snapshot: RwLock::new(HashMap::new()),
        }
    }

    /// Add a watcher for a key
    pub fn add(&self, key: &str, callback: Watcher) -> WatcherId {
        let id = WatcherId::next();
        let entry = WatcherEntry { id, callback };

        let mut watchers = self.watchers.write().unwrap();
        watchers.entry(key.to_string()).or_default().push(entry);

        id
    }

    /// Remove a watcher by ID
    pub fn remove(&self, id: WatcherId) {
        let mut watchers = self.watchers.write().unwrap();
        for entries in watchers.values_mut() {
            entries.retain(|e| e.id != id);
        }
    }

    /// Check for changes and notify watchers
    pub async fn check(&self, current_values: &HashMap<String, serde_json::Value>) {
        let watchers = self.watchers.read().unwrap();
        let mut snapshot = self.snapshot.write().unwrap();

        for (key, entries) in watchers.iter() {
            let old_value = snapshot.get(key).cloned();
            let new_value = current_values.get(key).cloned();

            if old_value != new_value {
                // Update snapshot
                if let Some(ref v) = new_value {
                    snapshot.insert(key.clone(), v.clone());
                } else {
                    snapshot.remove(key);
                }

                // Notify watchers
                for entry in entries {
                    // Catch panics in watchers
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (entry.callback)(old_value.clone(), new_value.clone());
                    }));
                    if let Err(e) = result {
                        tracing::error!("Watcher for key '{}' panicked: {:?}", key, e);
                    }
                }
            }
        }
    }

    /// Update snapshot without notifying (for initialization)
    pub fn update_snapshot(&self, key: &str, value: serde_json::Value) {
        let mut snapshot = self.snapshot.write().unwrap();
        snapshot.insert(key.to_string(), value);
    }
}

impl Default for WatchersService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_add_and_remove_watcher() {
        let service = WatchersService::new();

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let id = service.add(
            "MY_KEY",
            Box::new(move |_, _| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        );

        // Verify watcher was added
        let watchers = service.watchers.read().unwrap();
        assert!(watchers.contains_key("MY_KEY"));
        drop(watchers);

        // Remove and verify
        service.remove(id);
        let watchers = service.watchers.read().unwrap();
        assert!(watchers.get("MY_KEY").map_or(true, |v| v.is_empty()));
    }

    #[tokio::test]
    async fn test_check_triggers_on_change() {
        let service = WatchersService::new();

        let called = Arc::new(AtomicU32::new(0));
        let called_clone = called.clone();

        service.add(
            "KEY",
            Box::new(move |old, new| {
                assert!(old.is_none());
                assert_eq!(new, Some(serde_json::json!("new_value")));
                called_clone.fetch_add(1, Ordering::SeqCst);
            }),
        );

        // Check with new value
        let mut current_values = HashMap::new();
        current_values.insert("KEY".to_string(), serde_json::json!("new_value"));

        service.check(&current_values).await;

        assert_eq!(called.load(Ordering::SeqCst), 1);
    }
}
