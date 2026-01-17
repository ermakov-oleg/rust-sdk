// lib/runtime-settings/src/filters/dynamic_filters.rs
use super::{DynamicFilter, FilterResult};
use crate::context::Context;

pub struct UrlPathFilter;
pub struct HostFilter;
pub struct EmailFilter;
pub struct IpFilter;
pub struct HeaderFilter;
pub struct ContextFilter;
pub struct ProbabilityFilter;
