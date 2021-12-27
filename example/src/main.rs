use structopt::StructOpt;

use struct_log::setup_logger;

use crate::consts::{APPLICATION_NAME, VERSION};

mod consts;
mod settings;
mod web;

#[derive(Debug, StructOpt)]
#[structopt(name = "classify")]
pub struct ApplicationArguments {
    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(Debug, StructOpt)]
pub enum Command {
    #[structopt(name = "serve")]
    Serve(web::Serve),
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let _guard = setup_logger(APPLICATION_NAME.to_string(), VERSION.to_string());

    runtime_settings::setup().await;
    let key = "SOME_KEY";
    let val: Option<String> = runtime_settings::settings.get(key, &settings::get_context());
    tracing::warn!(key = key, value = ?val, "runtime-settings result");

    let opt = ApplicationArguments::from_args();
    match opt.command {
        Command::Serve(params) => {
            web::start_server(params).await;
        }
    };
    Ok(())
}
