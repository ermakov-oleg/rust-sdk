use axum::{routing::get, Router};
use runtime_settings::{Context, RuntimeSettings};
use std::net::SocketAddr;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Opt {
    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(StructOpt)]
enum Command {
    Serve {
        #[structopt(long, default_value = "127.0.0.1")]
        address: String,
        #[structopt(long, default_value = "8080")]
        port: u16,
    },
}

#[tokio::main]
async fn main() {
    // Initialize logging
    let _guard = struct_log::setup_logger("example-service".to_string(), "dev".to_string());

    let opt = Opt::from_args();

    match opt.cmd {
        Command::Serve { address, port } => {
            // Initialize settings
            let settings = RuntimeSettings::builder()
                .application("example-service")
                .mcs_enabled(false) // Disable MCS for local testing
                .file_path("settings.json")
                .build();

            settings.init().await.expect("Failed to init settings");

            // Test getting a setting
            let ctx = Context {
                application: "example-service".to_string(),
                ..Default::default()
            };
            let _guard = settings.set_context(ctx);

            let some_key: Option<String> = settings.get("SOME_KEY");
            tracing::info!(key = "SOME_KEY", value = ?some_key, "Got setting");

            // Start server
            let app = Router::new().route("/", get(|| async { "Hello, World!" }));

            let addr: SocketAddr = format!("{}:{}", address, port).parse().unwrap();
            tracing::info!("Starting server on {}", addr);

            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
        }
    }
}
