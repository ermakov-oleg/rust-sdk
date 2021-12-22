use serde::Deserialize;

mod consts;
mod settings;
mod web;

#[derive(Debug, Deserialize)]
struct PGConnectionString {
    user: String,
    password: String,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::init();

    let runtime_settings = settings::setup().await;
    let key = "SOME_KEY";
    let val: Option<String> = runtime_settings.get(key, &settings::get_context());
    tracing::warn!("Settings {}:{:#?}", key, val);

    web::start_server().await;
    Ok(())
}
