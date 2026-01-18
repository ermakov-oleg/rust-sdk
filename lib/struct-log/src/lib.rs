mod error;
mod formatting_layer;
mod setup;

pub use error::SetupError;
pub use formatting_layer::JsonLogLayer;
pub use setup::setup_logger;
