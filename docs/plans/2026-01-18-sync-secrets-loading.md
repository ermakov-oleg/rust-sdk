# Синхронная ленивая загрузка секретов

## Проблема

Нужно реализовать ленивую загрузку секретов с синхронным API `get()`.

### Почему ленивая загрузка важна

Не все инстансы приложения имеют доступ ко всем секретам:
- Один инстанс ходит только в БД
- Консьюмер ходит ещё и в Kafka

При eager загрузке будут запрашиваться лишние секреты, что приведёт к ошибкам доступа.

### Текущее состояние

- `get()` — синхронный метод
- `SecretsService::get()` — асинхронный (использует `vaultrs` с async HTTP)
- Секреты в настройках: `{"$secret": "path:key"}`

---

## Как делает Python

Python использует **синхронный HTTP клиент** для Vault:

```python
# cian_settings/_secrets/_secrets_service.py:36
def get_secret(self, path: str) -> 'Secret':
    if path in self._secrets:
        return self._secrets[path]  # Кэш

    # СИНХРОННЫЙ HTTP запрос
    response = self._client.request('GET', f'/v1/{path}')

    secret = Secret(path=path, response=response, valid_from=now)
    self._secrets[path] = secret
    return secret
```

Ключевые моменты:
1. Первый вызов блокирует поток (синхронный HTTP)
2. Результат кэшируется
3. Последующие вызовы мгновенные
4. Обновление секретов происходит в async `refresh()`

---

## Варианты решения для Rust

### Вариант A: Blocking HTTP клиент

```rust
fn get_sync(&self, path: &str, key: &str) -> Result<Value, SettingsError> {
    // Быстрый путь - кэш
    if let Some(cached) = self.cache.blocking_read().get(path) {
        return cached.value.get(key).cloned().ok_or(...);
    }

    // Медленный путь - блокирующий HTTP
    let client = reqwest::blocking::Client::new();
    let response = client.get(&url).send()?;
    self.cache.blocking_write().insert(path, ...);
    Ok(value)
}
```

**Проблема:** `reqwest::blocking` паникует если вызван из async контекста.

### Вариант B: spawn_blocking

```rust
fn get_sync(&self, path: &str, key: &str) -> Result<Value, SettingsError> {
    if let Some(cached) = self.cache.blocking_read().get(path) {
        return Ok(cached.value.get(key).cloned()?);
    }

    let handle = tokio::runtime::Handle::try_current();

    match handle {
        Ok(h) => {
            // Внутри async runtime
            std::thread::scope(|s| {
                s.spawn(|| {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(self.fetch_secret(path))
                }).join()
            })
        }
        Err(_) => {
            // Вне async runtime
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(self.fetch_secret(path))
        }
    }
}
```

**Проблема:** Сложная логика, создание runtime на каждый запрос.

### Вариант C: Отдельный thread pool для секретов (рекомендуется)

```rust
use rayon::ThreadPool;

lazy_static! {
    static ref SECRETS_EXECUTOR: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
}

fn get_sync(&self, path: &str, key: &str) -> Result<Value, SettingsError> {
    if let Some(cached) = self.cache.blocking_read().get(path) {
        return Ok(cached.value.get(key).cloned()?);
    }

    // Выполняем в отдельном thread pool
    let (tx, rx) = std::sync::mpsc::channel();
    let path = path.to_string();
    let client = self.client.clone();

    SECRETS_EXECUTOR.spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(fetch_secret(&client, &path));
        tx.send(result).ok();
    });

    let result = rx.recv().map_err(|_| SettingsError::SecretFetchFailed)?;

    // Кэшируем
    self.cache.blocking_write().insert(path, result.clone());

    Ok(result)
}
```

**Преимущества:**
- Нет риска deadlock
- Аналог Python подхода
- Thread pool переиспользуется

**Недостатки:**
- Дополнительные 2 потока
- Зависимость от rayon

### Вариант D: Полностью синхронный Vault клиент

Заменить `vaultrs` на синхронную реализацию или использовать `ureq`/`reqwest::blocking` напрямую.

```rust
struct SyncVaultClient {
    client: reqwest::blocking::Client,
    address: String,
    token: String,
}

impl SyncVaultClient {
    fn read_secret(&self, path: &str) -> Result<Value, SettingsError> {
        let url = format!("{}/v1/secret/data/{}", self.address, path);
        let response = self.client
            .get(&url)
            .header("X-Vault-Token", &self.token)
            .send()?;
        // ...
    }
}
```

**Преимущества:**
- Простота
- Нет thread pool
- Полный контроль

**Недостатки:**
- Паника если вызван из async контекста
- Нужно обрабатывать этот случай

---

## Рекомендация

**Вариант D (синхронный Vault клиент)** с защитой от вызова из async:

```rust
fn get_sync(&self, path: &str, key: &str) -> Result<Value, SettingsError> {
    // Кэш
    if let Some(cached) = self.cache.blocking_read().get(path) {
        return Ok(cached.value.get(key).cloned()?);
    }

    // Проверка контекста
    if tokio::runtime::Handle::try_current().is_ok() {
        // Мы внутри async runtime - используем spawn_blocking
        return Err(SettingsError::SyncCallFromAsyncContext);
        // Или: используем oneshot channel + spawn_blocking
    }

    // Синхронный запрос
    self.sync_client.read_secret(path, key)
}
```

Или документировать, что `get()` нельзя вызывать из async контекста для настроек с секретами, и предоставить `get_async()` для этого случая.

---

## Интеграция в get()

После реализации синхронной загрузки:

```rust
// В Setting
fn get_value_with_secrets<T>(&self, secrets: &SecretsService) -> Option<Arc<T>> {
    if self.has_secrets {
        let resolved = resolve_secrets_sync(&self.value, secrets)?;
        return serde_json::from_value(resolved).ok().map(Arc::new);
    }
    self.get_value()
}

// В RuntimeSettings::get()
fn get<T>(&self, key: &str) -> Option<Arc<T>> {
    let setting = self.find_matching_setting(key)?;
    setting.get_value_with_secrets(&self.secrets)
}
```

---

## Зависимости

- Решение влияет на: Task 5 (Integrate secrets resolution in get())
- Блокирует: полную интеграцию секретов в синхронный API

---

## TODO

- [ ] Выбрать вариант реализации
- [ ] Реализовать синхронный Vault клиент или thread pool
- [ ] Добавить метод `has_secrets` в Setting для быстрой проверки
- [ ] Интегрировать в `get()`
- [ ] Добавить тесты
