// lib/runtime-settings/src/filters/static_filters.rs
use super::{FilterResult, StaticFilter};
use crate::context::StaticContext;

pub struct ApplicationFilter;
pub struct ServerFilter;
pub struct EnvironmentFilter;
pub struct McsRunEnvFilter;
pub struct LibraryVersionFilter;
