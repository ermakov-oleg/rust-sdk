use settings::RUNTIME_SETTINGS;
use struct_log::setup_logger;

use crate::consts::{APPLICATION_NAME, VERSION};

mod consts;
mod settings;
mod web;

#[tokio::main]
async fn main() -> Result<(), ()> {
    let _guard = setup_logger(APPLICATION_NAME.to_string(), VERSION.to_string());

    settings::setup().await;
    let key = "SOME_KEY";
    let val: Option<String> = RUNTIME_SETTINGS.get(key, &settings::get_context());
    tracing::warn!(key = key, value = ?val, "runtime-settings result");

    web::start_server().await;
    Ok(())
}
