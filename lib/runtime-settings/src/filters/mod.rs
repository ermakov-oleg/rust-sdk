// lib/runtime-settings/src/filters/mod.rs
pub mod dynamic_filters;
pub mod static_filters;

use crate::context::{Context, StaticContext};

/// Result of filter check
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterResult {
    Match,
    NoMatch,
    NotApplicable,
}

/// Static filter - checked once when loading settings
pub trait StaticFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult;
}

/// Dynamic filter - checked on every get()
pub trait DynamicFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult;
}

pub use dynamic_filters::*;
pub use static_filters::*;
