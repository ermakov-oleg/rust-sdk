# vault-client Library Design

Rust библиотека для работы с HashiCorp Vault.

## Overview

Библиотека `vault-client` предоставляет async API для работы с Vault с автоматическим определением окружения и методов аутентификации. Используется внутри `runtime-settings` вместо прямого использования `vaultrs`.

## File Structure

```
lib/vault-client/
├── Cargo.toml
└── src/
    ├── lib.rs              # pub use, публичный API
    ├── client.rs           # VaultClient, VaultClientBuilder
    ├── auth/
    │   ├── mod.rs          # AuthMethod trait, TokenManager
    │   ├── token.rs        # StaticTokenAuth
    │   ├── kubernetes.rs   # KubernetesAuth
    │   └── oidc.rs         # OidcAuth + browser + callback server
    ├── kv.rs               # kv_read, kv_metadata реализация
    ├── models.rs           # KvData, KvMetadata, KvVersion
    ├── error.rs            # VaultError enum
    └── cache.rs            # OidcTokenCache (disk)
```

## Public API

### VaultClient

```rust
pub struct VaultClient { /* ... */ }

impl VaultClient {
    /// Автоматически определяет окружение и метод аутентификации:
    /// 1. Если есть VAULT_TOKEN → static token
    /// 2. Если есть KUBERNETES_SERVICE_HOST → K8s auth
    /// 3. Иначе → OIDC (локальная разработка)
    pub async fn from_env() -> Result<Self, VaultError>;

    /// Builder для кастомной конфигурации
    pub fn builder() -> VaultClientBuilder;

    /// Получить KV v2 секрет
    pub async fn kv_read(&self, mount: &str, path: &str) -> Result<KvData, VaultError>;

    /// Получить метаданные секрета (версии, timestamps)
    pub async fn kv_metadata(&self, mount: &str, path: &str) -> Result<KvMetadata, VaultError>;

    /// Низкоуровневый HTTP запрос к Vault API
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T, VaultError>;
}
```

### VaultClientBuilder

```rust
pub struct VaultClientBuilder { /* ... */ }

impl VaultClientBuilder {
    pub fn base_url(self, url: impl Into<String>) -> Self;
    pub fn token(self, token: impl Into<String>) -> Self;
    pub fn k8s_auth_method(self, method: impl Into<String>) -> Self;
    pub fn oidc_auth_method(self, method: impl Into<String>) -> Self;
    pub fn role(self, role: impl Into<String>) -> Self;
    pub fn application_name(self, name: impl Into<String>) -> Self;

    /// Минимальная длительность токена для renewal (default: 300 сек)
    pub fn renewable_token_min_duration(self, duration: Duration) -> Self;

    /// Интервал повтора при ошибках renewal (default: 10 сек)
    pub fn retry_interval(self, duration: Duration) -> Self;

    pub async fn build(self) -> Result<VaultClient, VaultError>;
}
```

### Data Models

```rust
#[derive(Debug, Clone)]
pub struct KvData {
    pub data: HashMap<String, serde_json::Value>,
    pub metadata: KvVersion,
}

#[derive(Debug, Clone)]
pub struct KvVersion {
    pub version: u64,
    pub created_time: DateTime<Utc>,
    pub deletion_time: Option<DateTime<Utc>>,
    pub destroyed: bool,
}

#[derive(Debug, Clone)]
pub struct KvMetadata {
    pub created_time: DateTime<Utc>,
    pub custom_metadata: Option<HashMap<String, String>>,
    pub versions: Vec<KvVersion>,
}
```

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("Vault not detected: VAULT_ADDR not set")]
    VaultNotDetected,

    #[error("Secret not found: {path}")]
    SecretNotFound { path: String },

    #[error("Vault client error ({status}): {message}")]
    ClientError {
        status: u16,
        message: String,
        response_data: Option<serde_json::Value>,
    },

    #[error("Vault request error: {0}")]
    RequestError(String),

    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("OIDC authentication failed: {0}")]
    OidcError(String),

    #[error("Kubernetes auth failed: {0}")]
    KubernetesError(String),
}
```

## Environment Variables

Читаются автоматически в `from_env()`:

| Variable | Default | Description |
|----------|---------|-------------|
| `VAULT_ADDR` | — | Адрес Vault сервера (обязателен) |
| `VAULT_TOKEN` | — | Статический токен (приоритет над auth methods) |
| `VAULT_AUTH_METHOD` | `"kubernetes"` | Название K8s auth method |
| `VAULT_ROLE_ID` | `"app"` | Роль для K8s/OIDC |
| `KUBERNETES_SERVICE_HOST` | — | Детект K8s окружения |
| `K8S_JWT_TOKEN_PATH` | `/var/run/secrets/kubernetes.io/serviceaccount/token` | Путь к service account token |

## Authentication

### Priority

1. **Static Token** — если есть `VAULT_TOKEN`
2. **Kubernetes Auth** — если есть `KUBERNETES_SERVICE_HOST`
3. **OIDC** — fallback для локальной разработки

### Token Management

- Background tokio task для auto-renewal
- Renewal при достижении 75% от lease_duration
- При 4xx ошибке renewal — re-authenticate
- При 5xx ошибке — retry через `retry_interval` (default: 10 сек)
- Минимальная длительность для renewal: 300 сек

### OIDC Flow

1. Запрос auth URL у Vault (`/v1/auth/{method}/oidc/auth_url`)
2. Открытие браузера через `webbrowser::open()`
3. Локальный HTTP сервер на `localhost:8250` для callback
4. Получение токена через callback endpoint

### OIDC Disk Cache

- Путь: `~/.cache/vault-client/` (через `directories` crate)
- Ключ кеша: SHA256 от `vault_addr + auth_method + role`
- Переиспользуем если токен валиден ещё минимум 1 час

## Integration with runtime-settings

### Changes

1. Убираем прямую зависимость от `vaultrs`
2. `VaultClient` передаётся в `RuntimeSettingsBuilder`

```rust
let vault = VaultClient::from_env().await?;

let settings = RuntimeSettings::builder()
    .application("my-service")
    .vault_client(vault)
    .build()
    .await?;
```

### RuntimeSettingsBuilder

```rust
impl RuntimeSettingsBuilder {
    pub fn vault_client(self, client: VaultClient) -> Self;
}
```

### SecretsService

```rust
pub struct SecretsService {
    client: Option<VaultClient>,  // передан извне
    cache: RwLock<HashMap<String, CachedSecret>>,
    version: AtomicU64,
}

impl SecretsService {
    pub fn new(client: VaultClient) -> Self;
    pub fn without_vault() -> Self;
}
```

Кеширование секретов и `resolve_secrets()` остаются в runtime-settings.

## Dependencies

```toml
[dependencies]
vaultrs = "0.7"
tokio = { version = "1", features = ["sync", "time", "rt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
chrono = { version = "0.4", features = ["serde"] }
directories = "5"
sha2 = "0.10"
webbrowser = "1"
tracing = "0.1"
```
