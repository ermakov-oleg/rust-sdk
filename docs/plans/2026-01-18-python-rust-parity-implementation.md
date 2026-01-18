# Python-Rust Parity Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Achieve feature parity between Python and Rust runtime-settings implementations.

**Architecture:** Extend existing RuntimeSettings with secrets auto-renewal, async watchers, and configurable refresh.

**Tech Stack:** Rust, tokio, reqwest, vaultrs, uuid, serde

---

## Deferred / Skipped

- IGNORECASE in regex filters — already implemented
- PEP 440 vs semver — keeping semver for Rust
- testutils/FakeSettings — skipped
- Parallel watcher execution — skipped
- Sync secrets in get() — deferred to `docs/plans/2026-01-18-sync-secrets-loading.md`

---

## Task 1: Add X-OperationId Header to MCS Requests

**Files:**
- Modify: `lib/runtime-settings/src/providers/mcs.rs`
- Modify: `lib/runtime-settings/Cargo.toml`

**Step 1: Add uuid dependency**

```bash
cd lib/runtime-settings && cargo add uuid --features v4
```

**Step 2: Add header in load()**

```rust
use uuid::Uuid;

// In load(), change:
let response = self.client.get(&url).query(&request).send().await?;

// To:
let operation_id = Uuid::new_v4().to_string();
let response = self
    .client
    .get(&url)
    .query(&request)
    .header("X-OperationId", &operation_id)
    .send()
    .await?;
```

**Step 3: Test and commit**

```bash
cargo test -p runtime-settings
git add lib/runtime-settings && git commit -m "feat(runtime-settings): add X-OperationId header to MCS requests"
```

---

## Task 2: Make Refresh Interval Configurable

**Files:**
- Modify: `lib/runtime-settings/src/settings.rs`
- Modify: `lib/runtime-settings/src/setup.rs`

**Step 1: Add refresh_interval to builder and struct**

In `settings.rs`, add to `RuntimeSettingsBuilder`:
```rust
refresh_interval: Duration,
```

In `new()`:
```rust
refresh_interval: Duration::from_secs(30),
```

Add builder method:
```rust
pub fn refresh_interval(mut self, interval: Duration) -> Self {
    self.refresh_interval = interval;
    self
}
```

Add to `RuntimeSettings`:
```rust
pub(crate) refresh_interval: Duration,
```

In `build()`:
```rust
refresh_interval: self.refresh_interval,
```

**Step 2: Update setup.rs**

```rust
pub async fn setup(builder: RuntimeSettingsBuilder) -> Result<(), SettingsError> {
    let runtime_settings = builder.build();
    let refresh_interval = runtime_settings.refresh_interval;
    runtime_settings.init().await?;

    SETTINGS
        .set(runtime_settings)
        .map_err(|_| SettingsError::Vault("Settings already initialized".to_string()))?;

    tokio::spawn(async move {
        loop {
            sleep(refresh_interval).await;
            if let Err(e) = settings().refresh().await {
                tracing::error!("Settings refresh failed: {}", e);
            }
        }
    });

    Ok(())
}
```

**Step 3: Test and commit**

```bash
cargo test -p runtime-settings
git add lib/runtime-settings && git commit -m "feat(runtime-settings): make refresh interval configurable"
```

---

## Task 3: Implement Secrets Auto-Renewal

**Files:**
- Modify: `lib/runtime-settings/src/secrets/mod.rs`

**Step 1: Add needs_refresh to CachedSecret**

```rust
impl CachedSecret {
    fn needs_refresh(&self, threshold: f64) -> bool {
        match self.lease_duration {
            Some(duration) if self.renewable => {
                let elapsed = self.fetched_at.elapsed();
                let threshold_duration = Duration::from_secs_f64(duration.as_secs_f64() * threshold);
                elapsed >= threshold_duration
            }
            _ => false,
        }
    }
}
```

**Step 2: Add needs_static_refresh**

```rust
impl SecretsService {
    fn needs_static_refresh(&self, path: &str, cached: &CachedSecret) -> bool {
        if cached.renewable {
            return false;
        }
        for (pattern, interval) in &self.refresh_intervals {
            if path.contains(pattern) {
                return cached.fetched_at.elapsed() >= *interval;
            }
        }
        false
    }
}
```

**Step 3: Implement refresh()**

```rust
pub async fn refresh(&self) -> Result<(), SettingsError> {
    let client = match &self.client {
        Some(c) => c,
        None => return Ok(()),
    };

    let paths_to_refresh: Vec<String> = {
        let cache = self.cache.read().await;
        cache
            .iter()
            .filter(|(path, cached)| {
                cached.needs_refresh(0.75) || self.needs_static_refresh(path, cached)
            })
            .map(|(path, _)| path.clone())
            .collect()
    };

    for path in paths_to_refresh {
        match vaultrs::kv2::read::<serde_json::Value>(client, "secret", &path).await {
            Ok(secret) => {
                let mut cache = self.cache.write().await;
                cache.insert(
                    path.clone(),
                    CachedSecret {
                        value: secret,
                        lease_id: None,
                        lease_duration: None,
                        renewable: false,
                        fetched_at: Instant::now(),
                    },
                );
                tracing::debug!(path = %path, "Refreshed secret");
            }
            Err(e) => {
                tracing::warn!(path = %path, error = %e, "Failed to refresh secret");
            }
        }
    }

    Ok(())
}
```

**Step 4: Test and commit**

```bash
cargo test -p runtime-settings
git add lib/runtime-settings && git commit -m "feat(runtime-settings): implement secrets auto-renewal"
```

---

## Task 4: Add STATIC_SECRETS_REFRESH_INTERVALS Support

**Files:**
- Modify: `lib/runtime-settings/src/secrets/mod.rs`

**Step 1: Add load_refresh_intervals**

```rust
impl SecretsService {
    fn load_refresh_intervals() -> HashMap<String, Duration> {
        let mut intervals = Self::default_refresh_intervals();

        if let Ok(json) = std::env::var("STATIC_SECRETS_REFRESH_INTERVALS") {
            match serde_json::from_str::<HashMap<String, u64>>(&json) {
                Ok(custom) => {
                    for (key, secs) in custom {
                        intervals.insert(key, Duration::from_secs(secs));
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Invalid STATIC_SECRETS_REFRESH_INTERVALS format");
                }
            }
        }

        intervals
    }
}
```

**Step 2: Use in constructors**

Replace `Self::default_refresh_intervals()` with `Self::load_refresh_intervals()` in `from_env()` and `new()`.

**Step 3: Test and commit**

```bash
cargo test -p runtime-settings
git add lib/runtime-settings && git commit -m "feat(runtime-settings): add STATIC_SECRETS_REFRESH_INTERVALS support"
```

---

## Task 5: Convert Watchers to Async-Only

**Files:**
- Modify: `lib/runtime-settings/src/watchers.rs`
- Modify: `lib/runtime-settings/src/settings.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Change Watcher type to async**

Replace current sync watcher with async:

```rust
use std::future::Future;
use std::pin::Pin;

/// Async watcher callback
pub type Watcher = Box<
    dyn Fn(Option<serde_json::Value>, Option<serde_json::Value>) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

struct WatcherEntry {
    id: WatcherId,
    callback: Watcher,
}
```

**Step 2: Update check() for async**

```rust
pub async fn check(&self, current_values: &HashMap<String, serde_json::Value>) {
    let watchers = self.watchers.read().unwrap();
    let mut snapshot = self.snapshot.write().unwrap();

    for (key, entries) in watchers.iter() {
        let old_value = snapshot.get(key).cloned();
        let new_value = current_values.get(key).cloned();

        if old_value != new_value {
            if let Some(ref v) = new_value {
                snapshot.insert(key.clone(), v.clone());
            } else {
                snapshot.remove(key);
            }

            for entry in entries {
                (entry.callback)(old_value.clone(), new_value.clone()).await;
            }
        }
    }
}
```

**Step 3: Update add() signature**

```rust
pub fn add(&self, key: &str, callback: Watcher) -> WatcherId {
    let id = WatcherId::next();
    let entry = WatcherEntry { id, callback };
    let mut watchers = self.watchers.write().unwrap();
    watchers.entry(key.to_string()).or_default().push(entry);
    id
}
```

**Step 4: Update tests**

```rust
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

    let mut current_values = HashMap::new();
    current_values.insert("KEY".to_string(), serde_json::json!("new_value"));

    service.check(&current_values).await;

    assert_eq!(called.load(Ordering::SeqCst), 1);
}
```

**Step 5: Test and commit**

```bash
cargo test -p runtime-settings
git add lib/runtime-settings && git commit -m "feat(runtime-settings): convert watchers to async-only"
```

---

## Task 6: Add Configurable Timeout to Refresh

**Files:**
- Modify: `lib/runtime-settings/src/error.rs`
- Modify: `lib/runtime-settings/src/settings.rs`

**Step 1: Add Timeout error**

```rust
#[error("Request timed out")]
Timeout,
```

**Step 2: Add refresh_with_timeout**

```rust
pub async fn refresh_with_timeout(&self, timeout: Duration) -> Result<(), SettingsError> {
    tokio::time::timeout(timeout, self.refresh())
        .await
        .map_err(|_| SettingsError::Timeout)?
}
```

**Step 3: Test and commit**

```bash
cargo test -p runtime-settings
git add lib/runtime-settings && git commit -m "feat(runtime-settings): add configurable timeout to refresh"
```

---

## Execution Order

1. Task 1: X-OperationId header
2. Task 2: Configurable refresh interval
3. Task 3: Secrets auto-renewal
4. Task 4: STATIC_SECRETS_REFRESH_INTERVALS
5. Task 5: Async-only watchers
6. Task 6: Configurable timeout

---

## Final Verification

```bash
cargo test -p runtime-settings
cargo clippy -p runtime-settings
cargo fmt
cargo build --workspace
```
