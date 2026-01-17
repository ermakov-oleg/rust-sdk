// lib/runtime-settings/src/scoped.rs
//! Thread-local and task-local context storage for runtime settings.
//!
//! This module provides scoped context management, allowing context to be
//! automatically available to code within a scope without explicit passing.

use crate::context::{Context, CustomContext, Request};
use std::cell::RefCell;
use std::collections::HashMap;

tokio::task_local! {
    static TASK_CONTEXT: Option<Context>;
    static TASK_REQUEST: Option<Request>;
    static TASK_CUSTOM: CustomContext;
}

thread_local! {
    static THREAD_CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
    static THREAD_REQUEST: RefCell<Option<Request>> = const { RefCell::new(None) };
}

thread_local! {
    static THREAD_CUSTOM: RefCell<CustomContext> = RefCell::new(CustomContext::new());
}

/// Get current context (task-local takes priority over thread-local)
pub fn current_context() -> Option<Context> {
    TASK_CONTEXT
        .try_with(|c| c.clone())
        .ok()
        .flatten()
        .or_else(|| THREAD_CONTEXT.with(|c| c.borrow().clone()))
}

/// Get current request (task-local takes priority over thread-local)
pub fn current_request() -> Option<Request> {
    TASK_REQUEST
        .try_with(|r| r.clone())
        .ok()
        .flatten()
        .or_else(|| THREAD_REQUEST.with(|r| r.borrow().clone()))
}

/// Get current custom context (task-local takes priority over thread-local)
pub fn current_custom() -> CustomContext {
    TASK_CUSTOM
        .try_with(|c| c.clone())
        .ok()
        .unwrap_or_else(|| THREAD_CUSTOM.with(|c| c.borrow().clone()))
}

/// Guard that restores previous context on drop
#[must_use = "guard must be held for the context to remain active"]
pub struct ContextGuard {
    previous: Option<Context>,
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        THREAD_CONTEXT.with(|c| {
            *c.borrow_mut() = self.previous.take();
        });
    }
}

/// Guard that restores previous request on drop
#[must_use = "guard must be held for the request to remain active"]
pub struct RequestGuard {
    previous: Option<Request>,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        THREAD_REQUEST.with(|r| {
            *r.borrow_mut() = self.previous.take();
        });
    }
}

/// Guard that pops layer from custom context on drop
#[must_use = "guard must be held for the custom context layer to remain active"]
pub struct CustomContextGuard;

impl Drop for CustomContextGuard {
    fn drop(&mut self) {
        THREAD_CUSTOM.with(|c| c.borrow_mut().pop_layer());
    }
}

/// Set thread-local context, returns guard that restores previous on drop
pub fn set_thread_context(ctx: Context) -> ContextGuard {
    let previous = THREAD_CONTEXT.with(|c| c.borrow_mut().replace(ctx));
    ContextGuard { previous }
}

/// Set thread-local request, returns guard that restores previous on drop
pub fn set_thread_request(req: Request) -> RequestGuard {
    let previous = THREAD_REQUEST.with(|r| r.borrow_mut().replace(req));
    RequestGuard { previous }
}

/// Add layer to thread-local custom context, returns guard that pops on drop
pub fn set_thread_custom(layer: HashMap<String, String>) -> CustomContextGuard {
    THREAD_CUSTOM.with(|c| c.borrow_mut().push_layer(layer));
    CustomContextGuard
}

/// Execute async closure with task-local context
pub async fn with_task_context<F, T>(ctx: Context, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TASK_CONTEXT.scope(Some(ctx), f).await
}

/// Execute async closure with task-local request
pub async fn with_task_request<F, T>(req: Request, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TASK_REQUEST.scope(Some(req), f).await
}

/// Execute async closure with additional custom context layer
pub async fn with_task_custom<F, T>(layer: HashMap<String, String>, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let mut ctx = current_custom();
    ctx.push_layer(layer);
    TASK_CUSTOM.scope(ctx, f).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_local_context() {
        let ctx = Context {
            application: "test-app".to_string(),
            ..Default::default()
        };

        {
            let _guard = set_thread_context(ctx.clone());
            let current = current_context().unwrap();
            assert_eq!(current.application, "test-app");
        }

        assert!(current_context().is_none());
    }

    #[test]
    fn test_thread_local_request() {
        let req = Request {
            method: "POST".to_string(),
            path: "/api".to_string(),
            headers: std::collections::HashMap::new(),
        };

        {
            let _guard = set_thread_request(req.clone());
            let current = current_request().unwrap();
            assert_eq!(current.method, "POST");
        }

        assert!(current_request().is_none());
    }

    #[test]
    fn test_nested_context_guards() {
        let ctx1 = Context {
            application: "app1".to_string(),
            ..Default::default()
        };
        let ctx2 = Context {
            application: "app2".to_string(),
            ..Default::default()
        };

        {
            let _guard1 = set_thread_context(ctx1);
            assert_eq!(current_context().unwrap().application, "app1");

            {
                let _guard2 = set_thread_context(ctx2);
                assert_eq!(current_context().unwrap().application, "app2");
            }

            assert_eq!(current_context().unwrap().application, "app1");
        }

        assert!(current_context().is_none());
    }

    #[tokio::test]
    async fn test_task_local_context() {
        let ctx = Context {
            application: "async-app".to_string(),
            ..Default::default()
        };

        let result = with_task_context(ctx, async {
            current_context().unwrap().application.clone()
        })
        .await;

        assert_eq!(result, "async-app");
    }

    #[tokio::test]
    async fn test_task_local_priority_over_thread_local() {
        let thread_ctx = Context {
            application: "thread-app".to_string(),
            ..Default::default()
        };
        let task_ctx = Context {
            application: "task-app".to_string(),
            ..Default::default()
        };

        let _guard = set_thread_context(thread_ctx);

        let result = with_task_context(task_ctx, async {
            current_context().unwrap().application.clone()
        })
        .await;

        assert_eq!(result, "task-app");
        assert_eq!(current_context().unwrap().application, "thread-app");
    }

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

    #[tokio::test]
    async fn test_task_local_custom() {
        let layer: HashMap<String, String> = [("async_key".to_string(), "async_value".to_string())].into();
        let result = with_task_custom(layer, async {
            current_custom().get("async_key").map(|s| s.to_string())
        }).await;
        assert_eq!(result, Some("async_value".to_string()));
    }
}
