# Context Refactoring Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Разделить контексты на три независимых хранилища: StaticContext, Request, CustomContext — для устранения дублирования и потери данных.

**Architecture:** Вместо единого `Context` struct используем три независимых источника: `StaticContext` (в RuntimeSettings), `Request` (thread/task-local), `CustomContext` с ChainMap-семантикой (thread/task-local). Для фильтров создаём `DynamicContext` который собирает request + custom.

**Tech Stack:** Rust, tokio task_local, std thread_local

---

## Task 1: Добавить CustomContext в context.rs

**Files:**
- Modify: `lib/runtime-settings/src/context.rs`

**Step 1: Write failing test for CustomContext**

Добавить в конец файла в mod tests:

```rust
#[test]
fn test_custom_context_single_layer() {
    let mut ctx = CustomContext::new();
    ctx.push_layer([("key1".to_string(), "value1".to_string())].into());
    assert_eq!(ctx.get("key1"), Some("value1"));
    assert_eq!(ctx.get("missing"), None);
}

#[test]
fn test_custom_context_layered_override() {
    let mut ctx = CustomContext::new();
    ctx.push_layer([("key1".to_string(), "base".to_string())].into());
    ctx.push_layer([("key1".to_string(), "override".to_string())].into());
    assert_eq!(ctx.get("key1"), Some("override"));
    ctx.pop_layer();
    assert_eq!(ctx.get("key1"), Some("base"));
}

#[test]
fn test_custom_context_iter() {
    let mut ctx = CustomContext::new();
    ctx.push_layer([("a".to_string(), "1".to_string())].into());
    ctx.push_layer([("b".to_string(), "2".to_string()), ("a".to_string(), "override".to_string())].into());
    let items: HashMap<&str, &str> = ctx.iter().collect();
    assert_eq!(items.get("a"), Some(&"override"));
    assert_eq!(items.get("b"), Some(&"2"));
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p runtime-settings test_custom_context
```
Expected: FAIL - `CustomContext` not found

**Step 3: Implement CustomContext**

Добавить после `impl Request` (после строки 37):

```rust
/// Hierarchical custom context with ChainMap-like semantics
#[derive(Debug, Clone, Default)]
pub struct CustomContext {
    layers: Vec<HashMap<String, String>>,
}

impl CustomContext {
    /// Create empty custom context
    pub fn new() -> Self {
        Self { layers: vec![] }
    }

    /// Add a new layer on top
    pub fn push_layer(&mut self, layer: HashMap<String, String>) {
        self.layers.push(layer);
    }

    /// Remove the top layer
    pub fn pop_layer(&mut self) {
        self.layers.pop();
    }

    /// Get value by key (searches from top layer to bottom)
    pub fn get(&self, key: &str) -> Option<&str> {
        for layer in self.layers.iter().rev() {
            if let Some(v) = layer.get(key) {
                return Some(v.as_str());
            }
        }
        None
    }

    /// Iterate over all unique key-value pairs (top layer wins)
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for layer in self.layers.iter().rev() {
            for (k, v) in layer {
                if seen.insert(k.as_str()) {
                    result.push((k.as_str(), v.as_str()));
                }
            }
        }
        result.into_iter()
    }

    /// Check if context is empty
    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|l| l.is_empty())
    }
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p runtime-settings test_custom_context
```
Expected: PASS

**Step 5: Commit**

```bash
git add lib/runtime-settings/src/context.rs
git commit -m "Add CustomContext with ChainMap semantics"
```

---

## Task 2: Добавить DynamicContext в context.rs

**Files:**
- Modify: `lib/runtime-settings/src/context.rs`

**Step 1: Write failing test for DynamicContext**

```rust
#[test]
fn test_dynamic_context_default() {
    let ctx = DynamicContext::default();
    assert!(ctx.request.is_none());
    assert!(ctx.custom.is_empty());
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p runtime-settings test_dynamic_context
```
Expected: FAIL - `DynamicContext` not found

**Step 3: Implement DynamicContext**

Добавить после `impl CustomContext`:

```rust
/// Context for dynamic filter evaluation
#[derive(Debug, Clone, Default)]
pub struct DynamicContext {
    pub request: Option<Request>,
    pub custom: CustomContext,
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p runtime-settings test_dynamic_context
```
Expected: PASS

**Step 5: Commit**

```bash
git add lib/runtime-settings/src/context.rs
git commit -m "Add DynamicContext struct"
```

---

## Task 3: Обновить scoped.rs — добавить CustomContext хранилище

**Files:**
- Modify: `lib/runtime-settings/src/scoped.rs`

**Step 1: Write failing test for custom context storage**

Добавить в mod tests:

```rust
#[test]
fn test_thread_local_custom() {
    let layer: HashMap<String, String> = [("key".to_string(), "value".to_string())].into();
    {
        let _guard = set_thread_custom(layer);
        let current = current_custom();
        assert_eq!(current.get("key"), Some("value"));
    }
    let current = current_custom();
    assert!(current.is_empty());
}

#[test]
fn test_nested_custom_guards() {
    let layer1: HashMap<String, String> = [("key".to_string(), "base".to_string())].into();
    let layer2: HashMap<String, String> = [("key".to_string(), "override".to_string())].into();
    {
        let _guard1 = set_thread_custom(layer1);
        assert_eq!(current_custom().get("key"), Some("base"));
        {
            let _guard2 = set_thread_custom(layer2);
            assert_eq!(current_custom().get("key"), Some("override"));
        }
        assert_eq!(current_custom().get("key"), Some("base"));
    }
    assert!(current_custom().is_empty());
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p runtime-settings test_thread_local_custom
```
Expected: FAIL

**Step 3: Implement custom context storage**

Обновить imports в начале файла:

```rust
use crate::context::{Context, CustomContext, Request};
```

Добавить после `THREAD_REQUEST`:

```rust
thread_local! {
    static THREAD_CUSTOM: RefCell<CustomContext> = RefCell::new(CustomContext::new());
}
```

Добавить после `current_request()`:

```rust
/// Get current custom context (returns clone)
pub fn current_custom() -> CustomContext {
    THREAD_CUSTOM.with(|c| c.borrow().clone())
}
```

Добавить `CustomContextGuard`:

```rust
/// Guard that pops layer from custom context on drop
#[must_use = "guard must be held for the custom context layer to remain active"]
pub struct CustomContextGuard;

impl Drop for CustomContextGuard {
    fn drop(&mut self) {
        THREAD_CUSTOM.with(|c| c.borrow_mut().pop_layer());
    }
}
```

Добавить `set_thread_custom`:

```rust
/// Add layer to thread-local custom context, returns guard that pops on drop
pub fn set_thread_custom(layer: HashMap<String, String>) -> CustomContextGuard {
    THREAD_CUSTOM.with(|c| c.borrow_mut().push_layer(layer));
    CustomContextGuard
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p runtime-settings test_thread_local_custom test_nested_custom
```
Expected: PASS

**Step 5: Commit**

```bash
git add lib/runtime-settings/src/scoped.rs
git commit -m "Add CustomContext storage in scoped.rs"
```

---

## Task 4: Добавить task-local для CustomContext

**Files:**
- Modify: `lib/runtime-settings/src/scoped.rs`

**Step 1: Write failing test**

```rust
#[tokio::test]
async fn test_task_local_custom() {
    let layer: HashMap<String, String> = [("async_key".to_string(), "async_value".to_string())].into();
    let result = with_task_custom(layer, async {
        current_custom().get("async_key").map(|s| s.to_string())
    }).await;
    assert_eq!(result, Some("async_value".to_string()));
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p runtime-settings test_task_local_custom
```
Expected: FAIL

**Step 3: Implement task-local custom**

Обновить tokio::task_local! блок:

```rust
tokio::task_local! {
    static TASK_CONTEXT: Option<Context>;
    static TASK_REQUEST: Option<Request>;
    static TASK_CUSTOM: CustomContext;
}
```

Обновить `current_custom()` для проверки task-local:

```rust
/// Get current custom context (task-local takes priority over thread-local)
pub fn current_custom() -> CustomContext {
    TASK_CUSTOM
        .try_with(|c| c.clone())
        .ok()
        .unwrap_or_else(|| THREAD_CUSTOM.with(|c| c.borrow().clone()))
}
```

Добавить `with_task_custom`:

```rust
/// Execute async closure with additional custom context layer
pub async fn with_task_custom<F, T>(layer: HashMap<String, String>, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let mut ctx = current_custom();
    ctx.push_layer(layer);
    TASK_CUSTOM.scope(ctx, f).await
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p runtime-settings test_task_local_custom
```
Expected: PASS

**Step 5: Commit**

```bash
git add lib/runtime-settings/src/scoped.rs
git commit -m "Add task-local CustomContext support"
```

---

## Task 5: Обновить trait CompiledDynamicFilter

**Files:**
- Modify: `lib/runtime-settings/src/filters/mod.rs`

**Step 1: Update imports and trait**

Изменить import:

```rust
use crate::context::{DynamicContext, StaticContext};
```

Изменить trait `DynamicFilter`:

```rust
/// Dynamic filter - checked on every get()
pub trait DynamicFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult;
}
```

Изменить trait `CompiledDynamicFilter`:

```rust
/// Trait for pre-compiled dynamic filters
pub trait CompiledDynamicFilter: Send + Sync {
    fn check(&self, ctx: &DynamicContext) -> bool;
}
```

**Step 2: Run cargo check**

```bash
cargo check -p runtime-settings 2>&1 | head -50
```
Expected: Errors in dynamic_filters.rs (will fix in next task)

**Step 3: Commit partial change**

```bash
git add lib/runtime-settings/src/filters/mod.rs
git commit -m "Update filter traits to use DynamicContext"
```

---

## Task 6: Обновить dynamic_filters.rs — часть 1 (request-based фильтры)

**Files:**
- Modify: `lib/runtime-settings/src/filters/dynamic_filters.rs`

**Step 1: Update imports**

```rust
use crate::context::DynamicContext;
```

**Step 2: Update all filter implementations**

Заменить `ctx: &Context` на `ctx: &DynamicContext` во всех impl блоках.

Для request-based фильтров (UrlPathFilter, HostFilter, EmailFilter, IpFilter, HeaderFilter) изменения минимальны — просто тип параметра.

Для ContextFilter нужно изменить:

```rust
impl DynamicFilter for ContextFilter {
    fn name(&self) -> &'static str {
        "context"
    }

    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult {
        // Convert CustomContext to HashMap for check_map_filter
        let map: HashMap<String, String> = ctx.custom.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        check_map_filter(pattern, &map)
    }
}
```

Для CompiledContextFilter:

```rust
impl CompiledDynamicFilter for CompiledContextFilter {
    fn check(&self, ctx: &DynamicContext) -> bool {
        for (key, regex) in &self.conditions {
            match ctx.custom.get(key) {
                Some(actual_value) => {
                    if !regex.is_match(actual_value) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}
```

**Step 3: Update test helper**

Заменить `make_ctx_with_request` на:

```rust
fn make_ctx_with_request(path: &str, headers: HashMap<String, String>) -> DynamicContext {
    DynamicContext {
        request: Some(Request {
            method: "GET".to_string(),
            path: path.to_string(),
            headers,
        }),
        custom: CustomContext::new(),
    }
}
```

Добавить import в тестах:
```rust
use crate::context::{CustomContext, DynamicContext, Request};
```

**Step 4: Update all Context::default() in tests**

Заменить `Context::default()` на `DynamicContext::default()`.

Для тестов с custom context:
```rust
// Было:
let mut ctx = Context::default();
ctx.custom.insert("user_id".to_string(), "123".to_string());

// Стало:
let mut custom = CustomContext::new();
custom.push_layer([("user_id".to_string(), "123".to_string())].into());
let ctx = DynamicContext {
    request: None,
    custom,
};
```

**Step 5: Run tests**

```bash
cargo test -p runtime-settings dynamic_filters
```
Expected: PASS

**Step 6: Commit**

```bash
git add lib/runtime-settings/src/filters/dynamic_filters.rs
git commit -m "Update dynamic_filters to use DynamicContext"
```

---

## Task 7: Обновить entities.rs

**Files:**
- Modify: `lib/runtime-settings/src/entities.rs`

**Step 1: Update import**

```rust
use crate::context::{DynamicContext, StaticContext};
```

**Step 2: Update check_dynamic_filters**

```rust
pub fn check_dynamic_filters(&self, ctx: &DynamicContext) -> bool {
```

**Step 3: Update tests**

Заменить `Context` на `DynamicContext` в тестах. Для тестов с custom:

```rust
// Было:
let mut ctx_match = Context::default();
ctx_match.custom.insert("feature".to_string(), "enabled".to_string());

// Стало:
let mut custom = CustomContext::new();
custom.push_layer([("feature".to_string(), "enabled".to_string())].into());
let ctx_match = DynamicContext {
    request: None,
    custom,
};
```

**Step 4: Run tests**

```bash
cargo test -p runtime-settings entities
```
Expected: PASS

**Step 5: Commit**

```bash
git add lib/runtime-settings/src/entities.rs
git commit -m "Update entities to use DynamicContext"
```

---

## Task 8: Обновить settings.rs — get_dynamic_context

**Files:**
- Modify: `lib/runtime-settings/src/settings.rs`

**Step 1: Update imports**

```rust
use crate::context::{DynamicContext, Request, StaticContext};
use crate::scoped::{
    current_custom, current_request, set_thread_custom, set_thread_request,
    with_task_custom, with_task_request, CustomContextGuard, RequestGuard,
};
```

**Step 2: Replace get_effective_context with get_dynamic_context**

```rust
/// Get dynamic context from scoped request and custom
fn get_dynamic_context(&self) -> DynamicContext {
    DynamicContext {
        request: current_request(),
        custom: current_custom(),
    }
}
```

**Step 3: Update get() method**

```rust
pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
    let ctx = self.get_dynamic_context();
    self.get_internal(key, &ctx)
}
```

**Step 4: Update get_internal signature**

```rust
fn get_internal<T: DeserializeOwned>(&self, key: &str, ctx: &DynamicContext) -> Option<T> {
```

**Step 5: Remove set_context and with_context, add set_custom and with_custom**

Удалить:
- `set_context`
- `with_context`

Добавить:
```rust
/// Set thread-local custom context layer
pub fn set_custom(&self, values: HashMap<String, String>) -> CustomContextGuard {
    set_thread_custom(values)
}

/// Execute async closure with additional custom context layer
pub async fn with_custom<F, T>(&self, values: HashMap<String, String>, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    with_task_custom(values, f).await
}
```

**Step 6: Update collect_current_values**

```rust
fn collect_current_values(&self) -> HashMap<String, serde_json::Value> {
    let state = self.state.read().unwrap();
    let mut values = HashMap::new();

    let ctx = self.get_dynamic_context();

    for (key, settings) in &state.settings {
        for setting in settings {
            if setting.check_dynamic_filters(&ctx) {
                values.insert(key.clone(), setting.value.clone());
                break;
            }
        }
    }

    values
}
```

**Step 7: Update tests**

Заменить все `Context` на `DynamicContext` или использование `set_request`/`set_custom`.

**Step 8: Run tests**

```bash
cargo test -p runtime-settings settings
```
Expected: PASS

**Step 9: Commit**

```bash
git add lib/runtime-settings/src/settings.rs
git commit -m "Replace get_effective_context with get_dynamic_context"
```

---

## Task 9: Удалить старый Context struct

**Files:**
- Modify: `lib/runtime-settings/src/context.rs`
- Modify: `lib/runtime-settings/src/scoped.rs`

**Step 1: Remove Context from context.rs**

Удалить struct Context и impl From<&Context> for StaticContext.

**Step 2: Remove Context-related code from scoped.rs**

Удалить:
- `TASK_CONTEXT`
- `THREAD_CONTEXT`
- `current_context()`
- `ContextGuard`
- `set_thread_context()`
- `with_task_context()`
- Тесты связанные с context

**Step 3: Run all tests**

```bash
cargo test -p runtime-settings
```
Expected: PASS (после удаления устаревших тестов)

**Step 4: Commit**

```bash
git add lib/runtime-settings/src/context.rs lib/runtime-settings/src/scoped.rs
git commit -m "Remove deprecated Context struct"
```

---

## Task 10: Обновить lib.rs exports

**Files:**
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Update exports**

```rust
pub use context::{CustomContext, DynamicContext, Request, StaticContext};
pub use scoped::{
    current_custom, current_request, set_thread_custom, set_thread_request,
    with_task_custom, with_task_request, CustomContextGuard, RequestGuard,
};
```

**Step 2: Run cargo build**

```bash
cargo build -p runtime-settings
```
Expected: SUCCESS

**Step 3: Commit**

```bash
git add lib/runtime-settings/src/lib.rs
git commit -m "Update lib.rs exports for new context API"
```

---

## Task 11: Обновить integration тесты

**Files:**
- Modify: `lib/runtime-settings/tests/integration_filters.rs`

**Step 1: Update tests to use DynamicContext**

Заменить все использования Context на DynamicContext.

**Step 2: Run integration tests**

```bash
cargo test -p runtime-settings --test integration_filters
```
Expected: PASS

**Step 3: Commit**

```bash
git add lib/runtime-settings/tests/
git commit -m "Update integration tests for new context API"
```

---

## Task 12: Обновить example

**Files:**
- Modify: `example/src/main.rs` (если использует Context)

**Step 1: Search for Context usage**

```bash
grep -r "Context" example/
```

**Step 2: Update if needed**

Заменить на использование set_request/set_custom.

**Step 3: Run example build**

```bash
cargo build -p example
```
Expected: SUCCESS

**Step 4: Commit**

```bash
git add example/
git commit -m "Update example for new context API"
```

---

## Task 13: Финальная проверка

**Step 1: Run all tests**

```bash
cargo test
```
Expected: All tests pass

**Step 2: Run clippy**

```bash
cargo clippy
```
Expected: No warnings

**Step 3: Run fmt**

```bash
cargo fmt
```

**Step 4: Final commit if needed**

```bash
git add -A
git commit -m "Fix clippy warnings and formatting"
```
