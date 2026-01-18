# Синхронная ленивая загрузка секретов — Дизайн

## Решения

| Вопрос | Решение |
|--------|---------|
| Как вызвать async из sync? | `tokio::task::block_in_place` + `Handle::current().block_on()` |
| Как определять настройки с секретами? | `secrets_usages: Vec<SecretUsage>` при compile() |
| Как инвалидировать кэш? | Версионирование: `AtomicU64` в SecretsService, сравнение в Setting |
| Когда инкрементить версию? | Только при реальном изменении секрета |

---

## Структуры данных

### SecretUsage

```rust
#[derive(Debug, Clone)]
pub enum JsonPathKey {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone)]
pub struct SecretUsage {
    pub path: String,                  // Vault path: "db/creds"
    pub key: String,                   // ключ в секрете: "password"
    pub value_path: Vec<JsonPathKey>,  // где подставить: ["connection", "password"]
}
```

### SecretsService (изменения)

```rust
pub struct SecretsService {
    client: Option<VaultClient>,
    cache: RwLock<HashMap<String, CachedSecret>>,
    refresh_intervals: HashMap<String, Duration>,
    version: AtomicU64,  // NEW
}

impl SecretsService {
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn get_sync(&self, path: &str, key: &str) -> Result<serde_json::Value, SettingsError> {
        // Быстрый путь — кэш
        {
            let cache = self.cache.blocking_read();
            if let Some(cached) = cache.get(path) {
                if let Some(value) = cached.value.get(key) {
                    return Ok(value.clone());
                }
            }
        }

        // Медленный путь — загрузка из Vault
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.fetch_and_cache(path, key))
        })
    }

    pub async fn refresh(&self) -> Result<(), SettingsError> {
        let mut changed = false;

        for path in paths_to_refresh {
            if self.refresh_secret(&path).await? {
                changed = true;
            }
        }

        if changed {
            self.version.fetch_add(1, Ordering::Release);
        }

        Ok(())
    }

    /// Возвращает true если секрет изменился
    async fn refresh_secret(&self, path: &str) -> Result<bool, SettingsError> {
        let new_value = self.fetch_from_vault(path).await?;

        let mut cache = self.cache.write().await;
        let changed = cache
            .get(path)
            .map(|cached| cached.value != new_value)
            .unwrap_or(true);

        cache.insert(path.to_string(), CachedSecret {
            value: new_value,
            fetched_at: Instant::now(),
            // ...
        });

        Ok(changed)
    }
}
```

### Setting (изменения)

```rust
pub struct Setting {
    pub key: String,
    pub priority: i64,
    pub value: serde_json::Value,
    pub static_filters: Vec<Box<dyn CompiledStaticFilter>>,
    pub dynamic_filters: Vec<Box<dyn CompiledDynamicFilter>>,
    value_cache: DashMap<TypeId, Arc<dyn Any + Send + Sync>>,
    secrets_usages: Vec<SecretUsage>,  // NEW
    cached_at_version: AtomicU64,       // NEW
}

impl Setting {
    pub fn compile(raw: RawSetting) -> Result<Self, SettingsError> {
        // ... existing filter compilation ...

        let secrets_usages = find_secret_usages(&raw.value)?;

        Ok(Setting {
            // ...
            secrets_usages,
            cached_at_version: AtomicU64::new(0),
        })
    }

    #[inline]
    pub fn has_secrets(&self) -> bool {
        !self.secrets_usages.is_empty()
    }

    fn invalidate_if_stale(&self, secrets_version: u64) {
        let cached = self.cached_at_version.load(Ordering::Acquire);
        if cached != secrets_version {
            self.value_cache.clear();
            self.cached_at_version.store(secrets_version, Ordering::Release);
        }
    }

    pub fn get_value_with_secrets<T>(&self, secrets: &SecretsService) -> Option<Arc<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();

        // Проверяем кэш
        if let Some(cached) = self.value_cache.get(&type_id) {
            return Arc::downcast::<T>(Arc::clone(cached.value())).ok();
        }

        // Резолвим секреты
        let resolved = if self.has_secrets() {
            resolve_secrets_sync(&self.value, &self.secrets_usages, secrets).ok()?
        } else {
            self.value.clone()
        };

        // Десериализуем и кэшируем
        let value: T = serde_json::from_value(resolved).ok()?;
        let arc_value = Arc::new(value);
        self.value_cache.insert(type_id, Arc::clone(&arc_value) as Arc<dyn Any + Send + Sync>);

        Some(arc_value)
    }
}
```

---

## Resolve секретов

```rust
pub fn resolve_secrets_sync(
    value: &serde_json::Value,
    usages: &[SecretUsage],
    secrets: &SecretsService,
) -> Result<serde_json::Value, SettingsError> {
    if usages.is_empty() {
        return Ok(value.clone());
    }

    let mut result = value.clone();

    for usage in usages {
        let secret_value = secrets.get_sync(&usage.path, &usage.key)?;
        set_at_path(&mut result, &usage.value_path, secret_value)?;
    }

    Ok(result)
}

fn set_at_path(
    root: &mut serde_json::Value,
    path: &[JsonPathKey],
    value: serde_json::Value,
) -> Result<(), SettingsError> {
    // Навигация по path и установка значения
    // ...
}
```

---

## Интеграция в get()

```rust
impl RuntimeSettings {
    fn get_internal<T>(&self, key: &str, ctx: &DynamicContext) -> Option<Arc<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let state = self.state.read().unwrap();
        let settings = state.settings.get(key)?;

        for setting in settings {
            if setting.check_dynamic_filters(ctx) {
                // Инвалидация кэша если секреты обновились
                if setting.has_secrets() {
                    setting.invalidate_if_stale(self.secrets.version());
                }

                return setting.get_value_with_secrets(&self.secrets);
            }
        }

        None
    }
}
```

---

## Парсинг secret usages при compile()

```rust
pub fn find_secret_usages(value: &serde_json::Value) -> Result<Vec<SecretUsage>, SettingsError> {
    let mut usages = Vec::new();
    find_secrets_recursive(value, &mut Vec::new(), &mut usages)?;
    Ok(usages)
}

fn find_secrets_recursive(
    value: &serde_json::Value,
    current_path: &mut Vec<JsonPathKey>,
    usages: &mut Vec<SecretUsage>,
) -> Result<(), SettingsError> {
    match value {
        serde_json::Value::Object(map) => {
            if map.len() == 1 {
                if let Some(serde_json::Value::String(reference)) = map.get("$secret") {
                    let (path, key) = parse_secret_ref(reference)?;
                    usages.push(SecretUsage {
                        path,
                        key,
                        value_path: current_path.clone(),
                    });
                    return Ok(());
                }
            }

            for (field, v) in map {
                current_path.push(JsonPathKey::Field(field.clone()));
                find_secrets_recursive(v, current_path, usages)?;
                current_path.pop();
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                current_path.push(JsonPathKey::Index(i));
                find_secrets_recursive(v, current_path, usages)?;
                current_path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_secret_ref(reference: &str) -> Result<(String, String), SettingsError> {
    reference
        .split_once(':')
        .map(|(p, k)| (p.to_string(), k.to_string()))
        .ok_or_else(|| SettingsError::InvalidSecretReference {
            reference: reference.to_string(),
        })
}
```

---

## Flow

```
get("DB_CONFIG")
    │
    ├─► find matching Setting
    │
    ├─► setting.has_secrets()? ──► yes ──► invalidate_if_stale(secrets.version())
    │                                              │
    │                                              ▼
    │                                      version changed?
    │                                              │
    │                              ┌───────────────┴───────────────┐
    │                              ▼                               ▼
    │                           no                              yes
    │                              │                               │
    │                              │                     value_cache.clear()
    │                              │                               │
    │                              └───────────────┬───────────────┘
    │                                              ▼
    ├─► check value_cache ─────────────────► hit? ──► return cached
    │                                              │
    │                                             miss
    │                                              │
    │                                              ▼
    │                                     resolve_secrets_sync()
    │                                              │
    │                                              ▼
    │                                   for usage in secrets_usages:
    │                                       secrets.get_sync(path, key)
    │                                              │
    │                               ┌──────────────┴──────────────┐
    │                               ▼                             ▼
    │                           in cache                      not in cache
    │                               │                             │
    │                               │                   block_in_place {
    │                               │                     block_on(fetch_from_vault)
    │                               │                   }
    │                               │                             │
    │                               └──────────────┬──────────────┘
    │                                              ▼
    │                                     set_at_path(result, path, value)
    │                                              │
    │                                              ▼
    │                                   serde_json::from_value::<T>()
    │                                              │
    │                                              ▼
    │                                     cache in value_cache
    │                                              │
    └──────────────────────────────────────────────┴──► return Arc<T>
```

---

## Ограничения

1. **Multi-threaded runtime only** — `block_in_place` паникует в current_thread runtime
2. **Первый get() с секретом блокирует** — это ожидаемо, происходит при старте

---

## TODO для реализации

- [ ] Добавить `JsonPathKey` и `SecretUsage` в secrets/mod.rs
- [ ] Добавить `find_secret_usages()` и `parse_secret_ref()`
- [ ] Добавить `version: AtomicU64` в SecretsService
- [ ] Добавить `get_sync()` в SecretsService
- [ ] Изменить `refresh()` — инкрементить version только при изменениях
- [ ] Добавить `secrets_usages` и `cached_at_version` в Setting
- [ ] Обновить `Setting::compile()` — парсить secret usages
- [ ] Добавить `resolve_secrets_sync()` и `set_at_path()`
- [ ] Добавить `get_value_with_secrets()` в Setting
- [ ] Обновить `RuntimeSettings::get_internal()` — использовать новый метод
- [ ] Тесты
