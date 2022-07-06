use opentelemetry::{global, sdk, trace::TraceError};

pub fn init_tracer() -> Result<sdk::trace::Tracer, TraceError> {
    global::set_text_map_propagator(propagator::Propagator::new());

    opentelemetry_jaeger::new_pipeline()
        .with_agent_endpoint(format!(
            "{}:6831",
            option_env!("HOST_IP").unwrap_or("127.0.0.1")
        ))
        .with_service_name(env!("CARGO_PKG_NAME"))
        .with_trace_config(sdk::trace::config().with_sampler(sdk::trace::Sampler::AlwaysOn))
        .install_batch(opentelemetry::runtime::Tokio)
}

mod propagator {
    use opentelemetry::{
        global::{self, Error},
        propagation::{text_map_propagator::FieldIter, Extractor, Injector, TextMapPropagator},
        trace::{
            SpanContext, SpanId, TraceContextExt, TraceError, TraceFlags, TraceId, TraceState,
        },
        Context,
    };
    use std::borrow::Cow;
    use std::str::FromStr;

    const JAEGER_HEADER: &str = "X-Trace-Id";
    const JAEGER_BAGGAGE_PREFIX: &str = "X-Trace-ctx-";
    const DEPRECATED_PARENT_SPAN: &str = "0";

    const TRACE_FLAG_DEBUG: TraceFlags = TraceFlags::new(0x04);

    lazy_static::lazy_static! {
        static ref JAEGER_HEADER_FIELD: [String; 1] = [JAEGER_HEADER.to_string()];
    }

    /// The Jaeger propagator propagates span contexts in jaeger's propagation format.
    ///
    /// See [`Jaeger documentation`] for format details.
    ///
    /// Note that jaeger header can be set in http header or encoded as url
    ///
    ///  [`Jaeger documentation`]: https://www.jaegertracing.io/docs/1.18/client-libraries/#propagation-format
    #[derive(Clone, Debug, Default)]
    pub struct Propagator {
        _private: (),
    }

    impl Propagator {
        /// Create a Jaeger propagator
        pub fn new() -> Self {
            Propagator::default()
        }

        /// Extract span context from header value
        fn extract_span_context(&self, extractor: &dyn Extractor) -> Result<SpanContext, ()> {
            let mut header_value = Cow::from(extractor.get(JAEGER_HEADER).unwrap_or(""));
            // if there is no :, it means header_value could be encoded as url, try decode first
            if !header_value.contains(':') {
                header_value = Cow::from(header_value.replace("%3A", ":"));
            }

            let parts = header_value.split_terminator(':').collect::<Vec<&str>>();
            if parts.len() != 4 {
                return Err(());
            }

            // extract trace id
            let trace_id = self.extract_trace_id(parts[0])?;
            let span_id = self.extract_span_id(parts[1])?;
            // Ignore parent span id since it's deprecated.
            let flags = self.extract_trace_flags(parts[3])?;
            let state = self.extract_trace_state(extractor)?;

            Ok(SpanContext::new(trace_id, span_id, flags, true, state))
        }

        /// Extract trace id from the header.
        fn extract_trace_id(&self, trace_id: &str) -> Result<TraceId, ()> {
            if trace_id.len() > 32 {
                return Err(());
            }

            TraceId::from_hex(trace_id).map_err(|_| ())
        }

        /// Extract span id from the header.
        fn extract_span_id(&self, span_id: &str) -> Result<SpanId, ()> {
            if span_id.len() != 16 {
                return Err(());
            }

            SpanId::from_hex(span_id).map_err(|_| ())
        }

        /// Extract flag from the header
        ///
        /// First bit control whether to sample
        /// Second bit control whether it's a debug trace
        /// Third bit is not used.
        /// Forth bit is firehose flag, which is not supported in OT now.
        fn extract_trace_flags(&self, flag: &str) -> Result<TraceFlags, ()> {
            if flag.len() > 2 {
                return Err(());
            }
            let flag = u8::from_str(flag).map_err(|_| ())?;
            if flag & 0x01 == 0x01 {
                if flag & 0x02 == 0x02 {
                    Ok(TraceFlags::SAMPLED | TRACE_FLAG_DEBUG)
                } else {
                    Ok(TraceFlags::SAMPLED)
                }
            } else {
                // Debug flag should only be set when sampled flag is set.
                // So if debug flag is set alone. We will just use not sampled flag
                Ok(TraceFlags::default())
            }
        }

        fn extract_trace_state(&self, extractor: &dyn Extractor) -> Result<TraceState, ()> {
            let uber_context_keys = extractor
                .keys()
                .into_iter()
                .filter(|key| key.starts_with(JAEGER_BAGGAGE_PREFIX))
                .filter_map(|key| {
                    extractor
                        .get(key)
                        .map(|value| (key.to_string(), value.to_string()))
                });

            match TraceState::from_key_value(uber_context_keys) {
                Ok(trace_state) => Ok(trace_state),
                Err(trace_state_err) => {
                    global::handle_error(Error::Trace(TraceError::Other(Box::new(
                        trace_state_err,
                    ))));
                    Err(()) //todo: assign an error type instead of using ()
                }
            }
        }
    }

    impl TextMapPropagator for Propagator {
        fn inject_context(&self, cx: &Context, injector: &mut dyn Injector) {
            let span = cx.span();
            let span_context = span.span_context();
            if span_context.is_valid() {
                let flag: u8 = if span_context.is_sampled() {
                    if span_context.trace_flags() & TRACE_FLAG_DEBUG == TRACE_FLAG_DEBUG {
                        0x03
                    } else {
                        0x01
                    }
                } else {
                    0x00
                };
                let header_value = format!(
                    "{:032x}:{:016x}:{:01}:{:01}",
                    span_context.trace_id(),
                    span_context.span_id(),
                    DEPRECATED_PARENT_SPAN,
                    flag,
                );
                injector.set(JAEGER_HEADER, header_value);
            }
        }

        fn extract_with_context(&self, cx: &Context, extractor: &dyn Extractor) -> Context {
            cx.with_remote_span_context(
                self.extract_span_context(extractor)
                    .unwrap_or_else(|_| SpanContext::empty_context()),
            )
        }

        fn fields(&self) -> FieldIter<'_> {
            FieldIter::new(JAEGER_HEADER_FIELD.as_ref())
        }
    }
}
