use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use futures::FutureExt;

/// Unique identifier for a watcher
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatcherId(u64);

static NEXT_WATCHER_ID: AtomicU64 = AtomicU64::new(0);

impl WatcherId {
    fn next() -> Self {
        Self(NEXT_WATCHER_ID.fetch_add(1, Ordering::SeqCst))
    }
}

/// Async watcher callback
pub type Watcher = Box<
    dyn Fn(
            Option<serde_json::Value>,
            Option<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

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
    #[allow(clippy::type_complexity)]
    pub async fn check(&self, current_values: &HashMap<String, serde_json::Value>) {
        // Collect callbacks to invoke outside the lock
        let callbacks_to_invoke = {
            let watchers = self.watchers.read().unwrap();
            let mut snapshot = self.snapshot.write().unwrap();
            let mut callbacks: Vec<(String, Pin<Box<dyn Future<Output = ()> + Send>>)> = Vec::new();

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

                    // Collect callbacks for later invocation
                    for entry in entries {
                        let future = (entry.callback)(old_value.clone(), new_value.clone());
                        callbacks.push((key.clone(), future));
                    }
                }
            }

            callbacks
        };

        // Invoke callbacks outside the lock
        for (key, future) in callbacks_to_invoke {
            if let Err(e) = AssertUnwindSafe(future).catch_unwind().await {
                tracing::error!(key = %key, "Watcher callback panicked: {:?}", e);
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
                let counter = counter_clone.clone();
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                })
            }),
        );

        // Verify watcher was added
        let watchers = service.watchers.read().unwrap();
        assert!(watchers.contains_key("MY_KEY"));
        drop(watchers);

        // Remove and verify
        service.remove(id);
        let watchers = service.watchers.read().unwrap();
        assert!(watchers.get("MY_KEY").is_none_or(|v| v.is_empty()));
    }

    #[tokio::test]
    async fn test_check_triggers_on_change() {
        let service = WatchersService::new();

        let called = Arc::new(AtomicU32::new(0));
        let called_clone = called.clone();

        service.add(
            "KEY",
            Box::new(move |old, new| {
                let called = called_clone.clone();
                Box::pin(async move {
                    assert!(old.is_none());
                    assert_eq!(new, Some(serde_json::json!("new_value")));
                    called.fetch_add(1, Ordering::SeqCst);
                })
            }),
        );

        // Check with new value
        let mut current_values = HashMap::new();
        current_values.insert("KEY".to_string(), serde_json::json!("new_value"));

        service.check(&current_values).await;

        assert_eq!(called.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_multiple_watchers_same_key() {
        let service = WatchersService::new();

        let counter1 = Arc::new(AtomicU32::new(0));
        let counter2 = Arc::new(AtomicU32::new(0));
        let c1 = counter1.clone();
        let c2 = counter2.clone();

        service.add(
            "KEY",
            Box::new(move |_, _| {
                let c = c1.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            }),
        );
        service.add(
            "KEY",
            Box::new(move |_, _| {
                let c = c2.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            }),
        );

        let mut current_values = HashMap::new();
        current_values.insert("KEY".to_string(), serde_json::json!("value"));

        service.check(&current_values).await;

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_panic_in_watcher_does_not_stop_others() {
        let service = WatchersService::new();

        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        // First watcher that panics
        service.add(
            "KEY",
            Box::new(move |_, _| {
                Box::pin(async move {
                    panic!("intentional panic");
                })
            }),
        );

        // Second watcher that should still execute
        service.add(
            "KEY",
            Box::new(move |_, _| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            }),
        );

        let mut current_values = HashMap::new();
        current_values.insert("KEY".to_string(), serde_json::json!("value"));

        service.check(&current_values).await;

        // Second watcher should have executed despite first panicking
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
