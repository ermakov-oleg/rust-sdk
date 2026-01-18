use std::fmt;

/// Errors that can occur during logger setup
#[derive(Debug)]
pub enum SetupError {
    /// LogTracer already initialized (log -> tracing bridge)
    LogTracerAlreadyInitialized,
    /// Global subscriber already set
    SubscriberAlreadySet,
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LogTracerAlreadyInitialized => {
                write!(f, "log tracer already initialized")
            }
            Self::SubscriberAlreadySet => {
                write!(f, "global tracing subscriber already set")
            }
        }
    }
}

impl std::error::Error for SetupError {}
