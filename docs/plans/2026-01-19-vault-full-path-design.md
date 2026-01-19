# Vault Full Path Support

## Problem

В текущей Rust реализации mount для Vault захардкожен как `"secret"`:

```rust
// lib/runtime-settings/src/secrets/mod.rs:188
client.kv_read("secret", path)
```

В Python реализации путь передается полностью, включая mount:

```python
# cian_settings/_secrets/_secrets_service.py:36
response = self._client.request('GET', f'/v1/{path}')
```

## Solution

Добавить метод `kv_read_raw` в `VaultClient`, который принимает полный путь.

### Новый формат секретов в настройках

```json
{"$secret": "secret/data/database/creds:password"}
```

Вместо текущего:

```json
{"$secret": "database/creds:password"}
```

## Implementation

### Step 1: Add `kv_read_raw` method to VaultClient

File: `lib/vault-client/src/client.rs`

```rust
pub async fn kv_read_raw(&self, full_path: &str) -> Result<KvData, VaultError> {
    let url = format!("{}/v1/{}", self.base_url, full_path);
    // ... same parsing logic as kv_read
}
```

### Step 2: Update SecretsService to use `kv_read_raw`

File: `lib/runtime-settings/src/secrets/mod.rs`

Replace:
```rust
client.kv_read("secret", path)
```

With:
```rust
client.kv_read_raw(path)
```

### Step 3: Update tests

- Update mock responses to use full paths
- Update test settings to use new format with `secret/data/` prefix

## Files to modify

1. `lib/vault-client/src/client.rs` - add `kv_read_raw` method
2. `lib/runtime-settings/src/secrets/mod.rs` - use `kv_read_raw` instead of `kv_read`
3. `lib/runtime-settings/tests/integration_vault.rs` - update test paths
