// lib/runtime-settings/src/lib.rs
pub mod context;
pub mod entities;
pub mod error;

pub use context::{Context, Request, StaticContext};
pub use entities::{McsResponse, Setting, SettingKey};
pub use error::SettingsError;
