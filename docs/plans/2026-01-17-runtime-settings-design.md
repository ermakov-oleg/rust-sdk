# Runtime Settings — Rust Implementation Design

Полная реализация аналога Python cian-settings на Rust.

## Требования

| Аспект | Решение |
|--------|---------|
| Провайдеры | env, file, MCS (без Consul) |
| Фильтры | Все 12 типов |
| Vault | vaultrs |
| Watchers | Да |
| Scoped-контексты | Thread-local + task-local |
| Существующий код | Переписать с нуля |
| Совместимость | Полная с Python, `runtime=rust` |

---

## Часть 1 — Структура модулей

```
lib/runtime-settings/src/
├── lib.rs                 # Публичный API, реэкспорты
├── settings.rs            # RuntimeSettings — главная структура
├── entities.rs            # Setting, SettingKey, Response
├── context.rs             # Context, Request, StaticContext
├── filters/
│   ├── mod.rs             # FilterService trait, registry
│   ├── static_filters.rs  # application, server, environment, mcs_run_env, library_version
│   └── dynamic_filters.rs # url_path, host, email, ip, header, context, probability
├── providers/
│   ├── mod.rs             # SettingsProvider trait
│   ├── env.rs             # EnvProvider (приоритет -10^18)
│   ├── file.rs            # FileProvider (приоритет из файла или 10^18)
│   └── mcs.rs             # McsProvider (HTTP, дельта-обновления)
├── secrets/
│   ├── mod.rs             # SecretsService
│   ├── secret.rs          # Secret, SecretLease
│   └── resolver.rs        # Резолвинг $secret в значениях
├── watchers.rs            # WatchersService, подписка на изменения
├── scoped.rs              # Thread-local/task-local контексты
└── error.rs               # Типы ошибок
```

**Ключевые принципы:**
- Разделение на статические и динамические фильтры (как в Python)
- Провайдеры — отдельные модули с общим trait
- Secrets — изолированный модуль для Vault-интеграции
- Scoped — отдельный модуль для управления контекстами

---

## Часть 2 — Основные структуры данных

```rust
// entities.rs

/// Одна настройка из любого источника
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub priority: i64,  // i64 для поддержки отрицательных (env: -10^18)
    #[serde(default)]
    pub filter: HashMap<String, String>,
    pub value: serde_json::Value,  // Храним как JSON Value, не String
}

/// Идентификатор для удаления настройки
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingKey {
    pub key: String,
    pub priority: i64,
}

/// Ответ от MCS
#[derive(Debug, Deserialize)]
pub struct McsResponse {
    pub settings: Vec<Setting>,
    pub deleted: Vec<SettingKey>,
    pub version: String,
}

// context.rs

/// HTTP-запрос (аналог Python Request)
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,  // Case-insensitive lookup
}

impl Request {
    pub fn host(&self) -> Option<&str> { /* из header "host" */ }
    pub fn ip(&self) -> Option<&str> { /* из header "x-real-ip" */ }
    pub fn email(&self) -> Option<&str> { /* из header "x-real-email" */ }
}

/// Полный контекст для фильтрации
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub application: String,
    pub server: String,
    pub environment: HashMap<String, String>,
    pub libraries_versions: HashMap<String, Version>,  // semver::Version
    pub mcs_run_env: Option<String>,
    // Динамические (из request или вручную)
    pub request: Option<Request>,
    pub custom: HashMap<String, String>,  // Произвольный контекст
}
```

**Отличия от текущей реализации:**
- `value` как `serde_json::Value` вместо `Option<String>` — проще работать с вложенными структурами и секретами
- `priority` как `i64` — поддержка отрицательных приоритетов для env
- `Request` отдельно от `Context` — как в Python
- `libraries_versions` использует `semver::Version` для PEP 440-подобных сравнений

---

## Часть 3 — Система фильтрации

```rust
// filters/mod.rs

/// Результат проверки фильтра
pub enum FilterResult {
    Match,
    NoMatch,
    NotApplicable,  // Фильтр не применим (нет данных в контексте)
}

/// Статический фильтр — проверяется один раз при загрузке
pub trait StaticFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult;
}

/// Динамический фильтр — проверяется при каждом get()
pub trait DynamicFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult;
}

// filters/static_filters.rs

/// application: regex против ctx.application
pub struct ApplicationFilter;

/// server: regex против ctx.server
pub struct ServerFilter;

/// environment: "KEY1=val1,KEY2=val2" против ctx.environment
pub struct EnvironmentFilter;

/// mcs_run_env: regex против ctx.mcs_run_env
pub struct McsRunEnvFilter;

/// library_version: "pkg>=1.0,<2.0" (semver) против ctx.libraries_versions
pub struct LibraryVersionFilter;

// filters/dynamic_filters.rs

/// url-path: regex против ctx.request.path
pub struct UrlPathFilter;

/// host: regex против ctx.request.host()
pub struct HostFilter;

/// email: regex против ctx.request.email()
pub struct EmailFilter;

/// ip: regex против ctx.request.ip()
pub struct IpFilter;

/// header: "KEY1=val1,KEY2=val2" против ctx.request.headers
pub struct HeaderFilter;

/// context: "KEY1=val1,KEY2=val2" против ctx.custom
pub struct ContextFilter;

/// probability: "25" — 25% шанс Match
pub struct ProbabilityFilter;
```

**Логика применения:**
1. При загрузке настройки — прогоняем через статические фильтры. Если `NoMatch` — настройка отбрасывается сразу
2. При `get()` — прогоняем подходящие настройки через динамические фильтры
3. `NotApplicable` не влияет на результат (фильтр пропускается если нет данных)
4. Все regex — case-insensitive, автоматически оборачиваются в `^...$`

**probability** — особый случай: использует thread-local RNG, кэширует результат на время запроса (чтобы повторные вызовы `get()` давали одинаковый результат).

---

## Часть 4 — Провайдеры настроек

```rust
// providers/mod.rs

#[async_trait]
pub trait SettingsProvider: Send + Sync {
    /// Загрузить настройки. Возвращает (settings, deleted, new_version)
    async fn load(&self, current_version: &str) -> Result<ProviderResponse, ProviderError>;

    /// Приоритет провайдера по умолчанию (если не указан в настройке)
    fn default_priority(&self) -> i64;
}

pub struct ProviderResponse {
    pub settings: Vec<Setting>,
    pub deleted: Vec<SettingKey>,
    pub version: String,
}

// providers/env.rs — EnvProvider

/// Читает все переменные окружения как настройки
/// - Приоритет: -10^18 (самый низкий)
/// - Без фильтров
/// - Значение парсится как JSON, если не получается — как строка
pub struct EnvProvider {
    environ: HashMap<String, String>,  // Можно передать кастомный env для тестов
}

// providers/file.rs — FileProvider

/// Читает JSON файл с массивом настроек
/// - Путь: RUNTIME_SETTINGS_FILE_PATH или "runtime-settings.json"
/// - Приоритет по умолчанию: 10^18
/// - Поддержка JSON5 (комментарии, trailing commas) через json5
pub struct FileProvider {
    path: PathBuf,
}

// providers/mcs.rs — McsProvider

/// HTTP провайдер от Microservice Configuration Service
/// - URL: {base_url}/v3/get-runtime-settings/
/// - Метод: POST
/// - Body: { runtime: "rust", version, application, mcs_run_env }
/// - Поддержка дельта-обновлений через version
pub struct McsProvider {
    base_url: String,      // RUNTIME_SETTINGS_BASE_URL
    application: String,
    mcs_run_env: Option<String>,
    client: reqwest::Client,
}
```

**Порядок загрузки:**
1. `EnvProvider` — один раз при инициализации
2. `FileProvider` — один раз при инициализации
3. `McsProvider` — при инициализации + периодически (refresh)

**Слияние:** настройки объединяются в `HashMap<String, Vec<SettingEntry>>`, где вектор отсортирован по приоритету (descending). При `get()` берётся первая подходящая.

---

## Часть 5 — Интеграция с Vault

```rust
// secrets/mod.rs

pub struct SecretsService {
    client: Option<vaultrs::client::VaultClient>,
    secrets: RwLock<HashMap<String, Secret>>,  // path -> Secret
    refresh_intervals: HashMap<String, Duration>,  // Для static секретов
}

impl SecretsService {
    /// Получить значение секрета по пути и ключу
    pub async fn get(&self, path: &str, key: &str) -> Result<serde_json::Value, SecretError>;

    /// Обновить все секреты (renewal, reissue если нужно)
    pub async fn refresh(&self) -> Result<(), SecretError>;
}

// secrets/secret.rs

pub struct Secret {
    path: String,
    value: serde_json::Value,  // Всё содержимое секрета
    lease: Option<SecretLease>,
    last_refresh: Instant,
}

pub struct SecretLease {
    lease_id: String,
    lease_duration: Duration,
    renewable: bool,
}

// secrets/resolver.rs

/// Рекурсивно обходит JSON Value и заменяет {"$secret": "path:key"} на реальные значения
pub async fn resolve_secrets(
    value: &serde_json::Value,
    secrets_service: &SecretsService,
) -> Result<serde_json::Value, SecretError>;
```

**Формат ссылки на секрет:**
```json
{
    "database": {
        "host": "db.internal",
        "password": { "$secret": "database/prod:password" }
    }
}
```

**Логика обновления:**
- Renewable секреты: обновлять на 75% срока жизни
- Static секреты (kafka-certificates, interservice-auth): по интервалам из `STATIC_SECRETS_REFRESH_INTERVALS`
- При смене Vault токена — переполучить все секреты

**Если Vault не сконфигурирован** (`client: None`), а в настройке есть `$secret` — возвращаем ошибку `SecretWithoutVault`.

---

## Часть 6 — Watchers (подписка на изменения)

```rust
// watchers.rs

/// Callback при изменении настройки
pub type Watcher = Box<dyn Fn(Option<serde_json::Value>, Option<serde_json::Value>) + Send + Sync>;

/// Async callback
pub type AsyncWatcher = Box<dyn Fn(Option<serde_json::Value>, Option<serde_json::Value>) -> BoxFuture<'static, ()> + Send + Sync>;

pub struct WatchersService {
    /// key -> список watchers
    watchers: RwLock<HashMap<String, Vec<WatcherEntry>>>,
    /// Снапшот значений для сравнения
    snapshot: RwLock<HashMap<String, serde_json::Value>>,
}

enum WatcherEntry {
    Sync(Watcher),
    Async(AsyncWatcher),
}

impl WatchersService {
    /// Добавить watcher на ключ
    pub fn add(&self, key: &str, watcher: Watcher) -> WatcherId;
    pub fn add_async(&self, key: &str, watcher: AsyncWatcher) -> WatcherId;

    /// Удалить watcher
    pub fn remove(&self, id: WatcherId);

    /// Проверить изменения и вызвать watchers
    /// Вызывается после refresh()
    pub async fn check(&self, settings: &RuntimeSettings, ctx: &Context);
}

pub struct WatcherId(u64);  // Уникальный идентификатор для удаления
```

**Логика:**
1. При `add()` — сохраняем текущее значение в snapshot
2. При `check()` — сравниваем текущее значение с snapshot
3. Если отличается — вызываем watcher с `(old_value, new_value)`
4. Обновляем snapshot
5. Ошибки в watchers логируются, но не прерывают процесс

**Отличие от Python:** возвращаем `WatcherId` вместо передачи самого watcher для удаления — проще управлять lifecycle.

---

## Часть 7 — Scoped-контексты (thread-local + task-local)

```rust
// scoped.rs

use std::cell::RefCell;
use tokio::task_local;

thread_local! {
    static THREAD_CONTEXT: RefCell<Option<Context>> = RefCell::new(None);
    static THREAD_REQUEST: RefCell<Option<Request>> = RefCell::new(None);
}

task_local! {
    static TASK_CONTEXT: Option<Context>;
    static TASK_REQUEST: Option<Request>;
}

/// Получить текущий контекст (task-local приоритетнее thread-local)
pub fn current_context() -> Option<Context> {
    TASK_CONTEXT.try_with(|c| c.clone()).ok().flatten()
        .or_else(|| THREAD_CONTEXT.with(|c| c.borrow().clone()))
}

pub fn current_request() -> Option<Request> {
    TASK_REQUEST.try_with(|r| r.clone()).ok().flatten()
        .or_else(|| THREAD_REQUEST.with(|r| r.borrow().clone()))
}

/// Guard для thread-local контекста
pub struct ContextGuard { /* восстанавливает предыдущее значение при drop */ }

impl RuntimeSettings {
    /// Установить thread-local контекст (sync код)
    pub fn set_context(&self, ctx: Context) -> ContextGuard;

    /// Установить thread-local request (sync код)
    pub fn set_request(&self, req: Request) -> RequestGuard;

    /// Выполнить async closure с task-local контекстом
    pub async fn with_context<F, T>(&self, ctx: Context, f: F) -> T
    where
        F: Future<Output = T>,
    {
        TASK_CONTEXT.scope(Some(ctx), f).await
    }

    /// Выполнить async closure с task-local request
    pub async fn with_request<F, T>(&self, req: Request, f: F) -> T
    where
        F: Future<Output = T>;
}
```

**Использование:**

```rust
// Sync (thread-local)
{
    let _guard = settings.set_request(request);
    let value = settings.get::<String>("KEY");  // Контекст из thread-local
}  // guard dropped, контекст восстановлен

// Async (task-local)
settings.with_request(request, async {
    let value = settings.get::<String>("KEY");  // Контекст из task-local
}).await;
```

**Приоритет:** task-local > thread-local > panic если не установлен

---

## Часть 8 — RuntimeSettings и публичный API

```rust
// settings.rs

pub struct RuntimeSettings {
    // Провайдеры
    providers: Vec<Box<dyn SettingsProvider>>,

    // Состояние
    state: RwLock<SettingsState>,

    // Сервисы
    secrets: SecretsService,
    watchers: WatchersService,

    // Статический контекст (не меняется после init)
    static_context: StaticContext,
}

struct SettingsState {
    version: String,
    settings: HashMap<String, Vec<SettingEntry>>,  // key -> sorted by priority desc
}

struct SettingEntry {
    setting: Setting,
    static_filters_passed: bool,  // Результат статической фильтрации
}

impl RuntimeSettings {
    /// Создать builder для конфигурации
    pub fn builder() -> RuntimeSettingsBuilder;

    /// Инициализация (загрузка из всех провайдеров)
    pub async fn init(&self) -> Result<(), SettingsError>;

    /// Обновить настройки (MCS + secrets + watchers)
    pub async fn refresh(&self) -> Result<(), SettingsError>;

    /// Получить значение (использует scoped-контекст, паника если не установлен)
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T>;

    /// Получить с default значением
    pub fn get_or<T: DeserializeOwned>(&self, key: &str, default: T) -> T;

    /// Создать getter-функцию (как make_getter в Python)
    pub fn getter<T: DeserializeOwned + Clone>(&self, key: &str, default: T) -> impl Fn() -> T;

    /// Watchers
    pub fn add_watcher(&self, key: &str, watcher: Watcher) -> WatcherId;
    pub fn remove_watcher(&self, id: WatcherId);

    /// Scoped contexts
    pub fn set_context(&self, ctx: Context) -> ContextGuard;
    pub fn set_request(&self, req: Request) -> RequestGuard;
    pub async fn with_context<F, T>(&self, ctx: Context, f: F) -> T where F: Future<Output = T>;
    pub async fn with_request<F, T>(&self, req: Request, f: F) -> T where F: Future<Output = T>;
}
```

**Builder pattern:**
```rust
let settings = RuntimeSettings::builder()
    .application("my-service")
    .server(hostname)
    .mcs_enabled(true)
    .vault_client(vault_client)
    .library_version("my-lib", "1.2.3")
    .build()?;
```

**Глобальный доступ (опционально):**
```rust
// setup.rs
lazy_static! {
    pub static ref SETTINGS: RuntimeSettings = ...;
}

pub async fn setup() { ... }
```

---

## Часть 9 — Обработка ошибок

```rust
// error.rs

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    // Провайдеры
    #[error("Failed to read settings file: {0}")]
    FileRead(#[from] std::io::Error),

    #[error("Failed to parse settings JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("MCS request failed: {0}")]
    McsRequest(#[from] reqwest::Error),

    #[error("MCS returned error: {status}, {message}")]
    McsResponse { status: u16, message: String },

    // Секреты
    #[error("Secret not found: {path}")]
    SecretNotFound { path: String },

    #[error("Secret key not found: {key} in {path}")]
    SecretKeyNotFound { path: String, key: String },

    #[error("Invalid secret reference: {reference}")]
    InvalidSecretReference { reference: String },

    #[error("Secret used but Vault not configured")]
    SecretWithoutVault,

    #[error("Vault error: {0}")]
    Vault(#[from] vaultrs::error::ClientError),

    // Фильтры
    #[error("Invalid regex pattern: {pattern}, {error}")]
    InvalidRegex { pattern: String, error: String },

    #[error("Invalid version specifier: {spec}")]
    InvalidVersionSpec { spec: String },

    // Runtime
    #[error("Context not set — call set_context() or with_context() first")]
    ContextNotSet,
}

/// Ошибки при получении значения (не прерывают работу)
#[derive(Debug, thiserror::Error)]
pub enum GetError {
    #[error("Key not found: {0}")]
    NotFound(String),

    #[error("Failed to deserialize value: {0}")]
    Deserialize(#[from] serde_json::Error),

    #[error("Secret resolution failed: {0}")]
    Secret(#[from] SecretError),
}
```

**Стратегия:**
- `init()` и `refresh()` возвращают `Result<(), SettingsError>` — caller решает что делать
- `get()` возвращает `Option<T>` — ошибки десериализации логируются, возвращается `None`
- Паника только при отсутствии контекста
- Ошибки в watchers логируются, не прерывают процесс

---

## Часть 10 — Зависимости и пример использования

**Cargo.toml:**
```toml
[dependencies]
# Async runtime
tokio = { version = "1", features = ["rt", "sync", "time", "macros"] }
async-trait = "0.1"
futures = "0.3"

# HTTP
reqwest = { version = "0.11", features = ["json"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
json5 = "0.4"  # Для файлов с комментариями

# Vault
vaultrs = "0.7"

# Utilities
thiserror = "1"
tracing = "0.1"
lazy_static = "1.4"
regex = "1"
semver = "1"  # Для library_version фильтра
rand = "0.8"  # Для probability фильтра
```

**Пример использования:**
```rust
use runtime_settings::{RuntimeSettings, Context, Request};

#[tokio::main]
async fn main() {
    // Инициализация
    let settings = RuntimeSettings::builder()
        .application("my-service")
        .mcs_enabled(true)
        .build()
        .unwrap();

    settings.init().await.unwrap();

    // Фоновое обновление
    let s = settings.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if let Err(e) = s.refresh().await {
                tracing::error!("Settings refresh failed: {}", e);
            }
        }
    });

    // В обработчике запроса
    let request = Request { method: "GET".into(), path: "/api/users".into(), headers };
    let _guard = settings.set_request(request);

    let feature_enabled: bool = settings.get_or("FEATURE_FLAG", false);
    let db_url: String = settings.get("DATABASE_URL").expect("DATABASE_URL required");
}
```
