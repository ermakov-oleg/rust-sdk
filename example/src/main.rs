use clap::{Parser, Subcommand};
use std::sync::Arc;

use struct_log::setup_logger;

use crate::consts::{APPLICATION_NAME, VERSION};

mod consts;
mod middleware;
mod web;

#[derive(Debug, Parser)]
#[command(name = "example")]
pub struct ApplicationArguments {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve(web::Serve),
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let _guard = setup_logger(APPLICATION_NAME.to_string(), VERSION.to_string());

    // Initialize runtime settings with new API
    let settings = Arc::new(
        runtime_settings::RuntimeSettings::builder()
            .application(APPLICATION_NAME)
            .mcs_enabled(false) // Disable MCS for local testing
            .file_path("settings.json")
            .build(),
    );

    settings.init().await.expect("Failed to init settings");

    // Test getting a setting at startup (no context needed for basic get)
    let key = "SOME_KEY";
    let val: Option<String> = settings.get(key);
    tracing::warn!(key = key, value = ?val, "runtime-settings result");

    let opt = ApplicationArguments::parse();
    match opt.command {
        Command::Serve(params) => {
            web::start_server(params, settings).await;
        }
    };

    Ok(())
}
