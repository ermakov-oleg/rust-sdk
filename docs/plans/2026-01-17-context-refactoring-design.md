# Рефакторинг контекстов для динамических и статических фильтров

## Проблема

Текущая реализация `get_effective_context` имеет проблемы:

1. **Потеря `custom` контекста** — когда есть только `scoped_request`, создаётся `Context` с `custom: HashMap::new()`
2. **Нет настоящего merge** — в Python используется `ChainMap.new_child()` для иерархии, в Rust контекст заменяется целиком
3. **Избыточное клонирование** — каждый вызов клонирует все поля `static_context`
4. **Request и Custom связаны** — нет способа установить только `custom` без полного `Context`

## Решение

Три независимых хранилища вместо единого `Context`:

- `StaticContext` — неизменяемый, хранится в `RuntimeSettings`
- `Request` — HTTP запрос, устанавливается независимо
- `CustomContext` — иерархический контекст с ChainMap-семантикой

## Новые типы

### CustomContext (иерархический контекст)

```rust
pub struct CustomContext {
    layers: Vec<HashMap<String, String>>,  // от внешнего к внутреннему
}

impl CustomContext {
    pub fn new() -> Self { Self { layers: vec![] } }

    pub fn push_layer(&mut self, layer: HashMap<String, String>) { ... }
    pub fn pop_layer(&mut self) { ... }

    // Поиск по всем слоям (внутренний приоритетнее)
    pub fn get(&self, key: &str) -> Option<&str> {
        for layer in self.layers.iter().rev() {
            if let Some(v) = layer.get(key) { return Some(v); }
        }
        None
    }

    // Итератор по всем уникальным ключам (для фильтров)
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> { ... }
}
```

### DynamicContext (для фильтров)

```rust
pub struct DynamicContext {
    pub request: Option<Request>,
    pub custom: CustomContext,
}
```

## Хранилища (scoped.rs)

```rust
thread_local! {
    static THREAD_REQUEST: RefCell<Option<Request>> = ...;
    static THREAD_CUSTOM: RefCell<CustomContext> = RefCell::new(CustomContext::new());
}

tokio::task_local! {
    static TASK_REQUEST: Option<Request>;
    static TASK_CUSTOM: CustomContext;
}
```

## API для установки контекста

### Guards

```rust
// Guard для request (как сейчас)
pub struct RequestGuard {
    previous: Option<Request>,
}

// Guard для custom context — добавляет/убирает слой
pub struct CustomContextGuard {
    // При drop вызывает pop_layer()
}

impl Drop for CustomContextGuard {
    fn drop(&mut self) {
        THREAD_CUSTOM.with(|c| c.borrow_mut().pop_layer());
    }
}
```

### Функции установки

```rust
// Request — как сейчас, заменяет целиком
pub fn set_thread_request(req: Request) -> RequestGuard { ... }

// Custom — добавляет слой поверх существующих (ChainMap семантика)
pub fn set_thread_custom(values: HashMap<String, String>) -> CustomContextGuard {
    THREAD_CUSTOM.with(|c| c.borrow_mut().push_layer(values));
    CustomContextGuard {}
}

// Async версии
pub async fn with_task_request<F, T>(req: Request, f: F) -> T { ... }
pub async fn with_task_custom<F, T>(values: HashMap<String, String>, f: F) -> T { ... }
```

### Пример использования (вложенность)

```rust
let _guard1 = set_thread_custom(hashmap!{"user_id" => "123"});
// custom: {"user_id": "123"}

let _guard2 = set_thread_custom(hashmap!{"feature" => "beta"});
// custom: {"user_id": "123", "feature": "beta"}

drop(_guard2);
// custom: {"user_id": "123"}  — слой "feature" удалён
```

## Изменения в фильтрах

### RuntimeSettings

```rust
impl RuntimeSettings {
    fn get_dynamic_context(&self) -> DynamicContext {
        DynamicContext {
            request: current_request(),
            custom: current_custom(),
        }
    }
}
```

### Trait динамических фильтров

```rust
pub trait CompiledDynamicFilter: Send + Sync {
    fn check(&self, ctx: &DynamicContext) -> bool;
}
```

### Пример реализации фильтра

```rust
impl CompiledDynamicFilter for ContextFilter {
    fn check(&self, ctx: &DynamicContext) -> bool {
        ctx.custom.get(&self.key)
            .map(|v| self.pattern.is_match(v))
            .unwrap_or(false)
    }
}
```

## Миграция

### Удаляем

- `Context` struct (заменяется на `DynamicContext`)
- `THREAD_CONTEXT` / `TASK_CONTEXT` хранилища
- `set_thread_context()` / `with_task_context()` функции
- `ContextGuard` (заменяется на `CustomContextGuard`)

### Остаётся без изменений

- `StaticContext` — используется при загрузке настроек
- `Request` struct и его методы (`host()`, `ip()`, `email()`)
- `set_thread_request()` / `with_task_request()` — API не меняется

### Добавляем

- `CustomContext` struct с иерархией слоёв
- `DynamicContext` struct для фильтров
- `set_thread_custom()` / `with_task_custom()` функции
- `CustomContextGuard`

### Пример миграции для пользователей

```rust
// Было:
let ctx = Context {
    request: Some(req),
    custom: hashmap!{"user_id" => "123"},
    ..Default::default()
};
let _guard = set_thread_context(ctx);

// Стало:
let _req_guard = set_thread_request(req);
let _ctx_guard = set_thread_custom(hashmap!{"user_id" => "123"});
```

## Структура файлов

```
lib/runtime-settings/src/
├── context.rs          # Request, StaticContext, CustomContext, DynamicContext
├── scoped.rs           # thread/task-local хранилища и guards
├── settings.rs         # RuntimeSettings с get_dynamic_context()
└── filters/
    ├── mod.rs          # trait CompiledDynamicFilter с DynamicContext
    ├── static_filters.rs   # без изменений
    └── dynamic_filters.rs  # фильтры используют DynamicContext
```

## Публичный API

```rust
// Типы
pub use context::{Request, StaticContext, CustomContext, DynamicContext};

// Установка контекста
pub use scoped::{
    set_thread_request, set_thread_custom,
    with_task_request, with_task_custom,
    current_request, current_custom,
    RequestGuard, CustomContextGuard,
};
```

## Middleware пример (axum)

```rust
async fn context_middleware(req: AxumRequest, next: Next) -> Response {
    let request = Request::from(&req);
    with_task_request(request, next.run(req)).await
}

// Отдельно в handler-е если нужен custom:
async fn handler() {
    let _guard = set_thread_custom(hashmap!{"feature" => "beta"});
    // ...
}
```
