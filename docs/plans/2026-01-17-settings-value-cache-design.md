# Settings Value Cache Design

## Problem

При каждом вызове `get<T>()` происходит десериализация `serde_json::Value` в нужный тип через `serde_json::from_value()`. При миллионах вызовов с одним и тем же типом это создаёт лишнюю нагрузку.

## Solution

Добавить кеш десериализованных значений по `TypeId` внутрь структуры `Setting`.

## Design

### Изменения в структуре Setting

```rust
use dashmap::DashMap;
use std::any::{Any, TypeId};
use std::sync::Arc;

pub struct Setting {
    pub key: String,
    pub priority: i64,
    pub value: serde_json::Value,
    pub static_filters: Vec<Box<dyn CompiledStaticFilter>>,
    pub dynamic_filters: Vec<Box<dyn CompiledDynamicFilter>>,

    // Новое поле: кеш десериализованных значений
    value_cache: DashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}
```

### Метод получения значения с кешированием

```rust
impl Setting {
    /// Получить значение с кешированием по TypeId.
    ///
    /// При первом обращении десериализует JSON и кладёт в кеш.
    /// Последующие вызовы с тем же типом возвращают закешированное значение.
    ///
    /// Note: возможен race condition при одновременном cache miss из нескольких потоков —
    /// оба десериализуют и один перезапишет другого. Это безопасно (значения идентичны),
    /// просто небольшая лишняя работа при первых вызовах.
    pub fn get_value<T>(&self) -> Option<Arc<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();

        // Проверяем кеш
        if let Some(cached) = self.value_cache.get(&type_id) {
            let arc_any: Arc<dyn Any + Send + Sync> = Arc::clone(cached.value());
            return Arc::downcast::<T>(arc_any).ok();
        }

        // Cache miss — десериализуем
        let value: T = serde_json::from_value(self.value.clone()).ok()?;
        let arc_value = Arc::new(value);

        // Кладём в кеш
        self.value_cache.insert(
            type_id,
            Arc::clone(&arc_value) as Arc<dyn Any + Send + Sync>,
        );

        Some(arc_value)
    }
}
```

### Изменения в публичном API RuntimeSettings

```rust
impl RuntimeSettings {
    /// Получить значение настройки по ключу.
    pub fn get<T>(&self, key: &str) -> Option<Arc<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let ctx = self.get_effective_context()
            .expect("Context not set - call set_context() or with_context() first");
        self.get_internal(key, &ctx)
    }

    /// Получить значение или default.
    pub fn get_or<T>(&self, key: &str, default: T) -> Arc<T>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        self.get(key).unwrap_or_else(|| Arc::new(default))
    }

    fn get_internal<T>(&self, key: &str, ctx: &Context) -> Option<Arc<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let state = self.state.read().unwrap();
        let settings = state.settings.get(key)?;

        for setting in settings {
            if setting.check_dynamic_filters(ctx) {
                return setting.get_value::<T>();
            }
        }
        None
    }
}
```

## Dependencies

Новая зависимость в `Cargo.toml`:

```toml
[dependencies]
dashmap = "6"
```

## Changes Summary

1. **Setting** — добавить поле `value_cache: DashMap<TypeId, Arc<dyn Any + Send + Sync>>`
2. **Setting::compile()** — инициализировать пустой `DashMap::new()`
3. **Setting::get_value<T>()** — новый метод с кешированием
4. **RuntimeSettings::get<T>()** — возвращает `Option<Arc<T>>` вместо `Option<T>`
5. **RuntimeSettings::get_or<T>()** — возвращает `Arc<T>` вместо `T`
6. **RuntimeSettings::get_internal<T>()** — использует `setting.get_value::<T>()`
7. **Тесты и примеры** — обновить под новый API

## Cache Invalidation

При `refresh()` создаются новые `Setting` объекты — кеш автоматически чистый для новых настроек. Ленивое заполнение при первых обращениях.
