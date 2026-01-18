# Runtime Settings: Python ↔ Rust Parity Plan

Сравнение реализаций runtime-settings между Python и Rust.
Цель — достичь функционального паритета.

## Провайдеры настроек

| Провайдер | Python | Rust | Статус |
|-----------|--------|------|--------|
| Environment | ✅ | ✅ | Паритет |
| File (JSON5) | ✅ | ✅ | Паритет |
| Consul | ✅ | ❌ | Не требуется |
| MCS | ✅ | ✅ | Паритет |

## Задачи на исправление

### 1. Фильтры

#### 1.1 Добавить IGNORECASE в regex фильтры
**Приоритет:** Высокий

Python использует `re.IGNORECASE | re.VERBOSE` для regex фильтров.
Rust использует стандартный regex без флагов.

**Затронутые фильтры:**
- `application` (static)
- `server` (static)
- `url-path` (dynamic)
- `host` (dynamic)
- `email` (dynamic)
- `ip` (dynamic)

**Решение:** Добавить `(?i)` prefix или использовать `RegexBuilder::case_insensitive(true)`.

#### 1.2 Рассмотреть PEP 440 вместо semver для library_version
**Приоритет:** Средний

Python использует PEP 440 (packaging library) для спецификаций версий.
Rust использует semver crate.

**Вопрос:** Нужна ли совместимость с PEP 440 или semver достаточно для Rust экосистемы?

---

### 2. Secrets (Vault интеграция)

#### 2.1 Реализовать авто-обновление lease
**Приоритет:** Высокий

Python обновляет lease на 75% от lease_duration.
Rust имеет TODO в `SecretsService::refresh()`.

**Требуется:**
- Отслеживание `lease_duration` и `fetched_at`
- Background task для обновления перед истечением
- Логика: `elapsed >= lease_duration * 0.75`

#### 2.2 Добавить поддержку static secrets
**Приоритет:** Средний

Python поддерживает non-renewable секреты с фиксированными интервалами:
- `kafka-certificates`: 600s
- `interservice-auth`: 60s

**Требуется:**
- Определение типа секрета (renewable vs static)
- Конфигурируемые интервалы обновления

#### 2.3 Добавить STATIC_SECRETS_REFRESH_INTERVALS env var
**Приоритет:** Низкий

Python читает JSON из env var для кастомных интервалов.

```json
{"my-secret": 120, "another-secret": 300}
```

#### 2.4 Интегрировать разрешение секретов в get()
**Приоритет:** Высокий

Python автоматически разрешает `{"$secret": "path:key"}` при вызове `get()`.
Rust требует отдельный вызов resolver.

**Решение:** Интегрировать `secrets::resolver` в `Setting::get_value()` или добавить `get_with_secrets()`.

---

### 3. Watchers

#### 3.1 Добавить поддержку async watchers
**Приоритет:** Средний

Python поддерживает как sync так и async callback'и.
Rust поддерживает только sync.

**Решение:** Добавить `AsyncWatcherCallback` trait или использовать `BoxFuture`.

#### 3.2 Реализовать параллельный вызов watchers
**Приоритет:** Низкий

Python использует `asyncio.gather` для параллельного вызова.
Rust вызывает последовательно.

**Решение:** Использовать `tokio::join!` или `futures::join_all`.

---

### 4. Testing

#### 4.1 Создать модуль testutils с FakeSettings
**Приоритет:** Высокий

Python предоставляет `FakeSettings` класс:
```python
fake = FakeSettings(runtime_settings)
fake.set({"KEY": "value"}, priority=10**20)
fake.delete("KEY")
fake.reset()
```

**Требуется:**
- `FakeSettings` struct
- `set(settings, priority, filter)` метод
- `delete(key, priority)` метод
- `reset()` метод
- Приоритет 10^20 для override production settings

---

### 5. API

#### 5.1 Добавить configurable timeout в refresh()
**Приоритет:** Низкий

Python: `refresh(timeout=2.0)`
Rust: `refresh()` без параметров

**Решение:** `refresh_with_timeout(Duration)` или builder pattern.

---

### 6. MCS Provider

#### 6.1 Добавить X-OperationId header
**Приоритет:** Средний

Python отправляет `X-OperationId: <uuid>` для трейсинга запросов.

**Решение:** Генерировать UUID v4 и добавлять в headers.

---

### 7. Setup

#### 7.1 Сделать refresh interval конфигурируемым
**Приоритет:** Низкий

Rust: hardcoded 30s в `setup.rs`
Python: caller-controlled

**Решение:** Добавить в `RuntimeSettingsBuilder::refresh_interval(Duration)`.

---

## Приоритизация

### Must Have (блокеры для production)
1. IGNORECASE в regex фильтрах
2. Авто-обновление lease
3. Интеграция секретов в get()
4. FakeSettings для тестирования

### Should Have (улучшения)
1. Static secrets с интервалами
2. X-OperationId header
3. Async watchers

### Nice to Have (можно отложить)
1. PEP 440 для library_version
2. STATIC_SECRETS_REFRESH_INTERVALS
3. Параллельный вызов watchers
4. Configurable timeout в refresh()
5. Configurable refresh interval
