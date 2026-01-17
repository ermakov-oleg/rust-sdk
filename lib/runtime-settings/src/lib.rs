// lib/runtime-settings/src/lib.rs
pub mod context;
pub mod entities;
pub mod error;
pub mod filters;
pub mod providers;
pub mod scoped;
pub mod secrets;
pub mod settings;
pub mod setup;
pub mod watchers;

pub use context::{Context, Request, StaticContext};
pub use entities::{McsResponse, Setting, SettingKey};
pub use error::SettingsError;
pub use filters::{check_dynamic_filters, check_static_filters, FilterResult};
pub use providers::{ProviderResponse, SettingsProvider};
pub use scoped::{
    current_context, current_request, set_thread_context, set_thread_request, with_task_context,
    with_task_request, ContextGuard, RequestGuard,
};
pub use secrets::{resolve_secrets, SecretsService};
pub use settings::{RuntimeSettings, RuntimeSettingsBuilder};
pub use setup::{settings, setup, setup_from_env};
pub use watchers::{Watcher, WatcherId, WatchersService};
