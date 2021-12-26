use runtime_settings;
use struct_log::setup_logger;

use crate::consts::{APPLICATION_NAME, VERSION};

mod consts;
mod settings;
mod web;

#[tokio::main]
async fn main() -> Result<(), ()> {
    let _guard = setup_logger(APPLICATION_NAME.to_string(), VERSION.to_string());

    runtime_settings::setup().await;
    let key = "SOME_KEY";
    let val: Option<String> = runtime_settings::settings.get(key, &settings::get_context());
    tracing::warn!(key = key, value = ?val, "runtime-settings result");

    web::start_server().await;
    Ok(())
}
