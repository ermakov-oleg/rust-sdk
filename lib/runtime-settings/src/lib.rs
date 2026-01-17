// lib/runtime-settings/src/lib.rs
pub mod entities;
pub mod error;

pub use entities::{McsResponse, Setting, SettingKey};
pub use error::SettingsError;
