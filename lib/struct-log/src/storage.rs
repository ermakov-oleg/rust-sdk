use serde_json::Value;
use std::collections::HashMap;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// Storage for span fields, used to pass context to log events
#[derive(Default)]
pub struct SpanFieldsStorage {
    fields: HashMap<&'static str, Value>,
}

impl SpanFieldsStorage {
    pub fn values(&self) -> &HashMap<&'static str, Value> {
        &self.fields
    }
}

impl Visit for SpanFieldsStorage {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name(), Value::String(format!("{:?}", value)));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name(), Value::String(value.to_owned()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name(), Value::Number(value.into()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name(), Value::Number(value.into()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name(), Value::Bool(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if let Some(number) = serde_json::Number::from_f64(value) {
            self.fields.insert(field.name(), Value::Number(number));
        } else {
            self.fields
                .insert(field.name(), Value::String(value.to_string()));
        }
    }
}

/// Layer that stores span fields in extensions for later retrieval
pub struct StorageLayer;

impl<S> Layer<S> for StorageLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &tracing::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut storage = SpanFieldsStorage::default();
            attrs.record(&mut storage);
            span.extensions_mut().insert(storage);
        }
    }

    fn on_record(&self, id: &tracing::Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            if let Some(storage) = span.extensions_mut().get_mut::<SpanFieldsStorage>() {
                values.record(storage);
            }
        }
    }
}
